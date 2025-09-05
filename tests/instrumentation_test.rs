use std::collections::HashMap;
use std::mem::discriminant;

use log::{error, trace};
use wasmparser::Operator;

use wirm::ir::id::{FunctionID, TypeID};
use wirm::ir::types::InstrumentationMode;
use wirm::iterator::component_iterator::ComponentIterator;
use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
use wirm::iterator::module_iterator::ModuleIterator;
use wirm::module_builder::AddLocal;
use wirm::opcode::{Inject, Instrumenter};
use wirm::{Component, Location, Module, Opcode};

mod common;
use crate::common::check_instrumentation_encoding;

#[test]
fn no_injection() {
    let file = "tests/test_inputs/handwritten/components/add.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut component = Component::parse(&buff, false).expect("Unable to parse");
    let mut comp_it = ComponentIterator::new(&mut component, HashMap::new());

    let interested = Operator::Call { function_index: 1 };

    loop {
        let op = comp_it.curr_op();
        let instr_mode = comp_it.curr_instrument_mode();

        if let Location::Component {
            mod_idx,
            func_idx,
            instr_idx,
        } = comp_it.curr_loc().0
        {
            trace!(
                "Mod: {:?}, Func: {:?}, +{}: {:?}, {:?}",
                mod_idx,
                func_idx,
                instr_idx,
                op,
                instr_mode
            );
            if *comp_it.curr_op().unwrap() == interested {
                comp_it.before();
            }
            if comp_it.next().is_none() {
                break;
            };
        } else {
            panic!("Should've gotten Component Location!");
        }
    }

    comp_it.reset();

    loop {
        let op = comp_it.curr_op();
        let instr_mode = comp_it.curr_instrument_mode();
        if let Location::Component {
            mod_idx,
            func_idx,
            instr_idx,
        } = comp_it.curr_loc().0
        {
            if *comp_it.curr_op().unwrap() == interested {
                assert_ne!(discriminant(&instr_mode), discriminant(&None));
            }

            trace!(
                "Mod: {:?}, Func: {:?}, +{}: {:?}, {:?}",
                mod_idx,
                func_idx,
                instr_idx,
                op,
                instr_mode
            );

            if comp_it.next().is_none() {
                break;
            };
        } else {
            panic!("Should've gotten Component Location!");
        }
    }
}

