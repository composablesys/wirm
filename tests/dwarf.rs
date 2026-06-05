//! Tests for parse-aside DWARF handling, the `.debug_line` rewriter (step 5),
//! and the address-translated rewriter for the other DWARF sections (step 6).
//!
//! When `with_dwarf` is on, `.debug_*` custom sections lift into
//! `Module::debug` instead of `custom_sections`. `.debug_line` is rewritten
//! with anchor-aware row inheritance; `.debug_info` (+ `.debug_abbrev`,
//! `.debug_str` etc.) is rewritten via `gimli::write::Dwarf::from` with an
//! address translator. Both rewriters preserve semantics, not byte content,
//! so the tests check logical equivalence rather than byte equality.

use std::collections::BTreeMap;
use std::path::PathBuf;

use wirm::ir::module::Module;

const DWARF_SECTION_NAMES: &[&str] = &[".debug_abbrev", ".debug_str", ".debug_line", ".debug_info"];

fn input_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test_inputs/handwritten/dwarf/add.wasm")
}

/// Pulls every `.debug_*` custom section's bytes out, keyed by name.
/// `BTreeMap` so equality is order-independent — encode may legitimately
/// place DWARF sections in a different position relative to other custom
/// sections.
fn debug_section_bytes(wasm: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let wasmparser::Payload::CustomSection(cs) = payload.expect("valid wasm") {
            if cs.name().starts_with(".debug_") {
                out.insert(cs.name().to_string(), cs.data().to_vec());
            }
        }
    }
    out
}

/// Walks `.debug_info` and collects each DIE's `(low_pc, high_pc)` pair. Used
/// for semantic comparison since the rewriter may emit different bytes (e.g.
/// different abbreviation codes) but must preserve address ranges.
fn debug_info_pcs(wasm: &[u8]) -> Vec<(u64, u64)> {
    let sections = debug_section_bytes(wasm);
    let endian = gimli::LittleEndian;
    let lookup = |name: &str| -> gimli::EndianSlice<'_, gimli::LittleEndian> {
        gimli::EndianSlice::new(
            sections.get(name).map(|v| v.as_slice()).unwrap_or(&[]),
            endian,
        )
    };
    let dwarf = gimli::read::Dwarf::load(|id| -> Result<_, gimli::Error> {
        Ok(match id {
            gimli::SectionId::DebugInfo => lookup(".debug_info"),
            gimli::SectionId::DebugAbbrev => lookup(".debug_abbrev"),
            gimli::SectionId::DebugStr => lookup(".debug_str"),
            gimli::SectionId::DebugLine => lookup(".debug_line"),
            gimli::SectionId::DebugLineStr => lookup(".debug_line_str"),
            _ => gimli::EndianSlice::new(&[], endian),
        })
    })
    .expect("load DWARF");

    let mut out = Vec::new();
    let mut units = dwarf.units();
    while let Some(header) = units.next().expect("unit header") {
        let unit = dwarf.unit(header).expect("unit");
        let mut entries = unit.entries();
        while let Some(entry) = entries.next_dfs().expect("dfs") {
            // wasm-tools' CU stores low_pc/high_pc as DW_FORM_data4 (low_pc =
            // absolute, high_pc = length per the DWARF spec); the subprogram
            // uses DW_FORM_addr for low_pc and data4 for high_pc. Accept both.
            let read_uint = |v: gimli::read::AttributeValue<_>| -> Option<u64> {
                match v {
                    gimli::read::AttributeValue::Addr(a) => Some(a),
                    gimli::read::AttributeValue::Data1(d) => Some(d as u64),
                    gimli::read::AttributeValue::Data2(d) => Some(d as u64),
                    gimli::read::AttributeValue::Data4(d) => Some(d as u64),
                    gimli::read::AttributeValue::Data8(d) => Some(d),
                    gimli::read::AttributeValue::Udata(d) => Some(d),
                    _ => None,
                }
            };
            let low = entry.attr_value(gimli::DW_AT_low_pc).and_then(read_uint);
            let high_raw = entry.attr_value(gimli::DW_AT_high_pc).and_then(read_uint);
            // For Addr form high_pc is absolute; for Data*/Udata it's a length.
            let high = match entry.attr_value(gimli::DW_AT_high_pc) {
                Some(gimli::read::AttributeValue::Addr(_)) => high_raw,
                Some(_) => high_raw.zip(low).map(|(l_len, l_addr)| l_addr + l_len),
                None => None,
            };
            if let (Some(l), Some(h)) = (low, high) {
                out.push((l, h));
            }
        }
    }
    out
}

