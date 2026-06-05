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

// Shared with integration tests. See tests/common/wast_iter.rs for the same
// pattern — keeps the "validate with all features" choice in one place.
#[path = "../../../tests/common/dwarf.rs"]
mod dwarf_helpers;
#[path = "../../../tests/common/validate.rs"]
mod validate;

// FUNCTIONS
#[test]
fn test_add_local_func() {
    let (buff, mut init_state) = setup();
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
    state_assertions(&module, &init_state, false);

    // convert the import to a function
    let mut builder = FunctionBuilder::new(&[DataType::I32], &[DataType::I32]);
    builder.i32_const(1);
    builder.drop();
    builder
        .replace_import_in_module(&mut module, ImportsID(0))
        .expect("error");

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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
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
    let new_module = Module::parse(&buff, false, false, false).expect("Unable to parse");

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
    debug!("Validating wasm at: {wasm_path}");
    let bytes = match std::fs::read(wasm_path) {
        Ok(b) => b,
        Err(e) => {
            println!("failed to read {wasm_path}: {e}");
            return false;
        }
    };
    match validate::validate_bytes(&bytes) {
        Ok(()) => true,
        Err(e) => {
            println!("{e}");
            false
        }
    }
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
    let section = sections.get_by_id(id).expect("Should be present");
    assert_eq!(section.data.as_ref(), original_data);

    // Now trigger copy-on-write
    let data_mut = sections.get_section_data_mut(id).unwrap();
    data_mut.push(b'!');

    // The data should now be owned
    let section = sections.get_by_id(id).expect("Should be present");
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
    let section1 = sections.get_by_id(id1).expect("Should be present");
    assert_eq!(section1.name, "original1");
    assert_eq!(section1.data.as_ref(), b"data1");

    // Modify using new API
    let data_mut = sections.get_section_data_mut(id1).unwrap();
    data_mut.clear();
    data_mut.extend_from_slice(b"modified_data1");

    // Verify change via existing API
    let section1_after = sections.get_by_id(id1).expect("Should be present");
    assert_eq!(section1_after.data.as_ref(), b"modified_data1");

    // Test iteration
    let mut count = 0;
    for section in sections.iter() {
        count += 1;
        assert!(section.name.starts_with("original"));
    }
    assert_eq!(count, 2);
}

#[test]
fn test_delete_custom_section_roundtrip() {
    let (buff, _) = setup();
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");

    let keep1 = module.add_custom_section(CustomSection::new("keep1", b"a".to_vec()));
    let del = module.add_custom_section(CustomSection::new("del", b"b".to_vec()));
    let keep2 = module.add_custom_section(CustomSection::new("keep2", b"c".to_vec()));

    module.delete_custom_section(del);

    assert_eq!(
        module.custom_sections.get_by_id(keep1).unwrap().name,
        "keep1"
    );
    assert_eq!(
        module.custom_sections.get_by_id(keep2).unwrap().name,
        "keep2"
    );

    let encoded = module.encode().expect("encode failed");
    let reparsed = Module::parse(&encoded, false, false, false).expect("reparse failed");
    let names: Vec<&str> = reparsed.custom_sections.iter().map(|s| s.name).collect();

    assert!(names.contains(&"keep1"));
    assert!(names.contains(&"keep2"));
    assert!(!names.contains(&"del"));
}

