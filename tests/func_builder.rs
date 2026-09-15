use std::process::Command;
use wirm::ir::function::FunctionBuilder;
use wirm::ir::id::{FunctionID, TypeID};
use wirm::iterator::iterator_trait::Iterator;
use wirm::iterator::module_iterator::ModuleIterator;
use wirm::opcode::Instrumenter;
use wirm::{DataType, Opcode};
use wirm::{Location, Module};

/// Regression test for https://github.com/composablesys/wirm/issues/158
///
/// Building a function with `FunctionBuilder::set_name` should retain the
/// name across encoding/parsing — the user should not be required to call
/// `module.set_fn_name(id, name)` after `finish_module` to keep the name.
#[test]
fn func_builder_set_name_is_retained_after_round_trip() {
    let mut module = Module::default();

    // Build a function with a name set only via `FunctionBuilder::set_name`.
    let expected = "via_builder_set_name";
    let mut builder = FunctionBuilder::new(&[], &[]);
    builder.i32_const(1);
    builder.drop();
    builder.set_name(expected.to_string());
    let fid = builder.finish_module(&mut module);

    // Pre-encode: the IR itself should reflect the name set via the builder.
    assert_eq!(
        module.functions.get_name(fid).as_ref().map(String::as_str),
        Some(expected),
        "expected the name set via FunctionBuilder::set_name to be present in the IR before encoding"
    );

    // Round-trip the module and confirm the name section preserves the name.
    let bytes = module.encode().expect("encode failed");
    let reparsed = Module::parse(&bytes, false, false, false).expect("reparse failed");
    assert_eq!(
        reparsed
            .functions
            .get_name(fid)
            .as_ref()
            .map(String::as_str),
        Some(expected),
        "expected FunctionBuilder::set_name to be retained through the wasm name section"
    );
}

/// Same as above but on top of a parsed module that already has functions
/// and a populated name section.
#[test]
fn func_builder_set_name_is_retained_when_appended_to_parsed_module() {
    let file_name = "tests/test_inputs/handwritten/modules/_start.wat";
    let wasm = wat::parse_file(file_name).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&wasm, false, false, false).expect("Unable to parse");

    let expected = "appended_via_builder";
    let mut builder = FunctionBuilder::new(&[], &[]);
    builder.i32_const(1);
    builder.drop();
    builder.set_name(expected.to_string());
    let fid = builder.finish_module(&mut module);

    let bytes = module.encode().expect("encode failed");
    let reparsed = Module::parse(&bytes, false, false, false).expect("reparse failed");
    assert_eq!(
        reparsed
            .functions
            .get_name(fid)
            .as_ref()
            .map(String::as_str),
        Some(expected),
        "expected the appended function's name to survive the round-trip"
    );
}

#[test]
// build factorial from scratch
fn run_fac_wirm() {
    // run cargo run in fac_wirm dir
    let res = Command::new("cargo")
        .arg("run")
        .current_dir("fac_wirm")
        .output()
        .expect("failed to execute process");
    if !res.status.success() {
        println!("{}", std::str::from_utf8(&res.stdout).unwrap());
        println!("{}", std::str::from_utf8(&res.stderr).unwrap());
    }
    assert!(res.status.success());

    let fac_generated = wasmprinter::print_file("fac_wirm/target/out.wasm").unwrap();
    let fac_standard = wasmprinter::print_file("fac_wirm/fact.wasm").unwrap();
    assert_eq!(fac_generated, fac_standard);
}

// #[test]
// test start function instrumentation with FunctionModifier
#[allow(dead_code)]
fn run_start_wirm() {
    let file_name = "tests/test_inputs/handwritten/modules/start.wat";
    let wasm = wat::parse_file(file_name).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&wasm, false, false, false).expect("Unable to parse");

    let start_fun_id = module.start.unwrap();
    let mut function_builder = module.functions.get_fn_modifier(start_fun_id).unwrap();

    function_builder
        .before_at(Location::Module {
            func_idx: FunctionID(0), // not used
            instr_idx: 0,
        })
        .i32_const(1);

    let result = module.encode().expect("error");
    let out = wasmprinter::print_bytes(result.clone()).expect("couldn't translate Wasm to wat");
    println!("{}", out);
}

#[ignore]
#[test]
// test start function instrumentation with FunctionModifier
fn run_start_wirm_default() {
    let file_name = "tests/test_inputs/handwritten/modules/start.wat";
    let wasm = wat::parse_file(file_name).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&wasm, false, false, false).expect("Unable to parse");

    let start_fun_id = module.start.unwrap();
    let mut function_builder = module.functions.get_fn_modifier(start_fun_id).unwrap();

    function_builder.i32_const(1);

    let result = module.encode().expect("error!");
    let out = wasmprinter::print_bytes(result.clone()).expect("couldn't translate Wasm to wat");
    println!("{}", out);
}
#[test]
// test start function instrumentation with FunctionModifier
fn add_import_and_local_fn_then_iterate() {
    let file_name = "tests/test_inputs/handwritten/modules/_start.wat";
    let wasm = wat::parse_file(file_name).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&wasm, false, false, false).expect("Unable to parse");
    // add an imported function AND THEN a new local function
    module.add_import_func("new".to_string(), "import".to_string(), TypeID(0));
    assert_eq!(module.num_import_func(), 1);

    let params = vec![];
    let results = vec![DataType::I32];

    let mut new_func = FunctionBuilder::new(&params, &results);
    new_func.i32_const(1);
    new_func.finish_module(&mut module);

    // now iterate over module
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);
    loop {
        let _op = mod_it.curr_op();
        if mod_it.next().is_none() {
            break;
        };
    }

    let result = module.encode().expect("error!");
    let out = wasmprinter::print_bytes(result.clone()).expect("couldn't translate Wasm to wat");
    println!("{}", out);
}