/// Parses the input's `.debug_line` via gimli and collects each non-end-of-
/// sequence row as `(address, line, column)`. Used for semantic comparison
/// between input and rewritten outputs.
fn debug_line_rows(wasm: &[u8]) -> Vec<(u64, u64, u64)> {
    let dl_bytes = debug_section_bytes(wasm)
        .remove(".debug_line")
        .expect("input has .debug_line");
    let endian = gimli::LittleEndian;
    let slice = gimli::EndianSlice::new(&dl_bytes, endian);
    let dl = gimli::read::DebugLine::new(slice.slice(), endian);
    let program = dl
        .program(gimli::DebugLineOffset(0), 4, None, None)
        .expect("line program parses");
    let mut rows = program.rows();
    let mut out = Vec::new();
    while let Some((_header, row)) = rows.next_row().expect("row reads") {
        if row.end_sequence() {
            continue;
        }
        let line = row.line().map(|n| n.get()).unwrap_or(0);
        let column = match row.column() {
            gimli::ColumnType::LeftEdge => 0,
            gimli::ColumnType::Column(c) => c.get(),
        };
        out.push((row.address(), line, column));
    }
    out
}

/// `with_dwarf=false` leaves DWARF flowing through `custom_sections`. The
/// section bytes themselves still round-trip verbatim, since `custom_sections`
/// has always been opaque pass-through.
#[test]
fn opaque_round_trip_preserves_dwarf_section_bytes() {
    let input = std::fs::read(input_path()).unwrap();
    let module = Module::parse(&input, false, false, false).unwrap();
    assert!(
        module.debug.is_none(),
        "with_dwarf=false should leave Module::debug as None",
    );

    let output = module.encode().unwrap();
    assert_eq!(debug_section_bytes(&input), debug_section_bytes(&output));
}

/// `with_dwarf=true` lifts `.debug_*` aside and re-emits them. For an
/// uninstrumented round-trip the rewriters preserve DWARF semantics: every
/// `.debug_line` row and every `.debug_info` `(low_pc, high_pc)` pair must
/// match input. Byte equality is not required since gimli's encoder is free
/// to pick different abbreviation codes, opcode sequences, etc.
#[test]
fn parse_aside_lifts_dwarf_into_debug_field() {
    let input = std::fs::read(input_path()).unwrap();
    let module = Module::parse(&input, false, false, true).unwrap();

    let debug = module.debug.as_ref().expect("debug present");
    let names: Vec<&str> = debug.sections().iter().map(|s| s.name).collect();
    assert_eq!(names, DWARF_SECTION_NAMES);

    for section in module.custom_sections.iter() {
        assert!(
            !section.name.starts_with(".debug_"),
            "{} leaked into custom_sections",
            section.name,
        );
    }

    let output = module.encode().unwrap();
    assert_eq!(
        debug_line_rows(&input),
        debug_line_rows(&output),
        ".debug_line rows must match for an uninstrumented round-trip",
    );
    assert_eq!(
        debug_info_pcs(&input),
        debug_info_pcs(&output),
        ".debug_info DIE address ranges must match for an uninstrumented round-trip",
    );
}

/// `with_dwarf=true` on a module with no DWARF still yields `Some(empty)` —
/// distinguishes "user opted in, no DWARF was present" from "user didn't opt
/// in".
#[test]
fn parse_aside_empty_when_input_has_no_dwarf() {
    let wat = "(module (func))";
    let wasm = wat::parse_str(wat).expect("wat compiles");
    let module = Module::parse(&wasm, false, false, true).unwrap();
    let debug = module
        .debug
        .as_ref()
        .expect("debug present even when empty");
    assert!(debug.sections().is_empty());
}

/// Instrumented round-trip: injecting 4 nops before `i32.add` grows the
/// function body by 4 bytes; `.debug_info`'s `(low_pc, high_pc)` must
/// reflect the new size, not the original. This is the regression case the
/// `Dwarf::convert` fix in step 6 was added for.
#[test]
fn rewriter_translates_high_pc_after_body_growth() {
    use wasmparser::Operator;
    use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
    use wirm::Opcode;

    let input = std::fs::read(input_path()).unwrap();
    let in_pcs = debug_info_pcs(&input);
    assert_eq!(in_pcs, vec![(0, 8), (1, 8)], "fixture sanity check");

    let mut module = Module::parse(&input, false, false, true).unwrap();
    {
        let mut it = wirm::iterator::module_iterator::ModuleIterator::new(&mut module, &Vec::new());
        loop {
            if matches!(it.curr_op(), Some(Operator::I32Add)) {
                it.before().nop().nop().nop().nop();
                break;
            }
            if it.next().is_none() {
                break;
            }
        }
    }

    let output = module.encode().unwrap();
    let out_pcs = debug_info_pcs(&output);

    // Body grew by 4 bytes. CU spans the whole body: (0, 8+4)=(0,12).
    // Subprogram starts after the size LEB at the locals byte: (1, 12).
    assert_eq!(
        out_pcs,
        vec![(0, 12), (1, 12)],
        ".debug_info DIE ranges must reflect the post-injection body size",
    );
}