#[test]
fn test_delete_custom_section_invalid_id() {
    let (buff, _) = setup();
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");

    let before = module.custom_sections.len();
    module.delete_custom_section(CustomSectionID(9999));

    assert_eq!(module.custom_sections.len(), before);
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

// The third function declares two locals so this also acts as a regression
// guard against a parse↔encode PC convention drift: until the rewriter work
// aligned both sides on "PC measured from the first instruction", the parse
// side incidentally agreed with encode only when there were zero declared
// locals.
#[test]
fn with_dwarf_captures_per_op_pcs_matching_reparsed_offsets() {
    let wat = r#"(module
        (func (result i32) i32.const 1 i32.const 2 i32.add)
        (func nop nop)
        (func (local i32) (local i64)
            i32.const 0
            local.set 0
            i64.const 0
            local.set 1))"#;
    let wasm = wat::parse_str(wat).expect("wat compiles");
    let module = Module::parse(&wasm, false, false, true).expect("parse");

    let (encoded, _side_effects, maps) = module.encode_internal(false).expect("encode");
    let maps = maps.expect("with_dwarf=true should capture DWARF encode maps");
    assert!(!maps.per_func.is_empty(), "expected per-function maps");
    let out = encoded.finish();

    let reparsed = Module::parse(&out, false, true, false).expect("reparse output");
    for (func_idx, captured) in &maps.per_func {
        let local = reparsed
            .functions
            .unwrap_local(FunctionID(*func_idx))
            .expect("local function");
        assert_eq!(
            captured.pcs.len(),
            captured.anchors.len(),
            "func {func_idx}: pcs and anchors must be parallel arrays",
        );
        for (i, pc) in captured.pcs.iter().enumerate() {
            assert_eq!(
                Some(*pc),
                local.lookup_pc_offset_for(i),
                "func {func_idx} op {i}: captured PC disagrees with re-parsed offset",
            );
            // Uninstrumented: every emitted op is its own original, so anchor
            // is the identity.
            assert_eq!(
                captured.anchors[i], i,
                "func {func_idx} op {i}: uninstrumented anchor must be identity",
            );
        }
        // No op past the captured range — captured length matches the body.
        assert_eq!(
            local.lookup_pc_offset_for(captured.pcs.len()),
            None,
            "func {func_idx}: re-parsed output has more ops than captured",
        );
    }
}

// DWARF rewriting, step 10: property test for the source-location invariant.
//
// For every emitted op, `lookup(new_pc)` in the output's `.debug_line` must
// equal `lookup(anchor_orig_pc)` in the input's. That is, the debugger sees
// the same source location for an emitted op as it would for the orig op
// that emit anchors to (whether the emit is the orig op itself, an injected
// before/after, or an alt's first instruction). This is the strong
// correctness invariant for the rewriter.
//
// The proptest generates random before-injection plans (a Vec of nop counts)
// over `add.wasm` and asserts the invariant for every captured emit. Failed
// plans get shrunk automatically.

use dwarf_helpers::{debug_info_pcs, line_rows, lookup_src_at};

/// The strong source-location invariant the rewriter must preserve: for every
/// emitted op `i`, the output line program's source location at
/// `new_first + pcs[i]` equals the input line program's source location at
/// `orig_first + orig_pcs[anchors[i]]`. Encodes the module via
/// `encode_internal`, returns the encoded bytes so callers can run extra
/// sanity checks (e.g. range expansion) without re-encoding.
fn assert_source_location_invariant(input_bytes: &[u8], module: &Module) -> Vec<u8> {
    let (encoded, _side_effects, maps) = module.encode_internal(false).expect("encode");
    let maps = maps.expect("with_dwarf=true captures DWARF maps");
    let out_bytes = encoded.finish();

    let in_rows = line_rows(input_bytes);
    let out_rows = line_rows(&out_bytes);

    let debug = module.debug.as_ref().expect("orig debug data");
    for (func_idx, fmap) in &maps.per_func {
        let orig_dbg = debug
            .per_func
            .get(func_idx)
            .expect("orig per-func data for captured function");
        let local = module
            .functions
            .unwrap_local(FunctionID(*func_idx))
            .expect("local function for captured map");
        let orig_pcs = local
            .body
            .instructions
            .offsets()
            .expect("with_dwarf opt-in must populate offsets");
        // Line-program rows carry module-cumulative addresses, so the
        // function's "start of instructions" for lookup is the cumulative
        // base + the in-function header offset.
        let orig_first =
            (orig_dbg.dwarf_addr_base + orig_dbg.first_instr_dwarf_offset) as u64;
        let new_first = (fmap
            .dwarf_addr_base
            .expect("encode_internal must run the cumulative-base pass before returning maps")
            + fmap.first_instr_dwarf_offset) as u64;

        for (emit_idx, &new_pc) in fmap.pcs.iter().enumerate() {
            let anchor = fmap.anchors[emit_idx];
            let orig_pc = orig_pcs[anchor] as u64;
            let new_dwarf_addr = new_first + new_pc as u64;
            let orig_dwarf_addr = orig_first + orig_pc;
            let out_src = lookup_src_at(&out_rows, new_dwarf_addr);
            let in_src = lookup_src_at(&in_rows, orig_dwarf_addr);
            assert_eq!(
                out_src, in_src,
                "func {func_idx} emit {emit_idx} (anchor orig {anchor}): \
                 src mismatch — orig_addr={orig_dwarf_addr}, new_addr={new_dwarf_addr}",
            );
        }
    }

    out_bytes
}

