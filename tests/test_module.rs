use log::{debug, error};
use std::path::PathBuf;
use std::process::Command;
use wirm::ir::function::FunctionBuilder;
use wirm::ir::id::{ExportsID, FunctionID, ImportsID, TypeID};
use wirm::ir::module::module_functions::FuncKind::{Import, Local};
use wirm::ir::module::module_functions::{ImportedFunction, LocalFunction};
use wirm::ir::types::{Body, InitExpr, Value};
use wirm::{DataType, InitInstr, Module, Opcode};

mod common;
use crate::common::check_instrumentation_encoding;

#[test]
fn test_fn_types() {
    let file = "tests/test_inputs/handwritten/modules/add.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let module = Module::parse(&buff, false).expect("Unable to parse module");

    assert_eq!(
        *module.functions.get_kind(FunctionID(0)),
        Import(ImportedFunction::new(
            ImportsID(0),
            TypeID(2),
            FunctionID(0)
        ))
    );
    assert_eq!(
        *module.functions.get_kind(FunctionID(1)),
        Local(Box::new(LocalFunction::new(
            TypeID(5),
            FunctionID(0),
            Body::default(),
            0,
            None
        )))
    );
    assert_eq!(
        *module.functions.get_kind(FunctionID(2)),
        Local(Box::new(LocalFunction::new(
            TypeID(0),
            FunctionID(0),
            Body::default(),
            0,
            None
        )))
    );
}

#[test]
fn test_exports() {
    let file = "tests/test_inputs/instr_testing/modules/function_modification/export_deletion.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    // Get func ID by name
    assert_eq!(
        module.exports.get_func_by_name("add".to_string()).unwrap(),
        FunctionID(1)
    );

    // Get Export ID by name
    assert_eq!(
        module
            .exports
            .get_export_id_by_name("add".to_string())
            .unwrap(),
        ExportsID(0)
    );

    // Check deletion
    let id = module
        .exports
        .get_export_id_by_name("add".to_string())
        .unwrap();
    module.exports.delete(id);

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_exports.wasm"),
        true,
    );
}

#[test]
fn test_import_delete() {
    let file = "tests/test_inputs/instr_testing/modules/function_modification/import_delete.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    let id = module.imports.find("bogus".to_string(), "hi".to_string());
    let fid = module
        .imports
        .get_func("bogus".to_string(), "hi".to_string());
    assert!(id.is_some());
    assert!(fid.is_some());

    let id = id.unwrap();
    let fid = fid.unwrap();
    assert_eq!(*id, *fid);
    module.delete_func(fid);

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_import_delete.wasm"),
        true,
    );
}

#[test]
fn test_local_fn_delete() {
    let file = "tests/test_inputs/instr_testing/modules/function_modification/local_fn_delete.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    module.delete_func(FunctionID(2));

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_local_fn_delete.wasm"),
        true,
    );
}

#[test]
#[should_panic]
fn test_panic_call_delete() {
    let file = "tests/test_inputs/handwritten/modules/add.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    module.delete_func(FunctionID(1));

    // Should panic here as func 2 calls func 1 which has been deleted
    module.encode();
}

#[test]
fn test_renumber_fn_id() {
    let file =
        "tests/test_inputs/instr_testing/modules/function_modification/local_fn_renumber.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    module.delete_func(FunctionID(1));

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_renumber_fn_id.wasm"),
        true,
    );
}

#[test]
fn test_middle_import_to_local() {
    let file =
        "tests/test_inputs/instr_testing/modules/function_modification/middle_import_to_local.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    let mut builder = FunctionBuilder::new(&[DataType::I32, DataType::I32], &[]);
    builder.i32_const(1);
    builder.drop();

    builder.replace_import_in_module(&mut module, ImportsID(1));

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_middle_import_to_local.wasm"),
        true,
    );
}

#[test]
fn test_first_import_to_local() {
    let file =
        "tests/test_inputs/instr_testing/modules/function_modification/first_import_to_local.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    let mut builder = FunctionBuilder::new(&[DataType::I32, DataType::I32], &[]);
    builder.i32_const(1);
    builder.drop();

    builder.replace_import_in_module(&mut module, ImportsID(0));

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_first_import_to_local.wasm"),
        true,
    );
}

