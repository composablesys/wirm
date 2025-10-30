// ==================================
// ==== Manipulation API Testing ====
// ==================================

use crate::ir::function::FunctionBuilder;
use crate::ir::id::{CustomSectionID, ExportsID, FunctionID, GlobalID, ImportsID, TypeID};
use crate::ir::types::{CustomSection, CustomSections, InitExpr};
use crate::{DataType, InitInstr, Module, Opcode};
use log::debug;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

// FUNCTIONS
#[test]
fn test_add_local_func() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    // add local func
    let mut builder = FunctionBuilder::new(&[], &[]);
    builder.i32_const(1);
    builder.drop();
    assert_eq!(
        FunctionID(init_state.func_count + 1),
        builder.finish_module(&mut module)
    );
    init_state.add_local_func();

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &HashMap::new(),
        "test_add_local_func",
    );
}

#[test]
fn test_add_import_func() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    // add imported func
    let (fid, imp0) = module.add_import_func("test0".to_string(), "func0".to_string(), TypeID(0));
    assert_eq!(init_state.next_fid(), *fid); // zero-based, no '+ 1'
    assert_eq!(init_state.next_imp_id(), *imp0); // zero-based, no '+ 1'
    init_state.add_imported_func();

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &HashMap::new(),
        "test_add_import_func",
    );
}

#[test]
fn test_add_local_then_imported_func() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    // add local function
    let mut builder = FunctionBuilder::new(&[], &[]);
    builder.i32_const(1);
    builder.drop();
    assert_eq!(init_state.next_fid(), *builder.finish_module(&mut module));
    init_state.add_local_func();

    // add imported func
    let (fid, imp0) = module.add_import_func("test0".to_string(), "func0".to_string(), TypeID(0));
    assert_eq!(init_state.next_fid(), *fid); // zero-based, no '+ 1'
    assert_eq!(init_state.next_imp_id(), *imp0); // zero-based, no '+ 1'
    init_state.add_imported_func();

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &HashMap::new(),
        "test_add_local_then_imported_func",
    );
}

#[test]
fn test_add_imported_then_local_func() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    // add imported func
    let (fid, imp0) = module.add_import_func("test0".to_string(), "func0".to_string(), TypeID(0));
    assert_eq!(init_state.next_fid(), *fid); // zero-based, no '+ 1'
    assert_eq!(init_state.next_imp_id(), *imp0); // zero-based, no '+ 1'
    init_state.add_imported_func();

    // add local function using the imported function
    let mut builder = FunctionBuilder::new(&[], &[DataType::I32]);
    builder.i32_const(1);
    builder.i32_const(1);
    builder.call(fid);
    assert_eq!(init_state.next_fid(), *builder.finish_module(&mut module));
    init_state.add_local_func();

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &HashMap::new(),
        "test_add_imported_then_local_func",
    );
}

#[test]
fn test_add_then_delete_local_func() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    // add local function
    let mut builder = FunctionBuilder::new(&[], &[]);
    builder.i32_const(1);
    builder.drop();
    let fid = builder.finish_module(&mut module);
    assert_eq!(init_state.next_fid(), *fid);
    init_state.add_local_func();

    module.delete_func(fid);
    assert!(module.functions.is_deleted(fid));
    init_state.delete_local_func();

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &HashMap::new(),
        "test_add_then_delete_local_func",
    );
}

#[test]
fn test_delete_local_func() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    // delete local function
    module.delete_func(FunctionID(52)); // unused in wat file!
    init_state.delete_local_func();

    module.exports.delete(ExportsID(49));

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &HashMap::new(),
        "test_delete_local_func",
    );
}

#[test]
fn test_add_then_delete_imported_func() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    // add imported func
    let (fid, imp0) = module.add_import_func("test0".to_string(), "func0".to_string(), TypeID(0));
    assert_eq!(init_state.next_fid(), *fid); // zero-based, no '+ 1'
    assert_eq!(init_state.next_imp_id(), *imp0); // zero-based, no '+ 1'
    init_state.add_imported_func();

    // delete imported function
    module.delete_func(fid); // unused in wat file!
    init_state.delete_imported_func();

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &HashMap::new(),
        "test_add_then_delete_imported_func",
    );
}