fn dwarf_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/test_inputs/handwritten/dwarf")
        .join(name)
}

// `proptest::strategy::Strategy` provides the `.prop_map` adapter we use in
// the generator's `prop_oneof![]` arms. The macro expands its strategy
// expressions at module scope, so the trait import has to be here, not inside
// the test body.
use proptest::strategy::Strategy as _;

/// Per-op random instrumentation: one of these is generated for every visited
/// op. `Skip` leaves the op alone; the other variants inject N nops in the
/// corresponding mode. Alt with N=0 is treated as a no-op so we never produce
/// a "replace orig with nothing" plan.
#[derive(Debug, Clone)]
enum DwarfFuzzAction {
    Skip,
    BeforeNops(u8),
    AfterNops(u8),
    AltNops(u8),
}

/// Walks the module applying one `DwarfFuzzAction` per op in plan order.
/// Multi-function inputs see plan entries consumed sequentially across all
/// functions in code-section order.
fn apply_dwarf_fuzz_plan(module: &mut Module, plan: &[DwarfFuzzAction]) {
    use crate::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
    use crate::iterator::module_iterator::ModuleIterator;
    use crate::Opcode;

    let mut it = ModuleIterator::new(module, &Vec::new());
    let mut op_idx = 0usize;
    loop {
        if it.curr_op().is_some() {
            let action = plan.get(op_idx).cloned().unwrap_or(DwarfFuzzAction::Skip);
            match action {
                DwarfFuzzAction::Skip => {}
                DwarfFuzzAction::BeforeNops(n) => {
                    for _ in 0..n {
                        it.before().nop();
                    }
                }
                DwarfFuzzAction::AfterNops(n) => {
                    for _ in 0..n {
                        it.after().nop();
                    }
                }
                DwarfFuzzAction::AltNops(n) => {
                    for _ in 0..n {
                        it.alternate().nop();
                    }
                }
            }
            op_idx += 1;
        }
        if it.next().is_none() {
            break;
        }
    }
}

proptest::proptest! {
    /// For every emitted op, the rewritten `.debug_line`'s source location at
    /// the new PC must equal the input's source location at the anchor's orig
    /// PC. Generates per-op random instrumentation plans (mix of
    /// before/after/alt at random counts) over `add.wasm` (one local function).
    #[test]
    fn rewriter_preserves_source_location_under_random_injection(
        plan in proptest::collection::vec(
            proptest::prop_oneof![
                proptest::strategy::Just(DwarfFuzzAction::Skip),
                (0u8..=3).prop_map(DwarfFuzzAction::BeforeNops),
                (0u8..=3).prop_map(DwarfFuzzAction::AfterNops),
                (1u8..=3).prop_map(DwarfFuzzAction::AltNops),
            ],
            0..=8usize,
        )
    ) {
        let input = std::fs::read(dwarf_fixture_path("add.wasm")).expect("read fixture");
        let mut module = Module::parse(&input, false, false, true).expect("parse");
        apply_dwarf_fuzz_plan(&mut module, &plan);
        let _ = assert_source_location_invariant(&input, &module);
    }

    /// Multi-function variant: same invariant, on `two_funcs.wasm` (two local
    /// functions sharing a module-cumulative DWARF address space). Catches
    /// per-function routing regressions where injection in one function
    /// scrambles the other's row addresses.
    #[test]
    fn rewriter_preserves_source_location_under_random_injection_multi_func(
        plan in proptest::collection::vec(
            proptest::prop_oneof![
                proptest::strategy::Just(DwarfFuzzAction::Skip),
                (0u8..=3).prop_map(DwarfFuzzAction::BeforeNops),
                (0u8..=3).prop_map(DwarfFuzzAction::AfterNops),
                (1u8..=3).prop_map(DwarfFuzzAction::AltNops),
            ],
            // Cap at 2 funcs × ~5 ops/func = 10 plan slots.
            0..=12usize,
        )
    ) {
        let input = std::fs::read(dwarf_fixture_path("two_funcs.wasm")).expect("read fixture");
        let mut module = Module::parse(&input, false, false, true).expect("parse");
        apply_dwarf_fuzz_plan(&mut module, &plan);
        let _ = assert_source_location_invariant(&input, &module);
    }
}

