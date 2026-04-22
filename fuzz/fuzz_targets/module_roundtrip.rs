//! wasm-smith → wirm parse → wirm encode → wasmparser validate.
//!
//! Design per fuzz/DECISIONS.md:
//!
//! - Parse failures on the smith-produced input are silently skipped.
//!   wasm-smith can emit features wirm doesn't support (e.g.
//!   `shared_everything_threads`, `stack_switching`); those aren't bugs.
//! - Encode errors and validator errors after a successful parse ARE bugs —
//!   wirm claimed the input, so wirm is on the hook for producing valid
//!   output. Those panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use wasm_smith::Module as SmithModule;

fuzz_target!(|smith: SmithModule| {
    let bytes = smith.to_bytes();

    let module = match wirm::Module::parse(&bytes, false, false) {
        Ok(m) => m,
        Err(_) => return,
    };

    let encoded = module
        .encode()
        .expect("parsed module failed to re-encode");

    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&encoded)
        .expect("re-encoded module failed wasmparser validation");
});
