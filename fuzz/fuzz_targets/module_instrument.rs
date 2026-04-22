//! wasm-smith → wirm parse → inject `nop` before every op → encode → validate.
//!
//! Exercises the instrumentation pipeline (iterator + injection) on top of the
//! parse/encode surface that `module_roundtrip` already covers. Any smith-
//! produced module wirm accepts should survive a trivial injection pass and
//! re-encode to valid wasm.
//!
//! Design per fuzz/DECISIONS.md:
//!
//! - Parse failures on the smith-produced input are silently skipped (wirm
//!   can't be expected to support every feature wasm-smith emits).
//! - Encode or validator errors *after* a successful parse are bugs — wirm
//!   claimed the input and the instrumentation pipeline is on the hook for
//!   producing valid output.

#![no_main]

use libfuzzer_sys::fuzz_target;
use wasm_smith::Module as SmithModule;
use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
use wirm::iterator::module_iterator::ModuleIterator;
use wirm::Opcode;

fuzz_target!(|smith: SmithModule| {
    let bytes = smith.to_bytes();

    // Skip inputs wasmparser itself rejects — see module_roundtrip for
    // the rationale.
    if wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&bytes)
        .is_err()
    {
        return;
    }

    let mut module = match wirm::Module::parse(&bytes, false, false) {
        Ok(m) => m,
        Err(_) => return,
    };

    {
        let mut it = ModuleIterator::new(&mut module, &vec![]);
        loop {
            if it.curr_op().is_some() {
                it.before().nop();
            }
            if it.next().is_none() {
                break;
            }
        }
    }

    let encoded = module
        .encode()
        .expect("instrumented module failed to re-encode");

    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&encoded)
        .expect("instrumented module failed wasmparser validation");
});