#[test]
fn test_last_import_to_local() {
    let file =
        "tests/test_inputs/instr_testing/modules/function_modification/last_import_to_local.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    let mut builder = FunctionBuilder::new(&[DataType::I32, DataType::I32], &[]);
    builder.i32_const(1);
    builder.drop();

    builder.replace_import_in_module(&mut module, ImportsID(2));

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_last_import_to_local.wasm"),
        true,
    );
}

#[test]
fn test_all_import_to_local() {
    let file =
        "tests/test_inputs/instr_testing/modules/function_modification/all_import_to_local.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    // Convert all to local
    let mut first_builder = FunctionBuilder::new(&[DataType::I32, DataType::I32], &[]);
    first_builder.i32_const(1);
    first_builder.drop();
    first_builder.replace_import_in_module(&mut module, ImportsID(0));

    let mut second_builder = FunctionBuilder::new(&[DataType::I32, DataType::I32], &[]);
    second_builder.i32_const(2);
    second_builder.drop();
    second_builder.replace_import_in_module(&mut module, ImportsID(1));

    let mut third_builder = FunctionBuilder::new(&[DataType::I32, DataType::I32], &[]);
    third_builder.i32_const(3);
    third_builder.drop();
    third_builder.replace_import_in_module(&mut module, ImportsID(2));

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_all_import_to_local.wasm"),
        true,
    );
}

#[test]
fn test_some_import_to_local() {
    let file =
        "tests/test_inputs/instr_testing/modules/function_modification/some_import_to_local.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    // Convert all to local
    let mut first_builder = FunctionBuilder::new(&[DataType::I32, DataType::I32], &[]);
    first_builder.i32_const(1);
    first_builder.drop();
    first_builder.replace_import_in_module(&mut module, ImportsID(0));

    let mut second_builder = FunctionBuilder::new(&[DataType::I32, DataType::I32], &[]);
    second_builder.i32_const(2);
    second_builder.drop();
    second_builder.replace_import_in_module(&mut module, ImportsID(1));

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_some_import_to_local.wasm"),
        true,
    );
}

#[test]
fn test_middle_import_to_local_import_delete() {
    let file =
        "tests/test_inputs/instr_testing/modules/function_modification/middle_import_to_local_import_delete.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    let mut builder = FunctionBuilder::new(&[DataType::I32, DataType::I32], &[]);
    builder.i32_const(1);
    builder.drop();

    builder.replace_import_in_module(&mut module, ImportsID(1));

    module.delete_func(FunctionID(2));

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_middle_import_to_local_import_delete.wasm"),
        true,
    );
}

#[test]
fn test_middle_import_to_local_local_delete() {
    let file =
        "tests/test_inputs/instr_testing/modules/function_modification/middle_import_to_local_local_delete.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    let mut builder = FunctionBuilder::new(&[DataType::I32, DataType::I32], &[]);
    builder.i32_const(1);
    builder.drop();

    builder.replace_import_in_module(&mut module, ImportsID(1));

    module.delete_func(FunctionID(2));
    module.delete_func(FunctionID(3));

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_middle_import_to_local_local_delete.wasm"),
        true,
    );
}

#[test]
fn test_add_import() {
    let file = "tests/test_inputs/instr_testing/modules/function_modification/add_import.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    module.add_import_func("wirm".to_string(), "better".to_string(), TypeID(2));

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_add_import.wasm"),
        true,
    );
}

#[test]
fn test_middle_local_to_import() {
    let file =
        "tests/test_inputs/instr_testing/modules/function_modification/middle_local_to_import.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    module.convert_local_fn_to_import(
        FunctionID(2),
        "wirm".to_string(),
        "better".to_string(),
        TypeID(2),
    );

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_middle_local_to_import.wasm"),
        true,
    );
}

#[test]
fn test_first_local_to_import() {
    let file =
        "tests/test_inputs/instr_testing/modules/function_modification/first_local_to_import.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    module.convert_local_fn_to_import(
        FunctionID(1),
        "wirm".to_string(),
        "better".to_string(),
        TypeID(2),
    );

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_first_local_to_import.wasm"),
        true,
    );
}