#[test]
fn test_delete_imported_func() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    // delete imported function
    module.delete_func(FunctionID(0)); // unused in wat file!
    init_state.delete_imported_func();

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &HashMap::new(),
        "test_delete_imported_func",
    );
}

#[test]
fn test_delete_local_and_imported_func() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    // delete local function
    module.delete_func(FunctionID(52)); // unused in wat file!
    init_state.delete_local_func();

    module.exports.delete(ExportsID(49));

    // delete imported function
    module.delete_func(FunctionID(0)); // unused in wat file!
    init_state.delete_imported_func();

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &HashMap::new(),
        "test_delete_local_and_imported_func",
    );
}

#[test]
fn test_convert_import_fn_to_local() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    // convert the import to a function
    let mut builder = FunctionBuilder::new(&[DataType::I32], &[DataType::I32]);
    builder.i32_const(1);
    builder.drop();
    builder.replace_import_in_module(&mut module, ImportsID(0));

    // add local function using the translated function
    let mut builder = FunctionBuilder::new(&[], &[DataType::I32]);
    builder.i32_const(1);
    builder.i32_const(1);
    builder.call(FunctionID(0));
    assert_eq!(init_state.next_fid(), *builder.finish_module(&mut module));
    init_state.add_local_func();
    init_state.import_func_to_local();

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &HashMap::new(),
        "test_convert_import_fn_to_local",
    );
}

#[test]
fn test_convert_local_fn_to_import() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    // convert local func to import
    module.convert_local_fn_to_import(
        FunctionID(52),
        "please".to_string(),
        "work".to_string(),
        TypeID(1),
    ); // unused in wat file!
    init_state.local_func_to_import();

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &HashMap::new(),
        "test_convert_local_fn_to_import",
    );
}

#[test]
fn test_set_fn_name_import_through_import() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    let mut new_import_names = HashMap::new();
    module.imports.set_name("test".to_string(), ImportsID(0));
    new_import_names.insert(ImportsID(0), "test".to_string());

    is_valid(
        &mut module,
        &mut init_state,
        &new_import_names,
        &HashMap::new(),
        "test_set_fn_name_import_through_import",
    );
}

#[test]
fn test_set_fn_name_import_through_module() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    let mut new_import_names = HashMap::new();
    module.set_fn_name(FunctionID(0), "test".to_string());
    new_import_names.insert(ImportsID(0), "test".to_string());

    is_valid(
        &mut module,
        &mut init_state,
        &new_import_names,
        &HashMap::new(),
        "test_set_fn_name_import_through_module",
    );
}

#[test]
fn test_set_fn_name_local_through_functions() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    let fid = FunctionID(10);
    let mut new_func_names = HashMap::new();
    module.functions.set_local_fn_name(fid, "test".to_string());
    new_func_names.insert(fid, "test".to_string());

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &new_func_names,
        "test_set_fn_name_local_through_functions",
    );
}

#[test]
fn test_set_fn_name_local_through_module() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    let fid = FunctionID(10);
    let mut new_func_names = HashMap::new();
    module.set_fn_name(fid, "test".to_string());
    new_func_names.insert(fid, "test".to_string());

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &new_func_names,
        "test_set_fn_name_local_through_module",
    );
}

#[test]
fn test_set_fn_name_local_through_func_builder() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    let mut new_func_names = HashMap::new();

    // add local function
    let name = "test0";
    let mut builder = FunctionBuilder::new(&[], &[]);
    builder.i32_const(1);
    builder.drop();
    builder.set_name(name.to_string());
    let fid = builder.finish_module(&mut module);

    assert_eq!(init_state.next_fid(), *fid);
    init_state.add_local_func();
    new_func_names.insert(fid, name.to_string());

    // add local function
    let name = "other";
    let mut builder = FunctionBuilder::new(&[], &[]);
    builder.i32_const(1);
    builder.drop();
    builder.set_name("test1".to_string());
    let fid = builder.finish_module(&mut module);

    assert_eq!(init_state.next_fid(), *fid);
    init_state.add_local_func();
    module.set_fn_name(fid, name.to_string());
    new_func_names.insert(fid, name.to_string());

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &new_func_names,
        "test_set_fn_name_local_through_func_builder",
    );
}