// Step 8 regression: nop injected before every op. Asserts the strong source-
// location invariant for every emit position, then sanity-checks that
// instrumentation actually took effect (DIE ranges expanded, max addr shifted).
// `lookup(new_pc) == lookup(anchor_orig_pc)` would trivially hold for an
// uninstrumented round-trip, so the post-helper checks confirm the test isn't
// silently a no-op.
#[test]
fn rewriter_anchors_nop_before_every_op_to_host_source_strong() {
    use crate::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
    use crate::iterator::module_iterator::ModuleIterator;
    use crate::Opcode;

    let input = std::fs::read(dwarf_fixture_path("add.wasm")).expect("read fixture");
    let mut module = Module::parse(&input, false, false, true).expect("parse");
    {
        let mut it = ModuleIterator::new(&mut module, &Vec::new());
        loop {
            if it.curr_op().is_some() {
                it.before().nop();
            }
            if it.next().is_none() {
                break;
            }
        }
    }
    let out = assert_source_location_invariant(&input, &module);

    // Sanity: every DIE range must have expanded to cover the injected bytes.
    let in_pcs = debug_info_pcs(&input);
    let out_pcs = debug_info_pcs(&out);
    assert_eq!(in_pcs.len(), out_pcs.len(), "DIE count must be preserved");
    for ((il, ih), (ol, oh)) in in_pcs.iter().zip(out_pcs.iter()) {
        assert!(
            *oh - *ol > *ih - *il,
            "DIE range must expand for injected bytes ({il}..{ih} → {ol}..{oh})",
        );
    }
}

// Step 8 regression: func_exit injection. Drives the special-mode resolution
// path that fans `func_exit` out to every exit slot (here a single end op).
// Strong source-location invariant + DIE range expansion confirm both the
// fan-out wiring and the address translation behave.
#[test]
fn rewriter_handles_func_exit_injection_strong() {
    use crate::opcode::Instrumenter;
    use crate::Opcode;

    let input = std::fs::read(dwarf_fixture_path("add.wasm")).expect("read fixture");
    let mut module = Module::parse(&input, false, false, true).expect("parse");
    {
        let mut it =
            crate::iterator::module_iterator::ModuleIterator::new(&mut module, &Vec::new());
        it.func_exit().nop().nop();
    }
    let out = assert_source_location_invariant(&input, &module);

    let in_pcs = debug_info_pcs(&input);
    let out_pcs = debug_info_pcs(&out);
    assert_eq!(in_pcs.len(), out_pcs.len());
    for ((il, ih), (ol, oh)) in in_pcs.iter().zip(out_pcs.iter()) {
        assert!(
            *oh - *ol > *ih - *il,
            "DIE range must expand for injected exit bytes ({il}..{ih} → {ol}..{oh})",
        );
    }
}

// Real rustc-emitted DWARF v4 fixture (`from-rust/from-rust.wasm`): multi
// function, inlined-subroutine DIEs, rangelist CU, `dead code` low_pc
// tombstone on the unused `panic` subprogram. Checks the strong invariant
// `output_lookup(new_pc) == input_lookup(anchor_orig_pc)` at every emit
// position — same contract the handwritten-fixture tests enforce.
#[test]
fn rewriter_handles_from_rust_fixture_uninstrumented_strong() {
    let input = std::fs::read(dwarf_fixture_path("from-rust/from-rust.wasm"))
        .expect("read from-rust fixture");
    let module = Module::parse(&input, false, false, true).expect("parse");
    let _ = assert_source_location_invariant(&input, &module);
}

#[test]
fn rewriter_handles_from_rust_fixture_instrumented_strong() {
    use crate::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
    use crate::iterator::module_iterator::ModuleIterator;
    use crate::Opcode;

    let input = std::fs::read(dwarf_fixture_path("from-rust/from-rust.wasm"))
        .expect("read from-rust fixture");
    let mut module = Module::parse(&input, false, false, true).expect("parse");
    {
        let mut it = ModuleIterator::new(&mut module, &Vec::new());
        loop {
            if it.curr_op().is_some() {
                it.before().nop();
            }
            if it.next().is_none() {
                break;
            }
        }
    }
    let _ = assert_source_location_invariant(&input, &module);
}

// Step 14: when `with_dwarf` is on and the module also carries an adjacent
// debug section (`external_debug_info` or `sourceMappingURL`), parsing must
// emit a `log::warn!` so the user knows their adjacent debug-info goes stale
// after instrumentation — wirm passes those bytes through unchanged.