#[test]
fn test_last_local_to_import() {
    let file =
        "tests/test_inputs/instr_testing/modules/function_modification/last_local_to_import.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    module.convert_local_fn_to_import(
        FunctionID(3),
        "wirm".to_string(),
        "better".to_string(),
        TypeID(2),
    );

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_last_local_to_import.wasm"),
        true,
    );
}

#[test]
fn test_all_local_to_import() {
    let file =
        "tests/test_inputs/instr_testing/modules/function_modification/all_local_to_import.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    module.convert_local_fn_to_import(
        FunctionID(3),
        "all".to_string(),
        "local".to_string(),
        TypeID(2),
    );
    module.convert_local_fn_to_import(
        FunctionID(4),
        "to".to_string(),
        "import".to_string(),
        TypeID(2),
    );
    module.convert_local_fn_to_import(
        FunctionID(5),
        "please".to_string(),
        "work".to_string(),
        TypeID(2),
    );

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_all_local_to_import.wasm"),
        true,
    );
}

#[test]
fn test_some_local_to_import() {
    let file =
        "tests/test_inputs/instr_testing/modules/function_modification/some_local_to_import.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    module.convert_local_fn_to_import(
        FunctionID(3),
        "all".to_string(),
        "local".to_string(),
        TypeID(2),
    );
    module.convert_local_fn_to_import(
        FunctionID(4),
        "to".to_string(),
        "import".to_string(),
        TypeID(2),
    );

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_some_local_to_import.wasm"),
        true,
    );
}

#[test]
fn test_all_local_to_import_all_import_to_local() {
    let file = "tests/test_inputs/instr_testing/modules/function_modification/all_local_to_import_all_import_to_local.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    // Convert all to local
    let mut first_builder = FunctionBuilder::new(&[DataType::I32, DataType::I32], &[]);
    first_builder.i32_const(4);
    first_builder.drop();
    first_builder.replace_import_in_module(&mut module, ImportsID(0));

    let mut second_builder = FunctionBuilder::new(&[DataType::I32, DataType::I32], &[]);
    second_builder.i32_const(5);
    second_builder.drop();
    second_builder.replace_import_in_module(&mut module, ImportsID(1));

    let mut third_builder = FunctionBuilder::new(&[DataType::I32, DataType::I32], &[]);
    third_builder.i32_const(6);
    third_builder.drop();
    third_builder.replace_import_in_module(&mut module, ImportsID(2));

    module.convert_local_fn_to_import(
        FunctionID(3),
        "all".to_string(),
        "local".to_string(),
        TypeID(2),
    );
    module.convert_local_fn_to_import(
        FunctionID(4),
        "to".to_string(),
        "import".to_string(),
        TypeID(2),
    );
    module.convert_local_fn_to_import(
        FunctionID(5),
        "please".to_string(),
        "work".to_string(),
        TypeID(2),
    );

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_all_local_to_import_all_import_to_local.wasm"),
        true,
    );
}

#[test]
fn test_add_fns_init_exprs() {
    let file = "tests/test_inputs/instr_testing/modules/init-exprs.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    // add first import func
    let (..) = module.add_import_func("test0".to_string(), "func0".to_string(), TypeID(4));

    // add first local func
    let mut first_builder = FunctionBuilder::new(&[], &[]);
    first_builder.i32_const(1);
    first_builder.i32_const(1);
    first_builder.i32_add();
    first_builder.drop();
    let fid0 = first_builder.finish_module(&mut module);

    // add second local func
    let mut sec_builder = FunctionBuilder::new(&[], &[]);
    sec_builder.i32_const(2);
    sec_builder.drop();
    sec_builder.call(fid0);
    sec_builder.finish_module(&mut module);

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/init-exprs.wasm"),
        false,
    )
}