// GLOBALS

#[test]
fn test_create_and_add_global() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    // add a local global
    let gid = module.add_global(
        InitExpr::new(vec![InitInstr::Value(crate::ir::types::Value::I32(0))]),
        DataType::I32,
        true,
        false,
    );
    assert_eq!(init_state.next_gid(), *gid);
    init_state.add_local_global();

    // add a function using the new global
    let mut builder = FunctionBuilder::new(&[], &[]);
    builder.global_get(gid);
    builder.drop();
    let fid = builder.finish_module(&mut module);

    assert_eq!(init_state.next_fid(), *fid);
    init_state.add_local_func();

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &HashMap::new(),
        "test_create_and_add_global",
    );
}

#[test]
fn test_add_imported_global() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    // add an imported global
    let (gid, imp_id) = module.add_imported_global(
        "knock knock".to_string(),
        "gimme a global".to_string(),
        DataType::I32,
        true,
        false,
    );
    assert_eq!(init_state.next_gid(), *gid);
    assert_eq!(init_state.next_imp_id(), *imp_id);
    init_state.add_imported_global();

    // add a function using the new global
    let mut builder = FunctionBuilder::new(&[], &[]);
    builder.global_get(gid);
    builder.drop();
    let fid = builder.finish_module(&mut module);

    assert_eq!(init_state.next_fid(), *fid);
    init_state.add_local_func();

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &HashMap::new(),
        "test_add_imported_global",
    );
}

#[test]
fn test_delete_global() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    module.delete_global(GlobalID(2));
    init_state.delete_local_global();

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &HashMap::new(),
        "test_delete_global",
    );
}

#[test]
fn test_delete_imported_global() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    module.delete_global(GlobalID(0));
    init_state.delete_imported_global();

    is_valid(
        &mut module,
        &mut init_state,
        &HashMap::new(),
        &HashMap::new(),
        "test_delete_imported_global",
    );
}

// ==========================
// ==== HELPER UTILITIES ====
// ==========================

struct State {
    // import state
    import_count: u32,
    import_func_count: u32,
    import_global_count: u32,
    import_table_count: u32,
    import_memory_count: u32,
    import_tag_count: u32,

    // import additions
    import_funcs_added: u32,
    import_tables_added: u32,
    import_globals_added: u32,
    import_memories_added: u32,
    import_tags_added: u32,

    // local state
    func_count: u32,
    global_count: u32,
    table_count: u32,
    memory_count: u32,
    #[allow(dead_code)]
    tag_count: u32,
}
impl State {
    fn clear_temporal(&mut self) {
        self.import_funcs_added = 0;
        self.import_tables_added = 0;
        self.import_globals_added = 0;
        self.import_memories_added = 0;
        self.import_tags_added = 0;
    }
    // IMPORTS
    fn next_imp_id(&self) -> u32 {
        self.import_count
    }
    fn add_import(&mut self) {
        self.import_count += 1;
    }
    fn delete_import(&mut self) {
        self.import_count -= 1;
    }
    // FUNCTIONS

    fn next_fid(&self) -> u32 {
        self.import_func_count + self.func_count
    }
    fn add_imported_func(&mut self) {
        self.add_import();
        self.import_funcs_added += 1;
        self.import_func_count += 1;
    }
    fn add_local_func(&mut self) {
        self.func_count += 1;
    }
    fn delete_local_func(&mut self) {
        self.func_count -= 1;
    }
    fn delete_imported_func(&mut self) {
        self.delete_import();
        self.import_func_count -= 1;
    }
    fn import_func_to_local(&mut self) {
        self.delete_imported_func();
        self.add_local_func();
    }
    fn local_func_to_import(&mut self) {
        self.delete_local_func();
        self.add_imported_func();
    }
    // GLOBALS

