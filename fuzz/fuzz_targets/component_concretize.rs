//! wasm-smith Component → wirm parse → concretize each import/export.
//!
//! Exercises `Component::concretize_import` and `Component::concretize_export`
//! — the alias-chain-walking, index-resolving translation that returns a
//! `ConcreteType` with no remaining indices. Any smith-produced component
//! wirm parses should survive concretize calls on every named import/export
//! without panicking. We don't care about the result shape here, only that
//! neither function panics on well-formed input.
//!
//! Design per fuzz/DECISIONS.md — parse/pre-validation failures silent,
//! any panic below that line is a bug.

#![no_main]

use libfuzzer_sys::fuzz_target;
use wasm_smith::Component as SmithComponent;

fuzz_target!(|smith: SmithComponent| {
    let bytes = smith.to_bytes();

    if wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&bytes)
        .is_err()
    {
        return;
    }

    let comp = match wirm::Component::parse(&bytes, false, false, false) {
        Ok(c) => c,
        Err(_) => return,
    };

    for import in comp.imports.iter() {
        let _ = comp.concretize_import(import.name.0);
    }
    for export in comp.exports.iter() {
        let _ = comp.concretize_export(export.name.0);
    }
});
