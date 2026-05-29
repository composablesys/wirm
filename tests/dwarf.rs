//! Tests for parse-aside DWARF handling (`with_dwarf = true`).
//!
//! When `with_dwarf` is on, `.debug_*` custom sections lift into
//! `Module::debug` instead of `custom_sections`, and encode re-emits the
//! section bytes verbatim.
//!
//! We assert on *DWARF section content* (not whole-module bytes) because
//! wirm regenerates other sections like `name` and isn't byte-identical
//! end-to-end even without instrumentation.

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

/// `with_dwarf=true` populates `Module::debug` with the `.debug_*` sections
/// in encounter order, removes them from `custom_sections`, and re-emits them
/// verbatim during encode.
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
    assert_eq!(debug_section_bytes(&input), debug_section_bytes(&output));
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