    fn next_gid(&self) -> u32 {
        self.import_global_count + self.global_count
    }
    fn add_imported_global(&mut self) {
        self.add_import();
        self.import_globals_added += 1;
        self.import_global_count += 1;
    }
    fn add_local_global(&mut self) {
        self.global_count += 1;
    }
    fn delete_local_global(&mut self) {
        self.global_count -= 1;
    }
    fn delete_imported_global(&mut self) {
        self.delete_import();
        self.import_global_count -= 1;
    }
}

fn setup() -> (Vec<u8>, State) {
    let file = "tests/test_inputs/spec-test/modules/if.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let init_state = State {
        // import state
        import_count: 4,
        import_func_count: 1,
        import_global_count: 1,
        import_table_count: 1,
        import_memory_count: 1,
        import_tag_count: 0, // todo test with tags!

        // import additions
        import_funcs_added: 0,
        import_tables_added: 0,
        import_globals_added: 0,
        import_memories_added: 0,
        import_tags_added: 0,

        // local state
        func_count: 53,
        global_count: 2,
        table_count: 1,
        memory_count: 1,
        tag_count: 0, // todo test with tags!
    };

    (buff, init_state)
}

const TEST_DEBUG_DIR: &str = "output/tests/debug_me/ir.module.test/";

/// create output path if it doesn't exist
pub(crate) fn try_path(path: &str) {
    if !PathBuf::from(path).exists() {
        std::fs::create_dir_all(PathBuf::from(path).parent().unwrap()).unwrap();
    }
}

fn is_valid(
    module: &mut Module,
    state: &mut State,
    new_import_names: &HashMap<ImportsID, String>,
    new_fn_names: &HashMap<FunctionID, String>,
    test_name: &str,
) {
    state_assertions(module, state, true);

    // encode and write to file
    let output_wasm_path = format!("{TEST_DEBUG_DIR}/{test_name}.wasm");
    encode_and_validate_wasm(module, &output_wasm_path);

    // reload from file
    let buff = std::fs::read(output_wasm_path).unwrap();
    let new_module = Module::parse(&buff, false, false).expect("Unable to parse");

    for (id, name) in new_import_names {
        assert_eq!(
            name,
            new_module.imports.get_import_name(*id).as_ref().unwrap()
        )
    }

    for (id, name) in new_fn_names {
        assert_eq!(name, new_module.functions.get_name(*id).as_ref().unwrap())
    }

    // make sure state assertions pass on the reloaded module!
    state.clear_temporal();
    state_assertions(&new_module, state, false)
}

fn state_assertions(module: &Module, state: &State, only_temporal: bool) {
    // import additions
    assert_eq!(state.import_funcs_added, module.imports.num_funcs_added);
    assert_eq!(state.import_globals_added, module.imports.num_globals_added);
    assert_eq!(state.import_tables_added, module.imports.num_tables_added);
    assert_eq!(
        state.import_memories_added,
        module.imports.num_memories_added
    );
    assert_eq!(state.import_tags_added, module.imports.num_tags_added);
    if only_temporal {
        return;
    }

    // import state
    assert_eq!(state.import_count, module.imports.len() as u32);
    assert_eq!(state.import_func_count, module.num_import_func());
    assert_eq!(state.import_func_count, module.imports.num_funcs);
    assert_eq!(state.import_global_count, module.imports.num_globals);
    assert_eq!(state.import_table_count, module.imports.num_tables);
    assert_eq!(state.import_memory_count, module.imports.num_memories);
    assert_eq!(state.import_tag_count, module.imports.num_tags);

    // local state
    assert_eq!(state.func_count, module.num_local_functions);
    assert_eq!(state.global_count, module.num_local_globals);
    assert_eq!(state.table_count, module.num_local_tables);
    assert_eq!(state.memory_count, module.num_local_memories);
}

