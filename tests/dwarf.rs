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

fn from_rust_input_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/test_inputs/handwritten/dwarf/from-rust/from-rust.wasm")
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

/// Multi-function inputs use a shared module-cumulative DWARF address space.
/// An uninstrumented round-trip must preserve every row address and every DIE
/// range — the rewriter has nothing to translate.
#[test]
fn rewriter_preserves_row_addresses_uninstrumented_multi_func() {
    let input = std::fs::read(multi_func_input_path()).unwrap();
    let in_rows = debug_line_rows(&input);
    let in_pcs = debug_info_pcs(&input);

    let module = Module::parse(&input, false, false, true).unwrap();
    let output = module.encode().unwrap();
    let out_rows = debug_line_rows(&output);
    let out_pcs = debug_info_pcs(&output);

    let in_addrs: Vec<u64> = in_rows.iter().map(|r| r.0).collect();
    let out_addrs: Vec<u64> = out_rows.iter().map(|r| r.0).collect();
    assert_eq!(
        in_addrs, out_addrs,
        "uninstrumented multi-function round-trip must preserve row addresses",
    );
    assert_eq!(
        in_pcs, out_pcs,
        ".debug_info ranges must match for an uninstrumented multi-function round-trip",
    );

    // Spot-check the fixture so a future regeneration that shifts addresses
    // surfaces in the test, not silently in downstream invariants.
    // CU spans both functions; foo and bar each occupy one subprogram DIE.
    assert_eq!(in_pcs, vec![(0, 16), (1, 8), (9, 16)]);
}

/// Injecting into the second function must shift its body forward and grow
/// every DIE / line-program address that lives past the first function. The
/// first function's addresses must stay put.
#[test]
fn rewriter_handles_instrumentation_in_second_function() {
    use wasmparser::Operator;
    use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
    use wirm::Opcode;

    let input = std::fs::read(multi_func_input_path()).unwrap();
    let in_pcs = debug_info_pcs(&input);
    assert_eq!(in_pcs, vec![(0, 16), (1, 8), (9, 16)], "fixture sanity check");

    let mut module = Module::parse(&input, false, false, true).unwrap();
    // Inject 3 nops before bar's `i32.mul`. Bar grows by 3 bytes; foo stays.
    // Assert we hit a bar instruction (func index 1) so a future fixture
    // change that put `i32.mul` into foo couldn't silently patch the wrong
    // function.
    {
        let mut it = wirm::iterator::module_iterator::ModuleIterator::new(&mut module, &Vec::new());
        loop {
            if matches!(it.curr_op(), Some(Operator::I32Mul)) {
                let (loc, _) = it.curr_loc();
                let wirm::Location::Module { func_idx, .. } = loc else {
                    panic!("expected module-level location, got {loc:?}");
                };
                assert_eq!(
                    *func_idx, 1u32,
                    "fixture changed: i32.mul should still be in bar (func 1)",
                );
                it.before().nop().nop().nop();
                break;
            }
            if it.next().is_none() {
                break;
            }
        }
    }

    let output = module.encode().unwrap();
    let out_pcs = debug_info_pcs(&output);

    // foo unchanged: subprogram still (1, 8). bar grows by 3: low_pc still 9
    // (its base shifts only if foo grew, which it didn't), high_pc 16 → 19.
    // CU spans [0, foo_total + bar_total) = [0, 8 + 11) = [0, 19).
    assert_eq!(out_pcs, vec![(0, 19), (1, 8), (9, 19)]);
}

