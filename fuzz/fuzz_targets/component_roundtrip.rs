//! wasm-smith Component → wirm parse → wirm encode → wasmparser validate.
//!
//! Tier-2 analog of `module_roundtrip`: covers the component-model parse
//! and encode paths (which internally drive `walk_topological`).
//!
//! Design per fuzz/DECISIONS.md — parse failures silently skipped, encode
//! or validator errors after a successful parse are bugs.

#![no_main]

use libfuzzer_sys::fuzz_target;
use wasm_smith::Component as SmithComponent;

fuzz_target!(|smith: SmithComponent| {
    let bytes = smith.to_bytes();

    // wasm-smith can produce structurally-parseable but semantically invalid
    // output (e.g. empty `flags` types). Skip those — the comparison we care
    // about is "if wasmparser accepts the input AND wirm parses it, then
    // wirm's re-encode should also be accepted." Anything else is out of
    // scope for this target.
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

    let encoded = comp
        .encode()
        .expect("parsed component failed to re-encode");

    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&encoded)
        .expect("re-encoded component failed wasmparser validation");
});