/// One-shot global logger that captures every log record into a shared
/// `Vec<String>`. The first test to install it wins; subsequent tests reuse
/// the same buffer and snapshot-diff to isolate their own records.
mod warn_capture {
    use std::sync::{Mutex, OnceLock};

    static BUFFER: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    static INSTALLED: OnceLock<()> = OnceLock::new();
    static LOGGER: CaptureLogger = CaptureLogger;

    struct CaptureLogger;
    impl log::Log for CaptureLogger {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }
        fn log(&self, record: &log::Record) {
            if let Some(buf) = BUFFER.get() {
                if let Ok(mut v) = buf.lock() {
                    v.push(format!("{}: {}", record.level(), record.args()));
                }
            }
        }
        fn flush(&self) {}
    }

    pub fn install() -> &'static Mutex<Vec<String>> {
        let buf = BUFFER.get_or_init(|| Mutex::new(Vec::new()));
        INSTALLED.get_or_init(|| {
            // Best-effort: if some other test in the same process already set
            // a logger we silently fall back to whatever it's doing (the
            // buffer just stays empty for our tests).
            let _ = log::set_logger(&LOGGER);
            log::set_max_level(log::LevelFilter::Warn);
        });
        buf
    }

    /// Snapshot the current buffer length so a test can read just the
    /// records it produced.
    pub fn snapshot(buf: &Mutex<Vec<String>>) -> usize {
        buf.lock().map(|v| v.len()).unwrap_or(0)
    }

    pub fn records_since(buf: &Mutex<Vec<String>>, start: usize) -> Vec<String> {
        buf.lock().map(|v| v[start..].to_vec()).unwrap_or_default()
    }
}

fn module_with_custom_section(name: &str, data: &[u8]) -> Vec<u8> {
    let mut m = wasm_encoder::Module::new();
    m.section(&wasm_encoder::CustomSection {
        name: std::borrow::Cow::Borrowed(name),
        data: std::borrow::Cow::Borrowed(data),
    });
    m.finish()
}

#[test]
fn with_dwarf_warns_on_external_debug_info_section() {
    let buf = warn_capture::install();
    let start = warn_capture::snapshot(buf);

    let bytes = module_with_custom_section("external_debug_info", b"https://example/dwarf");
    let module = Module::parse(&bytes, false, false, true).expect("parse");
    // The section passes through opaquely — it's NOT diverted into
    // Module::debug because it isn't a `.debug_*` section.
    assert!(
        module
            .debug
            .as_ref()
            .is_some_and(|d| d.sections().is_empty()),
        "Module::debug should be Some(empty); adjacent debug sections \
         do not divert into the DWARF rewriter",
    );

    let records = warn_capture::records_since(buf, start);
    assert!(
        records
            .iter()
            .any(|r| r.contains("external_debug_info") && r.contains("does not rewrite")),
        "expected an external_debug_info warning, got: {records:?}",
    );
}

#[test]
fn with_dwarf_warns_on_source_mapping_url_section() {
    let buf = warn_capture::install();
    let start = warn_capture::snapshot(buf);

    let bytes = module_with_custom_section("sourceMappingURL", b"./map.json");
    let _module = Module::parse(&bytes, false, false, true).expect("parse");

    let records = warn_capture::records_since(buf, start);
    assert!(
        records
            .iter()
            .any(|r| r.contains("sourceMappingURL") && r.contains("does not rewrite")),
        "expected a sourceMappingURL warning, got: {records:?}",
    );
}

#[test]
fn without_dwarf_does_not_warn_on_adjacent_debug_sections() {
    let buf = warn_capture::install();
    let start = warn_capture::snapshot(buf);

    let bytes = module_with_custom_section("external_debug_info", b"https://example/dwarf");
    let _module = Module::parse(&bytes, false, false, false).expect("parse");

    let records = warn_capture::records_since(buf, start);
    assert!(
        !records.iter().any(|r| r.contains("external_debug_info")),
        "no warning expected when with_dwarf=false, got: {records:?}",
    );
}