/// Instrumenting the first function shifts the second function's base forward.
/// Both DIE ranges and `.debug_line` rows for the second function must track
/// the new base, while the first function's addresses adjust to its own growth.
#[test]
fn rewriter_handles_instrumentation_in_first_function() {
    use wasmparser::Operator;
    use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
    use wirm::Opcode;

    let input = std::fs::read(multi_func_input_path()).unwrap();
    let mut module = Module::parse(&input, false, false, true).unwrap();
    // 2 nops before foo's `i32.add`. foo grows by 2 bytes; bar's base shifts.
    // Assert we land in foo (func 0); a fixture change that put `i32.add` in
    // bar would otherwise silently mis-attribute the growth.
    {
        let mut it = wirm::iterator::module_iterator::ModuleIterator::new(&mut module, &Vec::new());
        loop {
            if matches!(it.curr_op(), Some(Operator::I32Add)) {
                let (loc, _) = it.curr_loc();
                let wirm::Location::Module { func_idx, .. } = loc else {
                    panic!("expected module-level location, got {loc:?}");
                };
                assert_eq!(
                    *func_idx, 0u32,
                    "fixture changed: i32.add should still be in foo (func 0)",
                );
                it.before().nop().nop();
                break;
            }
            if it.next().is_none() {
                break;
            }
        }
    }

    let output = module.encode().unwrap();
    let out_pcs = debug_info_pcs(&output);

    // foo grows by 2: (1, 8) → (1, 10). bar's base shifts by foo's 2-byte growth:
    // bar's body is unchanged so its range slides forward by 2: (9, 16) → (11, 18).
    // CU: (0, 16) → (0, 18).
    assert_eq!(out_pcs, vec![(0, 18), (1, 10), (11, 18)]);

    // `.debug_line` rows for bar must shift right by 2 (foo's growth), since
    // bar's body is unchanged but its base address slid forward. Filter both
    // sides to bar's region (addr ≥ original bar low_pc = 9). Out has 2 more
    // rows than in (the two injected nops anchor onto i32.add and produce
    // extra in-foo rows), so length isn't comparable globally.
    let in_rows = debug_line_rows(&input);
    let out_rows = debug_line_rows(&output);
    let bar_in: Vec<_> = in_rows.iter().filter(|(a, _, _)| *a >= 9).collect();
    let bar_out: Vec<_> = out_rows.iter().filter(|(a, _, _)| *a >= 11).collect();
    assert_eq!(bar_in.len(), bar_out.len(), "bar row count must match");
    for ((ia, il, ic), (oa, ol, oc)) in bar_in.iter().zip(bar_out.iter()) {
        assert_eq!((*il, *ic), (*ol, *oc), "(line, col) must be preserved");
        assert_eq!(
            *oa,
            *ia + 2,
            "bar row at addr {ia} must shift by foo's 2-byte growth, got {oa}",
        );
    }
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

/// Real rustc-emitted DWARF (v4, multi-function, inlined subroutines,
/// rangelist CU, `DW_FORM_addr` low/high_pc, `dead code` tombstones) must
/// round-trip + re-validate. The strong source-location invariant is checked
/// crate-side in `src/ir/module/test.rs` (it needs `pub(crate)` access to
/// the DWARF encode maps); here we just confirm the pipeline doesn't error
/// out on realistic DWARF.
#[test]
fn from_rust_uninstrumented_round_trips_and_validates() {
    let input = std::fs::read(from_rust_input_path()).unwrap();
    let module = Module::parse(&input, false, false, true).unwrap();
    let output = module.encode().unwrap();
    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&output)
        .expect("rewritten module must validate");
}

/// Same with one nop injected before every op. Encode must succeed and the
/// output must re-validate; the strong invariant is verified crate-side.
#[test]
fn from_rust_instrumented_round_trips_and_validates() {
    use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
    use wirm::Opcode;

    let input = std::fs::read(from_rust_input_path()).unwrap();
    let mut module = Module::parse(&input, false, false, true).unwrap();
    {
        let mut it = wirm::iterator::module_iterator::ModuleIterator::new(&mut module, &Vec::new());
        loop {
            if it.curr_op().is_some() {
                it.before().nop();
            }
            if it.next().is_none() {
                break;
            }
        }
    }
    let output = module.encode().unwrap();
    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&output)
        .expect("instrumented module must validate");
}
