//! Regression: the name section's local-name subsection must follow functions
//! when the function index space is reindexed (e.g. when instrumentation inserts imports).

use wasmparser::{Name, Payload};
use wirm::ir::id::FunctionID;
use wirm::ir::module::Module;

/// Return `(function_index, name)` for each entry in the module's function-name
/// subsection.
fn function_name_entries(wasm: &[u8]) -> Vec<(u32, String)> {
    let mut entries = vec![];
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let Payload::CustomSection(cs) = payload.unwrap() {
            if let wasmparser::KnownCustom::Name(reader) = cs.as_known() {
                for sub in reader {
                    if let Name::Function(map) = sub.unwrap() {
                        for naming in map {
                            let naming = naming.unwrap();
                            entries.push((naming.index, naming.name.to_string()));
                        }
                    }
                }
            }
        }
    }
    entries
}

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
                            let count = indirect.names.count() as u32;
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

/// Encode an unsigned LEB128 integer.
fn leb(mut n: u32) -> Vec<u8> {
    let mut out = vec![];
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        if n != 0 {
            out.push(b | 0x80);
        } else {
            out.push(b);
            break;
        }
    }
    out
}

/// Encode a name-section string (length-prefixed bytes).
fn name_bytes(s: &str) -> Vec<u8> {
    let mut out = leb(s.len() as u32);
    out.extend_from_slice(s.as_bytes());
    out
}

/// Build a `name` custom section wrapping the given subsections (id, payload).
fn name_custom_section(subs: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut payload = vec![];
    for (id, pl) in subs {
        payload.push(*id);
        payload.extend(leb(pl.len() as u32));
        payload.extend_from_slice(pl);
    }
    let mut body = name_bytes("name");
    body.extend(payload);
    let mut out = vec![0x00]; // custom section id
    out.extend(leb(body.len() as u32));
    out.extend(body);
    out
}

/// Encode a function-name subsection payload from (index, name) pairs.
fn func_names(pairs: &[(u32, &str)]) -> Vec<u8> {
    let mut out = leb(pairs.len() as u32);
    for (idx, nm) in pairs {
        out.extend(leb(*idx));
        out.extend(name_bytes(nm));
    }
    out
}

/// Encode a local-name subsection payload: a vec of (function index, vec of (local index, name)).
fn local_names(entries: &[(u32, &[(u32, &str)])]) -> Vec<u8> {
    let mut out = leb(entries.len() as u32);
    for (func_idx, locals) in entries {
        out.extend(leb(*func_idx));
        out.extend(leb(locals.len() as u32));
        for (local_idx, nm) in *locals {
            out.extend(leb(*local_idx));
            out.extend(name_bytes(nm));
        }
    }
    out
}

#[test]
fn malformed_func_name_index_is_ignored_not_fatal() {
    // A valid two-function module with no name section of its own.
    let wat = r#"
        (module
          (func (param i32) (result i32) local.get 0)
          (func (export "_start") nop))
    "#;
    let mut buff = wat::parse_str(wat).unwrap();

    // Append a name section that names function 0 (valid) and function 99
    // (well past the two functions in the module). The bogus entry must be
    // ignored, not crash the parser.
    buff.extend(name_custom_section(&[(
        1,
        func_names(&[(0, "callee"), (99, "ghost")]),
    )]));

    let module = Module::parse(&buff, false, false).expect("parse must not panic");
    // Re-encoding the parsed module must also succeed.
    module.encode().expect("encode failed");
}

#[test]
fn malformed_local_name_index_is_ignored_not_fatal() {
    // A valid two-function module with no name section of its own.
    let wat = r#"
        (module
          (func (param i32) (result i32) local.get 0)
          (func (export "_start") nop))
    "#;
    let mut buff = wat::parse_str(wat).unwrap();

    // A local-name subsection naming function 77 (well past the two functions
    // in the module) must be ignored, not crash the parser.
    buff.extend(name_custom_section(&[(
        2,
        local_names(&[(77, &[(0, "a")])]),
    )]));

    let module = Module::parse(&buff, false, false).expect("parse must not panic");
    module.encode().expect("encode failed");
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

#[test]
fn set_fn_name_writes_function_name() {
    let wat = r#"
        (module
          (func $a nop)
          (func $b nop))
    "#;
    let buff = wat::parse_str(wat).unwrap();
    let mut module = Module::parse(&buff, false, false).unwrap();
    module.set_fn_name(FunctionID(1), "renamed".to_string());
    let out = module.encode().expect("encode failed");

    // Function 1's name is updated; function 0's original name is untouched.
    let names = function_name_entries(&out);
    assert!(
        names.contains(&(1, "renamed".to_string())),
        "function 1 should be named `renamed`, got {names:?}"
    );
    assert!(
        names.contains(&(0, "a".to_string())),
        "function 0's name should be preserved, got {names:?}"
    );
}
