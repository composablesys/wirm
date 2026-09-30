//! Regression: the name section's local-name subsection must follow functions
//! when the function index space is reindexed (e.g. when instrumentation inserts imports).

use wasmparser::{Name, Payload};
use wirm::ir::module::Module;

/// Return `(function_index, named_local_count)` for each local-name entry in the
/// module's name section, in encounter order.
fn local_name_entries(wasm: &[u8]) -> Vec<(u32, u32)> {
    let mut entries = vec![];
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let Payload::CustomSection(cs) = payload.unwrap() {
            if let wasmparser::KnownCustom::Name(reader) = cs.as_known() {
                for sub in reader {
                    if let Name::Local(map) = sub.unwrap() {
                        for indirect in map {
                            let indirect = indirect.unwrap();
                            let count = indirect.names.into_iter().count() as u32;
                            entries.push((indirect.index, count));
                        }
                    }
                }
            }
        }
    }
    entries
}

#[test]
fn local_names_follow_function_after_import_insertion() {
    // Two local functions, each carrying named locals (index 0 and 1).
    let wat = r#"
        (module
          (func $big (local $a i32) (local $b i64) (local $c f64)
            nop)
          (func $small (local $x i32)
            nop))
    "#;
    let buff = wat::parse_str(wat).unwrap();
    assert_eq!(local_name_entries(&buff), vec![(0, 3), (1, 1)]);

    let mut module = Module::parse(&buff, false, false).unwrap();

    // insert an imported func
    let ty = module.types.add_func_type(&[], &[]);
    module.add_import_func("env".to_string(), "imp".to_string(), ty);
    let out = module.encode().expect("encode failed");

    // should now be reindexed
    assert_eq!(local_name_entries(&out), vec![(1, 3), (2, 1)]);

    // The emitted module must re-parse cleanly.
    Module::parse(&out, false, false).expect("re-parse failed");
}

#[test]
fn local_names_preserved_on_unmodified_roundtrip() {
    let wat = r#"
        (module
          (func $big (local $a i32) (local $b i64) (local $c f64)
            nop)
          (func $small (local $x i32)
            nop))
    "#;
    let buff = wat::parse_str(wat).unwrap();
    let before = local_name_entries(&buff);
    assert_eq!(before, vec![(0, 3), (1, 1)]);

    // Encoding without any modification must round-trip the local names unchanged.
    let module = Module::parse(&buff, false, false).unwrap();
    let out = module.encode().expect("encode failed");
    assert_eq!(local_name_entries(&out), before);
}