// DWARF rewriting, step 9: differential test for an INSTRUMENTED module.
// Two paths produce per-emit byte offsets — the in-encode capture (each emit
// op's `function.byte_len()` rebased onto the first instruction) and a
// re-parse of the encoded output with `with_offsets=true`. They must agree
// byte-for-byte, including across injected ops, because parsing the output
// observes every byte the encoder emitted.
//
// Injecting a nop before every visited op exercises the cumulative shift the
// step-0 spike findings flagged as the primary source of off-by-one bugs.
#[test]
fn with_dwarf_capture_matches_reparsed_offsets_with_injection() {
    use crate::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
    use crate::iterator::module_iterator::ModuleIterator;
    use crate::Opcode;

    let wat = r#"(module
        (func (result i32) i32.const 1 i32.const 2 i32.add)
        (func (local i32) (local i64)
            i32.const 0
            local.set 0
            i64.const 0
            local.set 1))"#;
    let wasm = wat::parse_str(wat).expect("wat compiles");
    let mut module = Module::parse(&wasm, false, false, true).expect("parse");
    {
        // Inject a nop before every visited op across every local function.
        let mut it = ModuleIterator::new(&mut module, &Vec::new());
        loop {
            if it.curr_op().is_some() {
                it.before().nop();
            }
            if it.next().is_none() {
                break;
            }
        }
    }

    let (encoded, _side_effects, maps) = module.encode_internal(false).expect("encode");
    let maps = maps.expect("with_dwarf=true should capture DWARF encode maps");
    let out = encoded.finish();
    let reparsed = Module::parse(&out, false, true, false).expect("reparse output");

    assert!(
        !maps.per_func.is_empty(),
        "expected per-function maps for the instrumented test wat",
    );
    for (func_idx, captured) in &maps.per_func {
        let local = reparsed
            .functions
            .unwrap_local(FunctionID(*func_idx))
            .expect("local function in re-parsed output");
        for (i, pc) in captured.pcs.iter().enumerate() {
            assert_eq!(
                Some(*pc),
                local.lookup_pc_offset_for(i),
                "func {func_idx} emit {i}: in-encode capture vs re-parse mismatch",
            );
        }
        // Both paths must cover the same number of ops (no missing or extra
        // emits on either side). `lookup_pc_offset_for(captured.len())` past
        // the end returns `None` in re-parsed output.
        assert_eq!(
            local.lookup_pc_offset_for(captured.pcs.len()),
            None,
            "func {func_idx}: re-parsed output has more ops than captured",
        );
    }
}

// DWARF rewriting, step 3: encode records the byte offset where the code
// section begins in the output, so the rewriter can translate per-function
// PCs into module-absolute addresses. The captured offset must point at the
// code-section ID byte (0x0A) and lie past the wasm preamble.
#[test]
fn with_dwarf_captures_code_section_start_offset() {
    let wasm = wat::parse_str("(module (func nop))").expect("wat compiles");
    let module = Module::parse(&wasm, false, false, true).expect("parse");

    let (encoded, _side_effects, maps) = module.encode_internal(false).expect("encode");
    let maps = maps.expect("with_dwarf=true should capture DWARF encode maps");
    let css = maps
        .code_section_start
        .expect("code section is emitted for a module with a local function");

    let out = encoded.finish();
    // Wasm magic+version is 8 bytes; the code section sits after at least the
    // type and function sections that wirm always emits, so this must be past
    // the preamble. The captured offset must point at the code-section ID.
    assert!(
        css > 8,
        "code_section_start should be past the 8-byte preamble"
    );
    assert_eq!(
        out[css], 0x0a,
        "code_section_start must point at the code-section ID byte (0x0A)",
    );
}

// A module with no local functions emits no code section, so the rewriter
// has no code-section anchor to record even when DWARF rewriting is on.
#[test]
fn with_dwarf_no_code_section_leaves_code_section_start_none() {
    let wasm = wat::parse_str("(module)").expect("wat compiles");
    let module = Module::parse(&wasm, false, false, true).expect("parse");
    let (_encoded, _side_effects, maps) = module.encode_internal(false).expect("encode");
    let maps = maps.expect("with_dwarf=true should capture DWARF encode maps");
    assert!(
        maps.code_section_start.is_none(),
        "no code section emitted => no captured start offset",
    );
    assert!(maps.per_func.is_empty());
}

// `with_dwarf = false` must leave capture off entirely: no DWARF encode maps
// are produced, so encode pays nothing for modules that didn't opt in.
#[test]
fn without_dwarf_captures_no_per_op_pcs() {
    let wasm = wat::parse_str("(module (func nop))").expect("wat compiles");
    let module = Module::parse(&wasm, false, false, false).expect("parse");
    let (_encoded, _side_effects, maps) = module.encode_internal(false).expect("encode");
    assert!(
        maps.is_none(),
        "with_dwarf=false should not capture DWARF maps"
    );
}