pub(crate) fn encode_and_validate_wasm(module: &mut Module, output_wasm_path: &str) {
    try_path(output_wasm_path);
    if let Err(e) = module.emit_wasm(output_wasm_path) {
        panic!(
            "Failed to dump wasm to {output_wasm_path} due to error: {}",
            e
        );
    }
    validate_wasm(output_wasm_path);
}

pub(crate) fn validate_wasm(wasm_path: &str) -> bool {
    debug!("Running 'wasm-tools validate' on file: {wasm_path}");
    let res = Command::new("wasm-tools")
        .arg("validate")
        .arg(wasm_path)
        .output()
        .expect("failed to execute process");
    if !res.status.success() {
        println!("{:?}", std::str::from_utf8(&res.stderr).unwrap());
    }

    res.status.success()
}

// ====================================
// ==== Custom Sections API Testing ====
// ====================================

#[test]
fn test_custom_sections_get_section_data_mut() {
    // Test basic copy-on-write functionality
    let original_data = b"hello world";
    let mut sections = CustomSections::new(vec![("test", original_data)]);

    let id = sections.get_id("test".to_string()).unwrap();
    assert_eq!(id, CustomSectionID(0));

    // Get mutable reference - this should trigger copy-on-write
    let data_mut = sections.get_section_data_mut(id).unwrap();
    assert_eq!(data_mut, original_data);

    // Modify the data
    data_mut.push(b'!');
    assert_eq!(data_mut, b"hello world!");

    // Verify the change persisted
    let data_mut2 = sections.get_section_data_mut(id).unwrap();
    assert_eq!(data_mut2, b"hello world!");
}

#[test]
fn test_custom_sections_get_section_data_mut_invalid_id() {
    let mut sections = CustomSections::new(vec![]);
    let result = sections.get_section_data_mut(CustomSectionID(0));
    assert!(result.is_none());
}

#[test]
fn test_custom_sections_add_new_section() {
    let mut sections = CustomSections::new(vec![]);

    // Create a new section with owned data
    let section = CustomSection::new("new_section", vec![1, 2, 3, 4]);
    let id = sections.add(section);

    assert_eq!(id, CustomSectionID(0));
    assert_eq!(sections.len(), 1);

    // Get the data and modify it
    let data_mut = sections.get_section_data_mut(id).unwrap();
    assert_eq!(data_mut, &[1, 2, 3, 4]);

    data_mut.extend_from_slice(&[5, 6]);
    assert_eq!(data_mut, &[1, 2, 3, 4, 5, 6]);
}

#[test]
fn test_custom_sections_multiple_sections() {
    let mut sections = CustomSections::new(vec![("section1", b"data1"), ("section2", b"data2")]);

    let id1 = sections.get_id("section1".to_string()).unwrap();
    let id2 = sections.get_id("section2".to_string()).unwrap();

    assert_eq!(id1, CustomSectionID(0));
    assert_eq!(id2, CustomSectionID(1));

    // Modify first section
    let data1_mut = sections.get_section_data_mut(id1).unwrap();
    data1_mut.clear();
    data1_mut.extend_from_slice(b"modified1");

    // Modify second section
    let data2_mut = sections.get_section_data_mut(id2).unwrap();
    data2_mut.clear();
    data2_mut.extend_from_slice(b"modified2");

    // Verify both modifications
    assert_eq!(sections.get_section_data_mut(id1).unwrap(), b"modified1");
    assert_eq!(sections.get_section_data_mut(id2).unwrap(), b"modified2");
}

#[test]
fn test_custom_sections_cow_behavior() {
    let original_data = b"original";
    let mut sections = CustomSections::new(vec![("test", original_data)]);

    let id = sections.get_id("test".to_string()).unwrap();

    // First, verify the section starts as borrowed
    let section = sections.get_by_id(id);
    assert_eq!(section.data.as_ref(), original_data);

    // Now trigger copy-on-write
    let data_mut = sections.get_section_data_mut(id).unwrap();
    data_mut.push(b'!');

    // The data should now be owned
    let section = sections.get_by_id(id);
    assert_eq!(section.data.as_ref(), b"original!");
}

