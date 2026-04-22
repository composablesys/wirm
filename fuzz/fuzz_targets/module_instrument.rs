//! wasm-smith → wirm parse → inject `nop` before every op → encode → validate.
//!
//! Exercises the instrumentation pipeline (iterator + injection) on top of the
//! parse/encode surface that `module_roundtrip` already covers. In addition to
//! "re-encoded output validates", we count operators in the binary before and
//! after instrumentation and assert the count grew by exactly the number of
//! injections — catches silent drops where `.before().nop()` is accepted but
//! not emitted.
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
use wirm_fuzz::count_ops;

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

    let pre_count = count_ops(&bytes);

    let mut injected = 0usize;
    {
        let mut it = ModuleIterator::new(&mut module, &vec![]);
        loop {
            if it.curr_op().is_some() {
                it.before().nop();
                injected += 1;
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

    let post_count = count_ops(&encoded);
    assert_eq!(
        post_count,
        pre_count + injected,
        "injected nops not observable in re-encoded body: \
         pre={pre_count}, injected={injected}, post={post_count}",
    );
});