#[test]
fn test_add_imports_and_local_fns() {
    let file = "tests/test_inputs/instr_testing/modules/function_modification/add_imported_and_local_funcs.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    // add first import func
    let (fid, ..) = module.add_import_func("test0".to_string(), "func0".to_string(), TypeID(2));

    // add first local func
    let mut first_builder = FunctionBuilder::new(&[], &[]);
    first_builder.i32_const(1);
    first_builder.i32_const(1);
    first_builder.call(fid);
    let fid0 = first_builder.finish_module(&mut module);

    // add second local func
    let mut sec_builder = FunctionBuilder::new(&[], &[]);
    sec_builder.i32_const(2);
    sec_builder.drop();
    sec_builder.call(fid0);
    sec_builder.finish_module(&mut module);

    // add second import func
    module.add_import_func("test1".to_string(), "func1".to_string(), TypeID(2));
    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/test_add_imports_and_local_fns.wasm"),
        true,
    )
}

#[test]
fn add_global_with_import() {
    let file = "tests/test_inputs/instr_testing/modules/function_modification/add_global.wat";

    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse module");

    // add new global
    let gid = module.add_global(
        InitExpr::new(vec![InitInstr::Value(Value::I32(0))]),
        DataType::I32,
        true,
        false,
    );
    assert_eq!(1, *gid);

    check_validity(
        file,
        &mut module,
        &format!("{TEST_DEBUG_DIR}/add_global_with_import.wasm"),
        true,
    );
}

#[test]
fn test_elem_reindexing() {
    let output_path = format!("{TEST_DEBUG_DIR}/test_elems.wasm");
    // This wat inserts two table entries of type () -> ().
    // Running "check" will trap if the table entries have the wrong type.
    let wat = r#"
        (module
            (table 2 2 funcref)
            (type $f_type (func))
            (func $f)
            (elem (i32.const 0) $f)
            (elem (i32.const 1) funcref (ref.func $f))
            (func (export "check") 
                (call_indirect (type $f_type) (i32.const 0))
                (call_indirect (type $f_type) (i32.const 1))
            )
        )"#;
    let buff = wat::parse_str(wat).unwrap();
    let mut module = Module::parse(&buff, false).unwrap();

    // Add an import of a different type. Then the table will have entries of
    // the wrong type unless the element section is reindexed.
    let ty_id = module.types.add_func_type(&[DataType::I32], &[]);
    let _ = module.add_import_func("".to_string(), "".to_string(), ty_id);
    validate(&module.encode(), &output_path).unwrap();

    // Run the check function to assert that entries in the table have the expected types.
    let engine = wasmtime::Engine::default();
    let mut linker = wasmtime::Linker::new(&engine);
    let module = wasmtime::Module::from_file(&engine, &output_path).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    linker
        .func_wrap("", "", |_: wasmtime::Caller<_>, _: i32| {})
        .unwrap();

    let instance = linker.instantiate(&mut store, &module).unwrap();
    let check_func = instance
        .get_typed_func::<(), ()>(&mut store, "check")
        .unwrap();
    check_func.call(&mut store, ()).unwrap();
}

const TEST_DEBUG_DIR: &str = "output/tests/debug_me/test_module/";

/// create output path if it doesn't exist
pub(crate) fn try_path(path: &str) {
    if !PathBuf::from(path).exists() {
        std::fs::create_dir_all(PathBuf::from(path).parent().unwrap()).unwrap();
    }
}

pub(crate) fn validate(wasm: &Vec<u8>, output_wasm_path: &str) -> Result<(), std::io::Error> {
    try_path(output_wasm_path);
    std::fs::write(output_wasm_path, wasm)?;
    validate_wasm(output_wasm_path);
    Ok(())
}

pub(crate) fn validate_wasm(wasm_path: &str) -> bool {
    debug!("Running 'wasm-tools validate' on file: {wasm_path}");
    let res = Command::new("wasm-tools")
        .arg("validate")
        .arg("-f")
        .arg("legacy-exceptions")
        .arg(wasm_path)
        .output()
        .expect("failed to execute process");
    if !res.status.success() {
        println!("{:?}", std::str::from_utf8(&res.stderr).unwrap());
        assert!(false)
    }

    res.status.success()
}

fn check_validity(file: &str, module: &mut Module, output_wasm_path: &str, check_encoding: bool) {
    let result = module.encode();
    validate(&result, output_wasm_path).expect("Failed to write out to wasm file.");

    if check_encoding {
        let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
        if let Err(e) = check_instrumentation_encoding(&out, file) {
            error!(
                "Something went wrong when checking instrumentation encoding: {}",
                e
            )
        }
    }
}
