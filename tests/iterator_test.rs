use log::{debug, trace};
use std::collections::{HashMap, HashSet};
use wasmparser::Operator;
use wirm::ir::component::Component;
use wirm::ir::id::{FunctionID, ModuleID};
use wirm::ir::module::Module;
use wirm::ir::types::Location;
use wirm::iterator::component_iterator::ComponentIterator;
use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
use wirm::iterator::module_iterator::ModuleIterator;
use wirm::module_builder::AddLocal;
use wirm::opcode::Instrumenter;

#[test]
fn test_iterator_count() {
    let file = "tests/test_inputs/handwritten/components/add.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut component = Component::parse(&buff, false, false).expect("Unable to parse");
    let mut comp_it = ComponentIterator::new(&mut component, HashMap::new());
    iterate_component_and_count(&mut comp_it, 10);
}

#[test]
fn test_iterator_count_mul_mod() {
    let file = "tests/test_inputs/handwritten/components/mul_mod.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut component = Component::parse(&buff, false, false).expect("Unable to parse");
    let mut comp_it = ComponentIterator::new(&mut component, HashMap::new());
    iterate_component_and_count(&mut comp_it, 15);
}

#[test]
fn test_mod_iterator_count() {
    let file = "tests/test_inputs/handwritten/modules/add.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);
    iterate_module_and_count(&mut mod_it, 1, 9);
}

// example of a ModuleIterator
#[test]
fn test_blocks() {
    let file = "tests/test_inputs/handwritten/modules/block.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    assert_eq!(module.num_import_func(), 0);

    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);
    iterate_module_and_count(&mut mod_it, 0, 10);
}

// example of splicing an instrument at specific location
// TODO: no assertions for now, verify by eyeballing
#[test]
fn test_it_instr_at() {
    let file = "tests/test_inputs/handwritten/modules/add.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let loc = Location::Module {
        func_idx: FunctionID(1),
        instr_idx: 1,
    };
    mod_it.before_at(loc);
    mod_it.add_instr_at(loc, Operator::Unreachable);
    loop {
        let op = mod_it.curr_op();
        if let Location::Module {
            func_idx,
            instr_idx,
        } = mod_it.curr_loc().0
        {
            trace!("Func: {:?}, {}: {:?},", func_idx, instr_idx, op);
        } else {
            panic!("Should've gotten Module Location!");
        }
        if mod_it.next().is_none() {
            break;
        };
    }

    let a = module.encode();
    let wat = wasmprinter::print_bytes(&a).unwrap();
    debug!("{}", wat);
}

// example of duplicating every instruction
// TODO: no assertions for now, verify by eyeballing
#[test]
fn test_it_dup_instr() {
    let file = "tests/test_inputs/handwritten/modules/add.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    loop {
        let op = mod_it.curr_op();
        if let Location::Module {
            func_idx,
            instr_idx,
        } = mod_it.curr_loc().0
        {
            trace!("Func: {:?}, {}: {:?},", func_idx, instr_idx, op);

            let loc = mod_it.curr_loc().0;
            let orig = mod_it.curr_op_owned().unwrap();
            if !matches!(orig, Operator::End) {
                mod_it.before();
                mod_it.add_instr_at(loc, orig);
            }
        } else {
            panic!("Should've gotten Component Location!");
        }
        if mod_it.next().is_none() {
            break;
        };
    }

    let a = module.encode();
    let wat = wasmprinter::print_bytes(&a).unwrap();
    debug!("{}", wat);
}

#[test]
fn test_it_add_local_diff_type() {
    let file = "tests/test_inputs/handwritten/modules/add.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    mod_it.add_local(wirm::ir::types::DataType::I64);
    mod_it.add_local(wirm::ir::types::DataType::I32);
    let a = module.encode();
    let wat = wasmprinter::print_bytes(&a).unwrap();
    debug!("{}", wat);
}

#[test]
fn test_imports() {
    let file = "tests/test_inputs/handwritten/modules/import.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    assert_eq!(module.num_import_func(), 2);

    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);
    iterate_module_and_count(&mut mod_it, 2, 2);

    let a = module.encode();
    let wat = wasmprinter::print_bytes(&a).unwrap();
    debug!("{}", wat);
}

#[test]
fn test_function_skipping_module() {
    let file = "tests/test_inputs/handwritten/modules/add.wat";

    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    let functions_skip = vec![FunctionID(1)];
    let mut mod_it = ModuleIterator::new(&mut module, &functions_skip);

    let mut set = HashSet::new();

    loop {
        if let Location::Module {
            func_idx,
            instr_idx: _instr_idx,
        } = mod_it.curr_loc().0
        {
            set.insert(func_idx);
        } else {
            panic!("Should've gotten Module Location!");
        }
        if mod_it.next().is_none() {
            break;
        };
    }

    assert_eq!(set.len(), 1);
    assert!(set.contains(&FunctionID(2)));
}

#[test]
fn test_function_skipping_component() {
    let file = "tests/test_inputs/handwritten/components/add.wat";

    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut comp = Component::parse(&buff, false, false).expect("Unable to parse");
    let functions_skip = vec![FunctionID(0)];
    let mut mapping = HashMap::new();
    mapping.insert(ModuleID(0), functions_skip);
    let mut comp_it = ComponentIterator::new(&mut comp, mapping);

    let mut set = HashSet::new();

    loop {
        if let Location::Component { func_idx, .. } = comp_it.curr_loc().0 {
            set.insert(func_idx);
        } else {
            panic!("Should've gotten Component Location!");
        }
        if comp_it.next().is_none() {
            break;
        };
    }

    assert_eq!(set.len(), 1);
    assert!(set.contains(&FunctionID(1)));
}

#[test]
fn test_fn_name() {
    let file = "tests/test_inputs/handwritten/modules/add.wat";

    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let module = Module::parse(&buff, false, false).expect("Unable to parse");
    assert_eq!(
        "add".to_string(),
        *module.functions.get_name(FunctionID(1)).as_ref().unwrap()
    );
    assert_eq!(None, *module.functions.get_name(FunctionID(2)));
}

// =================
// ==== HELPERS ====
// =================

fn iterate_component_and_count(comp_it: &mut ComponentIterator, exp_count: u32) {
    let mut count = 0;
    loop {
        let op = comp_it.curr_op();
        if let Location::Component {
            mod_idx,
            func_idx,
            instr_idx,
        } = comp_it.curr_loc().0
        {
            trace!(
                "Mod: {:?}, Func: {:?}, {}: {:?},",
                mod_idx,
                func_idx,
                instr_idx,
                op
            );
            count += 1;
            if comp_it.next().is_none() {
                break;
            };
        } else {
            panic!("Should've gotten Component Location!");
        }
    }

    // TODO Check this iterator test (does it assert that it visits every func?)
    // (add an import)
    assert_eq!(count, exp_count);
}

fn iterate_module_and_count(mod_it: &mut ModuleIterator, num_imports: u32, exp_count: u32) {
    let mut count = 0;
    loop {
        let op = mod_it.curr_op();
        if let Location::Module {
            func_idx,
            instr_idx,
        } = mod_it.curr_loc().0
        {
            trace!(
                "Func: {}, {}: {:?},",
                num_imports + *func_idx,
                instr_idx,
                op
            );
        } else {
            panic!("Should've gotten Module Location!");
        }
        count += 1;
        if mod_it.next().is_none() {
            break;
        };
    }
    assert_eq!(count, exp_count);
}