#[test]
fn iterator_inject_i32_before() {
    let file = "tests/test_inputs/instr_testing/components/add-inject_i32_before.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut component = Component::parse(&buff, false).expect("Unable to parse");
    let mut comp_it = ComponentIterator::new(&mut component, HashMap::new());

    let interested = Operator::Call { function_index: 1 };

    loop {
        let op = comp_it.curr_op();
        let instr_mode = comp_it.curr_instrument_mode();

        if let Location::Component {
            mod_idx,
            func_idx,
            instr_idx,
        } = comp_it.curr_loc().0
        {
            trace!(
                "Mod: {:?}, Func: {:?}, +{}: {:?}, {:?}",
                mod_idx,
                func_idx,
                instr_idx,
                op,
                instr_mode
            );
            if *comp_it.curr_op().unwrap() == interested {
                comp_it.before().i32_const(1);
            }
            if comp_it.next().is_none() {
                break;
            };
        } else {
            panic!("Should've gotten Component Location!");
        }
    }
    comp_it.reset();

    let result = component.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn iterator_inject_all_variations() {
    let file = "tests/test_inputs/instr_testing/components/add-inject_all_variations.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut component = Component::parse(&buff, false).expect("Unable to parse");
    let mut comp_it = ComponentIterator::new(&mut component, HashMap::new());

    let after = Operator::Call { function_index: 1 };
    let before = Operator::Drop;
    let alternate = Operator::I32Const { value: 2 };

    loop {
        let op = comp_it.curr_op();
        let instr_mode = comp_it.curr_instrument_mode();

        if let Location::Component {
            mod_idx,
            func_idx,
            instr_idx,
        } = comp_it.curr_loc().0
        {
            trace!(
                "Mod: {:?}, Func: {:?}, +{}: {:?}, {:?}",
                mod_idx,
                func_idx,
                instr_idx,
                op,
                instr_mode
            );
            if *comp_it.curr_op().unwrap() == before {
                comp_it.before().call(FunctionID(0));
            }

            if *comp_it.curr_op().unwrap() == after {
                comp_it.after().i32_const(0);
            }

            if *comp_it.curr_op().unwrap() == alternate {
                comp_it.alternate().i32_const(3);
            }

            if comp_it.next().is_none() {
                break;
            };
        } else {
            panic!("Should've gotten Component Location!");
        }
    }
    let result = component.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_inject_locals() {
    let file = "tests/test_inputs/instr_testing/modules/add-inject_locals.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let mut is_first = true;
    while is_first || mod_it.next().is_some() {
        let op = mod_it.curr_op();
        if let Location::Module {
            func_idx,
            instr_idx,
        } = mod_it.curr_loc().0
        {
            trace!("Func: {:?}, {}: {:?},", func_idx, instr_idx, op);

            if mod_it.curr_op().unwrap() == &Operator::I32Add {
                let local_id = mod_it.add_local(wirm::ir::types::DataType::I32);
                trace!("new Local ID: {:?}", local_id);
            }

            if mod_it.curr_op().unwrap() == &(Operator::I32Const { value: 2 }) {
                let local_id = mod_it.add_local(wirm::ir::types::DataType::I32);
                println!("new Local ID: {:?}", local_id);
            }
        } else {
            panic!("Should've gotten Module Location!");
        }

        is_first = false;
    }

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

// ==== BLOCK ALT ====

#[test]
fn test_block_alt_one_func_nested_block() {
    let file = "tests/test_inputs/instr_testing/modules/block_alt/one_func_nested_block.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let loop_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];

    let ops_of_interest = vec![(
        SupportedOperators::Loop,
        (InstrumentationMode::BlockAlt, loop_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_alt_one_func_remove_else() {
    let file = "tests/test_inputs/instr_testing/modules/block_alt/one_func_remove_else.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let else_body = vec![];

    let ops_of_interest = vec![(
        SupportedOperators::Else,
        (InstrumentationMode::BlockAlt, else_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_alt_one_func_replace_else() {
    let file = "tests/test_inputs/instr_testing/modules/block_alt/one_func_replace_else.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let else_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];

    let ops_of_interest = vec![(
        SupportedOperators::Else,
        (InstrumentationMode::BlockAlt, else_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_alt_one_func_two_blocks() {
    let file = "tests/test_inputs/instr_testing/modules/block_alt/one_func_two_blocks.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];

    let ops_of_interest = vec![(
        SupportedOperators::Block,
        (InstrumentationMode::BlockAlt, block_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_alt_remove_else_nested_if() {
    let file = "tests/test_inputs/instr_testing/modules/block_alt/remove_else_nested_if.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let else_body = vec![];

    let ops_of_interest = vec![(
        SupportedOperators::Else,
        (InstrumentationMode::BlockAlt, else_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_alt_remove_else_with_instrumented_after_if() {
    let file =
        "tests/test_inputs/instr_testing/modules/block_alt/remove_else_with_instr'd_after_if.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let if_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];

    let else_body = vec![];

    let ops_of_interest = vec![
        (
            SupportedOperators::If,
            (InstrumentationMode::SemanticAfter, if_body),
        ),
        (
            SupportedOperators::Else,
            (InstrumentationMode::BlockAlt, else_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_alt_remove_else_with_instrumented_exit_if() {
    let file =
        "tests/test_inputs/instr_testing/modules/block_alt/remove_else_with_instr'd_exit_if.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let if_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];

    let else_body = vec![];

    let ops_of_interest = vec![
        (
            SupportedOperators::If,
            (InstrumentationMode::BlockExit, if_body),
        ),
        (
            SupportedOperators::Else,
            (InstrumentationMode::BlockAlt, else_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_alt_remove_entire_block() {
    let file = "tests/test_inputs/instr_testing/modules/block_alt/remove_entire_block.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![];

    let ops_of_interest = vec![(
        SupportedOperators::Block,
        (InstrumentationMode::BlockAlt, block_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_alt_remove_if_with_else() {
    let file = "tests/test_inputs/instr_testing/modules/block_alt/remove_if_with_else.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let if_body = vec![];

    let ops_of_interest = vec![(
        SupportedOperators::If,
        (InstrumentationMode::BlockAlt, if_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_alt_remove_nested_block() {
    let file = "tests/test_inputs/instr_testing/modules/block_alt/remove_nested_block.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![];

    let ops_of_interest = vec![(
        SupportedOperators::Block,
        (InstrumentationMode::BlockAlt, block_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_alt_replace_else_nested_if() {
    let file = "tests/test_inputs/instr_testing/modules/block_alt/replace_else_nested_if.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let else_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];

    let ops_of_interest = vec![(
        SupportedOperators::Else,
        (InstrumentationMode::BlockAlt, else_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_alt_replace_if_with_else() {
    let file = "tests/test_inputs/instr_testing/modules/block_alt/replace_if_with_else.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let if_body = vec![
        Operator::Drop,
        Operator::I32Const { value: 12 },
        Operator::Drop,
    ];

    let ops_of_interest = vec![(
        SupportedOperators::If,
        (InstrumentationMode::BlockAlt, if_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_alt_replace_nested_block() {
    let file = "tests/test_inputs/instr_testing/modules/block_alt/replace_nested_block.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];

    let ops_of_interest = vec![(
        SupportedOperators::Block,
        (InstrumentationMode::BlockAlt, block_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

// ==== BLOCK ENTRY ====

#[test]
fn test_block_entry_one_func_nested_block() {
    let file = "tests/test_inputs/instr_testing/modules/block_entry/one_func_nested_block.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];

    let loop_body = vec![Operator::I32Const { value: 34 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::BlockEntry, block_body),
        ),
        (
            SupportedOperators::Loop,
            (InstrumentationMode::BlockEntry, loop_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_entry_one_func_one_block() {
    let file = "tests/test_inputs/instr_testing/modules/block_entry/one_func_one_block.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];

    let loop_body = vec![Operator::I32Const { value: 34 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::BlockEntry, block_body),
        ),
        (
            SupportedOperators::Loop,
            (InstrumentationMode::BlockEntry, loop_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_entry_one_func_two_blocks() {
    let file = "tests/test_inputs/instr_testing/modules/block_entry/one_func_two_blocks.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let loop_body = vec![Operator::I32Const { value: 34 }, Operator::Drop];
    let if_body = vec![Operator::I32Const { value: 56 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::BlockEntry, block_body),
        ),
        (
            SupportedOperators::Loop,
            (InstrumentationMode::BlockEntry, loop_body),
        ),
        (
            SupportedOperators::If,
            (InstrumentationMode::BlockEntry, if_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_entry_two_funcs_nested_block() {
    let file = "tests/test_inputs/instr_testing/modules/block_entry/two_funcs_nested_block.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let loop_body = vec![Operator::I32Const { value: 34 }, Operator::Drop];
    let if_body = vec![Operator::I32Const { value: 56 }, Operator::Drop];
    let else_body = vec![Operator::I32Const { value: 78 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::BlockEntry, block_body),
        ),
        (
            SupportedOperators::Loop,
            (InstrumentationMode::BlockEntry, loop_body),
        ),
        (
            SupportedOperators::If,
            (InstrumentationMode::BlockEntry, if_body),
        ),
        (
            SupportedOperators::Else,
            (InstrumentationMode::BlockEntry, else_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_entry_two_funcs_one_block() {
    let file = "tests/test_inputs/instr_testing/modules/block_entry/two_funcs_one_block.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let loop_body = vec![Operator::I32Const { value: 34 }, Operator::Drop];
    let if_body = vec![Operator::I32Const { value: 56 }, Operator::Drop];
    let else_body = vec![Operator::I32Const { value: 78 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::BlockEntry, block_body),
        ),
        (
            SupportedOperators::Loop,
            (InstrumentationMode::BlockEntry, loop_body),
        ),
        (
            SupportedOperators::If,
            (InstrumentationMode::BlockEntry, if_body),
        ),
        (
            SupportedOperators::Else,
            (InstrumentationMode::BlockEntry, else_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_entry_two_funcs_two_blocks() {
    let file = "tests/test_inputs/instr_testing/modules/block_entry/two_funcs_two_blocks.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];

    let ops_of_interest = vec![(
        SupportedOperators::Block,
        (InstrumentationMode::BlockEntry, block_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

// ==== BLOCK EXIT ====

#[test]
fn test_block_exit_one_func_nested_block() {
    let file = "tests/test_inputs/instr_testing/modules/block_exit/one_func_nested_block.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let loop_body = vec![Operator::I32Const { value: 34 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::BlockExit, block_body),
        ),
        (
            SupportedOperators::Loop,
            (InstrumentationMode::BlockExit, loop_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_exit_one_func_one_block() {
    let file = "tests/test_inputs/instr_testing/modules/block_exit/one_func_one_block.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let loop_body = vec![Operator::I32Const { value: 34 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::BlockExit, block_body),
        ),
        (
            SupportedOperators::Loop,
            (InstrumentationMode::BlockExit, loop_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_exit_one_func_two_blocks() {
    let file = "tests/test_inputs/instr_testing/modules/block_exit/one_func_two_blocks.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let loop_body = vec![Operator::I32Const { value: 34 }, Operator::Drop];
    let if_body = vec![Operator::I32Const { value: 56 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::BlockExit, block_body),
        ),
        (
            SupportedOperators::Loop,
            (InstrumentationMode::BlockExit, loop_body),
        ),
        (
            SupportedOperators::If,
            (InstrumentationMode::BlockExit, if_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_exit_two_funcs_nested_block() {
    let file = "tests/test_inputs/instr_testing/modules/block_exit/two_funcs_nested_block.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let loop_body = vec![Operator::I32Const { value: 34 }, Operator::Drop];
    let if_body = vec![Operator::I32Const { value: 56 }, Operator::Drop];
    let else_body = vec![Operator::I32Const { value: 78 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::BlockExit, block_body),
        ),
        (
            SupportedOperators::Loop,
            (InstrumentationMode::BlockExit, loop_body),
        ),
        (
            SupportedOperators::If,
            (InstrumentationMode::BlockExit, if_body),
        ),
        (
            SupportedOperators::Else,
            (InstrumentationMode::BlockExit, else_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_exit_two_funcs_one_block() {
    let file = "tests/test_inputs/instr_testing/modules/block_exit/two_funcs_one_block.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let loop_body = vec![Operator::I32Const { value: 34 }, Operator::Drop];
    let if_body = vec![Operator::I32Const { value: 56 }, Operator::Drop];
    let else_body = vec![Operator::I32Const { value: 78 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::BlockExit, block_body),
        ),
        (
            SupportedOperators::Loop,
            (InstrumentationMode::BlockExit, loop_body),
        ),
        (
            SupportedOperators::If,
            (InstrumentationMode::BlockExit, if_body),
        ),
        (
            SupportedOperators::Else,
            (InstrumentationMode::BlockExit, else_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_block_exit_two_funcs_two_blocks() {
    let file = "tests/test_inputs/instr_testing/modules/block_exit/two_funcs_two_blocks.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];

    let ops_of_interest = vec![(
        SupportedOperators::Block,
        (InstrumentationMode::BlockExit, block_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

// ==== FUNCTION ENTRY ====

#[test]
fn test_fn_entry_one_func() {
    let file = "tests/test_inputs/instr_testing/modules/fn_entry/one_func.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let fn_entry_body = vec![Operator::I32Const { value: 1 }, Operator::Drop];

    inject_function_entry(&mut mod_it, fn_entry_body);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_fn_entry_two_funcs() {
    let file = "tests/test_inputs/instr_testing/modules/fn_entry/two_funcs.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let fn_entry_body = vec![Operator::I32Const { value: 1 }, Operator::Drop];

    inject_function_entry(&mut mod_it, fn_entry_body);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

// ==== FUNCTION EXIT ====

#[test]
fn test_fn_exit_one_func() {
    let file = "tests/test_inputs/instr_testing/modules/fn_exit/one_func.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let fn_entry_body = vec![Operator::I32Const { value: 1 }, Operator::Drop];

    inject_function_exit(&mut mod_it, fn_entry_body);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_fn_exit_two_funcs() {
    let file = "tests/test_inputs/instr_testing/modules/fn_exit/two_funcs.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let fn_entry_body = vec![Operator::I32Const { value: 1 }, Operator::Drop];

    inject_function_exit(&mut mod_it, fn_entry_body);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

// ==== SEMANTIC AFTER ====
#[test]
fn test_semantic_after_complex_mult_nested_diff_opcodes() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/complex_mult_nested_diff_opcodes.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let loop_body = vec![Operator::I32Const { value: 23 }, Operator::Drop];
    let if_body = vec![Operator::I32Const { value: 34 }, Operator::Drop];
    let else_body = vec![Operator::I32Const { value: 45 }, Operator::Drop];
    let br_body = vec![Operator::I32Const { value: 56 }, Operator::Drop];
    let br_if_body = vec![Operator::I32Const { value: 67 }, Operator::Drop];
    let br_table_body = vec![Operator::I32Const { value: 78 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::SemanticAfter, block_body),
        ),
        (
            SupportedOperators::Loop,
            (InstrumentationMode::SemanticAfter, loop_body),
        ),
        (
            SupportedOperators::If,
            (InstrumentationMode::SemanticAfter, if_body),
        ),
        (
            SupportedOperators::Else,
            (InstrumentationMode::SemanticAfter, else_body),
        ),
        (
            SupportedOperators::Br,
            (InstrumentationMode::SemanticAfter, br_body),
        ),
        (
            SupportedOperators::BrIf,
            (InstrumentationMode::SemanticAfter, br_if_body),
        ),
        (
            SupportedOperators::BrTable,
            (InstrumentationMode::SemanticAfter, br_table_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_medium_1br() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/medium_1br.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let br_body = vec![Operator::I32Const { value: 23 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::SemanticAfter, block_body),
        ),
        (
            SupportedOperators::Br,
            (InstrumentationMode::SemanticAfter, br_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_medium_1br_if() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/medium_1br_if.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let br_if_body = vec![Operator::I32Const { value: 23 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::SemanticAfter, block_body),
        ),
        (
            SupportedOperators::BrIf,
            (InstrumentationMode::SemanticAfter, br_if_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_medium_1br_table() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/medium_1br_table.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let br_table_body = vec![Operator::I32Const { value: 23 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::SemanticAfter, block_body),
        ),
        (
            SupportedOperators::BrTable,
            (InstrumentationMode::SemanticAfter, br_table_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_medium_2br() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/medium_2br.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let br_body = vec![Operator::I32Const { value: 23 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::SemanticAfter, block_body),
        ),
        (
            SupportedOperators::Br,
            (InstrumentationMode::SemanticAfter, br_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_medium_2br_if() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/medium_2br_if.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let br_if_body = vec![Operator::I32Const { value: 23 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::SemanticAfter, block_body),
        ),
        (
            SupportedOperators::BrIf,
            (InstrumentationMode::SemanticAfter, br_if_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_medium_2br_table() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/medium_2br_table.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let br_table_body = vec![Operator::I32Const { value: 23 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::SemanticAfter, block_body),
        ),
        (
            SupportedOperators::BrTable,
            (InstrumentationMode::SemanticAfter, br_table_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_medium_blocks() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/medium_blocks.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let br_table_body = vec![Operator::I32Const { value: 34 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::SemanticAfter, block_body),
        ),
        (
            SupportedOperators::BrTable,
            (InstrumentationMode::SemanticAfter, br_table_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_medium_ifelse() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/medium_ifelse.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let if_body = vec![Operator::I32Const { value: 23 }, Operator::Drop];
    let else_body = vec![Operator::I32Const { value: 34 }, Operator::Drop];
    let br_body = vec![Operator::I32Const { value: 45 }, Operator::Drop];
    let br_table_body = vec![Operator::I32Const { value: 56 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::SemanticAfter, block_body),
        ),
        (
            SupportedOperators::If,
            (InstrumentationMode::SemanticAfter, if_body),
        ),
        (
            SupportedOperators::Else,
            (InstrumentationMode::SemanticAfter, else_body),
        ),
        (
            SupportedOperators::Br,
            (InstrumentationMode::SemanticAfter, br_body),
        ),
        (
            SupportedOperators::BrTable,
            (InstrumentationMode::SemanticAfter, br_table_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_medium_ifs() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/medium_ifs.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let block_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let if_body = vec![Operator::I32Const { value: 23 }, Operator::Drop];
    let br_body = vec![Operator::I32Const { value: 34 }, Operator::Drop];
    let br_table_body = vec![Operator::I32Const { value: 45 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::Block,
            (InstrumentationMode::SemanticAfter, block_body),
        ),
        (
            SupportedOperators::If,
            (InstrumentationMode::SemanticAfter, if_body),
        ),
        (
            SupportedOperators::Br,
            (InstrumentationMode::SemanticAfter, br_body),
        ),
        (
            SupportedOperators::BrTable,
            (InstrumentationMode::SemanticAfter, br_table_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_medium_multiple() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/medium_multiple.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let br_if_body = vec![Operator::I32Const { value: 1234 }, Operator::Drop];
    let br_table_body = vec![Operator::I32Const { value: 5678 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::BrIf,
            (InstrumentationMode::SemanticAfter, br_if_body),
        ),
        (
            SupportedOperators::BrTable,
            (InstrumentationMode::SemanticAfter, br_table_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_medium_other_operators() {
    let _file = "tests/test_inputs/instr_testing/modules/semantic_after/medium_other_operators.wat";
    // todo -- test the other operators (when I know how to write wat using them)
}

#[test]
fn test_semantic_after_simple_1br() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/simple_1br.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let br_body = vec![Operator::I32Const { value: 1234 }, Operator::Drop];

    let ops_of_interest = vec![(
        SupportedOperators::Br,
        (InstrumentationMode::SemanticAfter, br_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_simple_1br_if() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/simple_1br_if.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let br_if_body = vec![Operator::I32Const { value: 1234 }, Operator::Drop];

    let ops_of_interest = vec![(
        SupportedOperators::BrIf,
        (InstrumentationMode::SemanticAfter, br_if_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_simple_1br_table() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/simple_1br_table.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let br_table_body = vec![Operator::I32Const { value: 1234 }, Operator::Drop];

    let ops_of_interest = vec![(
        SupportedOperators::BrTable,
        (InstrumentationMode::SemanticAfter, br_table_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_simple_1if() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/simple_1if.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let if_body = vec![Operator::I32Const { value: 12 }, Operator::Drop];
    let br_body = vec![Operator::I32Const { value: 34 }, Operator::Drop];

    let ops_of_interest = vec![
        (
            SupportedOperators::If,
            (InstrumentationMode::SemanticAfter, if_body),
        ),
        (
            SupportedOperators::Br,
            (InstrumentationMode::SemanticAfter, br_body),
        ),
    ];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_simple_2br() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/simple_2br.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let br_body = vec![Operator::I32Const { value: 1234 }, Operator::Drop];

    let ops_of_interest = vec![(
        SupportedOperators::Br,
        (InstrumentationMode::SemanticAfter, br_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_simple_2br_if() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/simple_2br_if.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let br_if_body = vec![Operator::I32Const { value: 1234 }, Operator::Drop];

    let ops_of_interest = vec![(
        SupportedOperators::BrIf,
        (InstrumentationMode::SemanticAfter, br_if_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn test_semantic_after_simple_2br_table() {
    let file = "tests/test_inputs/instr_testing/modules/semantic_after/simple_2br_table.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");
    let mut mod_it = ModuleIterator::new(&mut module, &vec![]);

    let br_table_body = vec![Operator::I32Const { value: 1234 }, Operator::Drop];

    let ops_of_interest = vec![(
        SupportedOperators::BrTable,
        (InstrumentationMode::SemanticAfter, br_table_body),
    )];
    run_block_injection(&mut mod_it, &ops_of_interest);

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

#[test]
fn add_imports_when_has_start_func() {
    let file = "tests/test_inputs/instr_testing/modules/add-imports-when-has-start-func.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false).expect("Unable to parse");

    module.add_import_func("ima".to_string(), "new_import".to_string(), TypeID(0));
    module.add_import_func("ya_dont".to_string(), "say".to_string(), TypeID(0));

    let result = module.encode();
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    if let Err(e) = check_instrumentation_encoding(&out, file) {
        error!(
            "Something went wrong when checking instrumentation encoding: {}",
            e
        )
    }
}

// =================
// ==== HELPERS ====
// =================

// This logic is necessary to avoid using `discriminant` fn for more complex enums
#[derive(Debug)]
enum SupportedOperators {
    // block-style
    Block,
    Loop,
    If,
    Else,

    // branching
    Br,
    BrIf,
    BrTable,
    // TODO -- test the following
    // BrOnCast,
    // BrOnCastFail,
    // BrOnNull,
    // BrOnNonNull
}

fn run_block_injection<'a, 'b, 'c>(
    mod_it: &mut ModuleIterator<'a, 'b>,
    ops_of_interest: &Vec<(SupportedOperators, (InstrumentationMode, Vec<Operator<'c>>))>,
) where
    'c: 'b,
{
    loop {
        let op = mod_it.curr_op();
        if let Location::Module {
            func_idx,
            instr_idx,
        } = mod_it.curr_loc().0
        {
            trace!("Func: {:?}, {}: {:?},", func_idx, instr_idx, op);

            for (op, (mode, body)) in ops_of_interest.iter() {
                let matches = match op {
                    SupportedOperators::Block => {
                        matches!(mod_it.curr_op().unwrap(), Operator::Block { .. })
                    }
                    SupportedOperators::Loop => {
                        matches!(mod_it.curr_op().unwrap(), Operator::Loop { .. })
                    }
                    SupportedOperators::If => {
                        matches!(mod_it.curr_op().unwrap(), Operator::If { .. })
                    }
                    SupportedOperators::Else => {
                        matches!(mod_it.curr_op().unwrap(), Operator::Else { .. })
                    }
                    SupportedOperators::Br => {
                        matches!(mod_it.curr_op().unwrap(), Operator::Br { .. })
                    }
                    SupportedOperators::BrIf => {
                        matches!(mod_it.curr_op().unwrap(), Operator::BrIf { .. })
                    }
                    SupportedOperators::BrTable => {
                        matches!(mod_it.curr_op().unwrap(), Operator::BrTable { .. })
                    }
                };
                if matches {
                    if !body.is_empty() {
                        // has body
                        mod_it.set_instrument_mode(*mode);
                        mod_it.inject_all(body);
                        mod_it.finish_instr();
                    } else {
                        // has no body...effectively removing
                        match mode {
                            InstrumentationMode::Alternate => {
                                mod_it.empty_alternate();
                            }
                            InstrumentationMode::BlockAlt => {
                                mod_it.empty_block_alt();
                            }
                            _ => {
                                // go ahead and call inject_all anyway (just in case this becomes important at some point)
                                mod_it.inject_all(body);
                            }
                        }
                    }
                }
            }

            if mod_it.next().is_none() {
                break;
            };
        } else {
            panic!("Should've gotten Module Location!");
        }
    }
}

fn inject_function_entry<'a, 'b, 'c>(mod_it: &mut ModuleIterator<'a, 'b>, body: Vec<Operator<'c>>)
where
    'c: 'b,
{
    let mut curr_func = None;
    loop {
        if let Location::Module { func_idx, .. } = mod_it.curr_loc().0 {
            if curr_func != Some(func_idx) {
                // visiting a new function, instrument it!
                mod_it.func_entry();
                mod_it.inject_all(&body);
            }
            curr_func = Some(func_idx);
        }

        if mod_it.next().is_none() {
            break;
        };
    }
}

fn inject_function_exit<'a, 'b, 'c>(mod_it: &mut ModuleIterator<'a, 'b>, body: Vec<Operator<'c>>)
where
    'c: 'b,
{
    let mut curr_func = None;
    loop {
        if let Location::Module { func_idx, .. } = mod_it.curr_loc().0 {
            if curr_func != Some(func_idx) {
                // visiting a new function, instrument it!
                mod_it.func_exit();
                mod_it.inject_all(&body);
            }
            curr_func = Some(func_idx);
        }

        if mod_it.next().is_none() {
            break;
        };
    }
}
