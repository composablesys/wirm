//! wasm-smith Component → wirm parse → inject `nop` before every op → encode → validate.
//!
//! Tier-2 analog of `module_instrument`: exercises the instrumentation
//! pipeline on a component, which internally iterates function bodies across
//! every nested core module. In addition to "re-encoded output validates",
//! we count operators across every nested module's code section before and
//! after instrumentation and assert the count grew by exactly the number of
//! injections.
//!
//! Design per fuzz/DECISIONS.md — parse/pre-validation failures silent,
//! encode / post-encode validation errors are bugs.

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::collections::HashMap;
use wasm_smith::Component as SmithComponent;
use wirm::iterator::component_iterator::ComponentIterator;
use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
use wirm::Opcode;
use wirm_fuzz::count_ops;

fuzz_target!(|smith: SmithComponent| {
    let bytes = smith.to_bytes();

    if wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&bytes)
        .is_err()
    {
        return;
    }

    let mut comp = match wirm::Component::parse(&bytes, false, false) {
        Ok(c) => c,
        Err(_) => return,
    };

    let pre_count = count_ops(&bytes);

    let mut injected = 0usize;
    {
        let mut it = ComponentIterator::new(&mut comp, HashMap::new());
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

    let encoded = comp
        .encode()
        .expect("instrumented component failed to re-encode");

    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&encoded)
        .expect("instrumented component failed wasmparser validation");

    let post_count = count_ops(&encoded);
    assert_eq!(
        post_count,
        pre_count + injected,
        "injected nops not observable in re-encoded bodies: \
         pre={pre_count}, injected={injected}, post={post_count}",
    );
});