#[test]
fn test_custom_sections_add_and_modify_workflow() {
    let mut sections = CustomSections::new(vec![]);

    // Add a new section
    let section = CustomSection::new("config", b"key=value".to_vec());
    let id = sections.add(section);

    // Modify the section data
    {
        let data = sections.get_section_data_mut(id).unwrap();
        data.clear();
        data.extend_from_slice(b"key1=value1\nkey2=value2");
    }

    // Add another section
    let section2 = CustomSection::new("metadata", b"version=1.0".to_vec());
    let id2 = sections.add(section2);

    // Verify both sections exist and have correct data
    assert_eq!(sections.len(), 2);
    assert_eq!(
        sections.get_section_data_mut(id).unwrap(),
        b"key1=value1\nkey2=value2"
    );
    assert_eq!(sections.get_section_data_mut(id2).unwrap(), b"version=1.0");
}

#[test]
fn test_custom_sections_edge_cases() {
    // Test with empty data
    let mut sections = CustomSections::new(vec![("empty", b"")]);
    let id = sections.get_id("empty".to_string()).unwrap();

    let data_mut = sections.get_section_data_mut(id).unwrap();
    assert!(data_mut.is_empty());

    data_mut.extend_from_slice(b"now has content");
    assert_eq!(data_mut, b"now has content");

    // Test with large data
    let large_data = vec![42u8; 10000];
    let section = CustomSection::new("large", large_data.clone());
    let large_id = sections.add(section);

    let large_data_mut = sections.get_section_data_mut(large_id).unwrap();
    assert_eq!(large_data_mut.len(), 10000);
    assert!(large_data_mut.iter().all(|&b| b == 42));

    // Modify large data
    large_data_mut[0] = 1;
    large_data_mut[9999] = 2;
    assert_eq!(large_data_mut[0], 1);
    assert_eq!(large_data_mut[9999], 2);
}

#[test]
fn test_custom_sections_integration_with_existing_api() {
    let mut sections = CustomSections::new(vec![("original1", b"data1"), ("original2", b"data2")]);

    // Test existing API still works
    assert_eq!(sections.len(), 2);
    assert!(!sections.is_empty());

    let id1 = sections.get_id("original1".to_string()).unwrap();
    let section1 = sections.get_by_id(id1);
    assert_eq!(section1.name, "original1");
    assert_eq!(section1.data.as_ref(), b"data1");

    // Modify using new API
    let data_mut = sections.get_section_data_mut(id1).unwrap();
    data_mut.clear();
    data_mut.extend_from_slice(b"modified_data1");

    // Verify change via existing API
    let section1_after = sections.get_by_id(id1);
    assert_eq!(section1_after.data.as_ref(), b"modified_data1");

    // Test iteration
    let mut count = 0;
    for section in sections.iter() {
        count += 1;
        assert!(section.name.starts_with("original"));
    }
    assert_eq!(count, 2);

    // Test deletion still works
    // Note: after deletion, IDs may shift since we use Vec::remove
    let original_len = sections.len();
    sections.delete(id1);
    assert_eq!(sections.len(), original_len - 1);
}

#[test]
fn test_custom_section_constructors() {
    // Test new() constructor (owned data)
    let owned_data1 = b"data1".to_vec();
    let section1 = CustomSection::new("test1", owned_data1.clone());
    assert_eq!(section1.name, "test1");
    assert_eq!(section1.data.as_ref(), &owned_data1);

    // Test new() constructor with different owned data
    let owned_data2 = b"data2".to_vec();
    let section2 = CustomSection::new("test2", owned_data2.clone());
    assert_eq!(section2.name, "test2");
    assert_eq!(section2.data.as_ref(), &owned_data2);
}
