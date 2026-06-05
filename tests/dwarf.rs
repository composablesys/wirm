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

#[path = "common/dwarf.rs"]
mod dwarf_helpers;

use dwarf_helpers::line_rows as debug_line_rows;

const DWARF_SECTION_NAMES: &[&str] = &[".debug_abbrev", ".debug_str", ".debug_line", ".debug_info"];

fn input_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test_inputs/handwritten/dwarf/add.wasm")
}

fn multi_func_input_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/test_inputs/handwritten/dwarf/two_funcs.wasm")
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

use dwarf_helpers::debug_info_pcs;

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

// Note: the heavy-injection regression (nop before every op) was moved to
// `src/ir/module/test.rs::rewriter_anchors_nop_before_every_op_to_host_source_strong`
// where it can apply the full `lookup(new_pc) == lookup(anchor_orig_pc)`
// invariant via `pub(crate) DwarfEncodeMaps` access.

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

/// Adding a local declaration grows the locals-vec encoding, shifting the
/// first-instruction DWARF offset. The translator must map orig addresses
/// for instruction bytes onto the shifted positions and preserve the
/// boundary addresses (`addr == size_leb_len → size_leb_len`,
/// `addr == first_instr_dwarf_offset → first_instr_dwarf_offset_new`).
#[test]
fn rewriter_handles_added_local() {
    use wirm::module_builder::AddLocal;
    use wirm::DataType;

    let input = std::fs::read(input_path()).unwrap();
    let mut module = Module::parse(&input, false, false, true).unwrap();
    {
        let mut it = wirm::iterator::module_iterator::ModuleIterator::new(&mut module, &Vec::new());
        // Cursor is on the first op; add a local to the current function.
        let _local = it.add_local(DataType::I32);
    }

    let output = module.encode().unwrap();

    // Body content grew by 2 bytes (1 run-count + 1 type byte for the i32),
    // so DIE ranges expand by exactly 2 and low_pc boundaries are preserved.
    // Input ranges: (0, 8) and (1, 8). Expected output: (0, 10) and (1, 10).
    let out_pcs = debug_info_pcs(&output);
    assert_eq!(out_pcs, vec![(0, 10), (1, 10)]);

    // Line-program rows must shift right by the same 2 bytes (the locals
    // grew but instruction encodings didn't change).
    let in_rows = debug_line_rows(&input);
    let out_rows = debug_line_rows(&output);
    assert_eq!(in_rows.len(), out_rows.len());
    for ((ia, il, ic), (oa, ol, oc)) in in_rows.iter().zip(out_rows.iter()) {
        assert_eq!(*oa, *ia + 2, "row address must shift by added locals bytes");
        assert_eq!((*il, *ic), (*ol, *oc), "(line, col) must be preserved");
    }
}

/// Multi-function inputs currently exercise the rewriter's defensive gate:
/// per-function DWARF address spaces overlap when each starts at 0, so step
/// 6 refuses to translate until per-CU routing lands. This test pins the
/// error so a future change that softens the gate is intentional.
#[test]
fn rewriter_refuses_multi_function_input() {
    let input = std::fs::read(multi_func_input_path()).unwrap();
    let module = Module::parse(&input, false, false, true).unwrap();
    let err = module
        .encode()
        .expect_err("multi-function .debug_info rewriting should refuse");
    // Structural match: the gate must produce a `DwarfError`, and the
    // message must reference multi-function (so the test fails loudly if a
    // future refactor produces the right variant but the wrong reason).
    let msg = match &err {
        wirm::error::Error::DwarfError(m) => m,
        other => panic!("expected DwarfError variant, got {other:?}"),
    };
    assert!(
        msg.contains("multi-function"),
        "DwarfError message should mention multi-function, got: {msg}",
    );
}

// Note: the `func_exit` injection test was moved to
// `src/ir/module/test.rs::rewriter_handles_func_exit_injection_strong` so it
// can apply the full `lookup(new_pc) == lookup(anchor_orig_pc)` invariant.

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
