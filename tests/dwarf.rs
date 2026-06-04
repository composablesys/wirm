//! Tests for parse-aside DWARF handling and the `.debug_line` rewriter.
//!
//! When `with_dwarf` is on, `.debug_*` custom sections lift into
//! `Module::debug` instead of `custom_sections`. The other DWARF sections
//! still round-trip byte-identically (no rewriter for them yet); `.debug_line`
//! is rewritten so its row addresses match the new code layout. We verify
//! `.debug_line` semantically (rows) rather than by bytes because gimli's
//! writer may choose more compact opcodes than the input.

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

/// `with_dwarf=true` lifts `.debug_*` aside and re-emits them. The non-line
/// sections round-trip byte-identically (no rewriter for them yet);
/// `.debug_line` is rewritten so the bytes may differ but the logical rows
/// must match for an uninstrumented module.
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
    let in_sections = debug_section_bytes(&input);
    let out_sections = debug_section_bytes(&output);
    // Non-line sections still pass through byte-for-byte.
    for name in DWARF_SECTION_NAMES.iter().filter(|n| **n != ".debug_line") {
        assert_eq!(
            in_sections.get(*name),
            out_sections.get(*name),
            "{name} should round-trip byte-identically (no rewriter for it)",
        );
    }
    // `.debug_line` must be semantically equivalent: same rows for an
    // uninstrumented module.
    assert_eq!(
        debug_line_rows(&input),
        debug_line_rows(&output),
        ".debug_line rows must match for an uninstrumented round-trip",
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