/// Alternate-path coverage: replacing `i32.add` (1 byte) with `nop nop nop`
/// (3 bytes) grows the body by 2 bytes. This exercises `self_emit_for_orig`
/// on the alt branch — the orig op `i32.add` is *not* emitted, but its
/// self-emit slot must still point at the first alt instruction so any DIE
/// addressing the orig op lands on the alt's start byte.
#[test]
fn rewriter_handles_alternate_replacement() {
    use wasmparser::Operator;
    use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
    use wirm::Opcode;

    let input = std::fs::read(input_path()).unwrap();
    let mut module = Module::parse(&input, false, false, true).unwrap();
    {
        let mut it = wirm::iterator::module_iterator::ModuleIterator::new(&mut module, &Vec::new());
        loop {
            if matches!(it.curr_op(), Some(Operator::I32Add)) {
                it.alternate().nop().nop().nop();
                break;
            }
            if it.next().is_none() {
                break;
            }
        }
    }

    let output = module.encode().unwrap();
    let out_pcs = debug_info_pcs(&output);

    // Body grew by 2 bytes (3 nops − 1 i32.add). CU/subprogram extents track.
    assert_eq!(
        out_pcs,
        vec![(0, 10), (1, 10)],
        ".debug_info DIE ranges must reflect the alt-induced body size delta",
    );

    // Sanity-check `.debug_line`: the i32.add's row must be addressable in
    // the rewritten output. With the alt path, `self_emit_for_orig[i32.add]`
    // points at the first nop's emit position, so the row stays present at
    // the alt's start byte (offset_in_op = 0).
    let out_rows = debug_line_rows(&output);
    assert!(
        !out_rows.is_empty(),
        ".debug_line must still cover the function after alt replacement",
    );
}

/// Rewritten DWARF lands in the output module's custom sections in
/// input order — re-emit is order-preserving so downstream tooling that
/// scans `.debug_*` payloads doesn't have to deal with reshuffled section
/// layouts.
#[test]
fn rewriter_preserves_dwarf_section_order_in_output() {
    let input = std::fs::read(input_path()).unwrap();
    let module = Module::parse(&input, false, false, true).unwrap();
    let output = module.encode().unwrap();

    let mut out_names: Vec<String> = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(&output) {
        if let wasmparser::Payload::CustomSection(cs) = payload.expect("valid wasm") {
            if cs.name().starts_with(".debug_") {
                out_names.push(cs.name().to_string());
            }
        }
    }
    let input_prefix: Vec<String> = DWARF_SECTION_NAMES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert!(
        out_names.starts_with(&input_prefix),
        "input `.debug_*` sections must appear in input order at the head of \
         the output: expected prefix {input_prefix:?}, got {out_names:?}",
    );
    // For add.wasm, gimli additionally materializes `.debug_line_str` (the v5
    // path table) during the .debug_line rewrite. Pin the exact tail so a
    // future change that drops or reorders gimli additions is caught.
    assert_eq!(
        &out_names[input_prefix.len()..],
        &[".debug_line_str".to_string()],
    );
}

/// Explicit address-translation invariant for the rewriter: for an
/// uninstrumented module the rewritten rows must use the same addresses as
/// the input, because new layout equals orig layout.
#[test]
fn rewriter_preserves_row_addresses_uninstrumented() {
    let input = std::fs::read(input_path()).unwrap();
    let in_rows = debug_line_rows(&input);

    let module = Module::parse(&input, false, false, true).unwrap();
    let output = module.encode().unwrap();
    let out_rows = debug_line_rows(&output);

    let in_addrs: Vec<u64> = in_rows.iter().map(|r| r.0).collect();
    let out_addrs: Vec<u64> = out_rows.iter().map(|r| r.0).collect();
    assert_eq!(
        in_addrs, out_addrs,
        "uninstrumented round-trip must preserve row addresses",
    );

    // Spot-check the test data so a future refactor that silently loses rows
    // is caught: add.wasm has rows at addrs 2, 4, 6, 7.
    assert_eq!(in_addrs, vec![2, 4, 6, 7]);
}
