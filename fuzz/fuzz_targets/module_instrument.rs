//! wasm-smith → wirm parse → force ID shifts → inject `nop` before every op
//! → encode → validate → assert no operator still references the shifted-out
//! index.
//!
//! Two complementary checks beyond a plain re-encode-and-validate:
//!
//! - **Operator count**: post == pre + injected. Catches silent drops where
//!   `.before().nop()` is accepted but not emitted.
//! - **No-stale-reference**: before injecting we add an imported memory,
//!   global, and function. Imports append, so each new import lands at
//!   index = `original_imported_count_of_kind` and any local entries of
//!   that kind shift up by one. Every operator referencing such a local
//!   has to be rewritten by the reindex pass. After encode we walk the
//!   operator stream and assert that no op still references the just-added
//!   import — if reindex skipped an op (e.g., because its variant wasn't in
//!   the categorization table), that op would still point at its original
//!   local index, which is now the freshly-added import.
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
use wasmparser::MemoryType;
use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
use wirm::iterator::module_iterator::ModuleIterator;
use wirm::{DataType, Opcode};
use wirm_fuzz::{assert_no_reference_to, count_ops};

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

    let mut module = match wirm::Module::parse(&bytes, false, false, false) {
        Ok(m) => m,
        Err(_) => return,
    };

    let pre_count = count_ops(&bytes);

    // Force ID shifts in the func/global/memory index spaces. Each new
    // import lands at index = original_imported_count_of_kind in the
    // final encoded output, and any local entries of that kind shift up
    // by one. This makes every operator that references one of those
    // locals go through the reindex path.
    let forbidden_mem = module.num_import_memory();
    let forbidden_global = module.num_import_global();
    let forbidden_func = module.num_import_func();

    module.add_import_memory(
        "wirm_fuzz".to_string(),
        "forced_mem".to_string(),
        MemoryType {
            memory64: false,
            shared: false,
            initial: 0,
            maximum: None,
            page_size_log2: None,
        },
    );
    module.add_imported_global(
        "wirm_fuzz".to_string(),
        "forced_global".to_string(),
        DataType::I32,
        false,
        false,
    );
    let new_func_type = module.types.add_func_type(&[], &[]);
    module.add_import_func(
        "wirm_fuzz".to_string(),
        "forced_func".to_string(),
        new_func_type,
    );

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

    assert_no_reference_to(
        &encoded,
        forbidden_func,
        forbidden_global,
        forbidden_mem,
    );
});
