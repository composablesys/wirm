use std::collections::HashMap;
use std::mem::discriminant;

use log::trace;
use wasmparser::Operator;

use wirm::ir::id::{FunctionID, TypeID};
use wirm::ir::types::{DataType, InstrumentationMode};
use wirm::iterator::component_iterator::ComponentIterator;
use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
use wirm::module_builder::AddLocal;
use wirm::opcode::Instrumenter;
use wirm::Opcode;
use wirm::{Component, Location, Module};

use crate::common::{
    check_instrumentation_encoding, inject_function_entry, inject_function_exit,
    run_block_injection, run_component_instr_test, run_module_instr_test, SupportedOperators,
};

#[test]
fn no_injection() {
    let file = "tests/test_inputs/handwritten/components/add.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut component = Component::parse(&buff, false, false, false).expect("Unable to parse");
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
    run_component_instr_test(
        "tests/test_inputs/instr_testing/components/add-inject_i32_before.wat",
        |comp_it| {
            let interested = Operator::Call { function_index: 1 };
            loop {
                if let Location::Component {
                    mod_idx,
                    func_idx,
                    instr_idx,
                } = comp_it.curr_loc().0
                {
                    trace!(
                        "Mod: {:?}, Func: {:?}, +{}: {:?}",
                        mod_idx,
                        func_idx,
                        instr_idx,
                        comp_it.curr_op()
                    );
                    if *comp_it.curr_op().unwrap() == interested {
                        comp_it.before().i32_const(1);
                    }
                    if comp_it.next().is_none() {
                        break;
                    }
                } else {
                    panic!("Should've gotten Component Location!");
                }
            }
        },
    );
}

#[test]
fn iterator_inject_all_variations() {
    run_component_instr_test(
        "tests/test_inputs/instr_testing/components/add-inject_all_variations.wat",
        |comp_it| {
            let after = Operator::Call { function_index: 1 };
            let before = Operator::Drop;
            let alternate = Operator::I32Const { value: 2 };
            loop {
                if let Location::Component {
                    mod_idx,
                    func_idx,
                    instr_idx,
                } = comp_it.curr_loc().0
                {
                    trace!(
                        "Mod: {:?}, Func: {:?}, +{}: {:?}",
                        mod_idx,
                        func_idx,
                        instr_idx,
                        comp_it.curr_op()
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
                    }
                } else {
                    panic!("Should've gotten Component Location!");
                }
            }
        },
    );
}

#[test]
fn test_inject_locals() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/add-inject_locals.wat",
        |mod_it| {
            let mut is_first = true;
            while is_first || mod_it.next().is_some() {
                if let Location::Module {
                    func_idx,
                    instr_idx,
                } = mod_it.curr_loc().0
                {
                    trace!(
                        "Func: {:?}, {}: {:?},",
                        func_idx,
                        instr_idx,
                        mod_it.curr_op()
                    );
                    if mod_it.curr_op().unwrap() == &Operator::I32Add {
                        let local_id = mod_it.add_local(DataType::I32);
                        trace!("new Local ID: {:?}", local_id);
                    }
                    if mod_it.curr_op().unwrap() == &(Operator::I32Const { value: 2 }) {
                        let local_id = mod_it.add_local(DataType::I32);
                        println!("new Local ID: {:?}", local_id);
                    }
                } else {
                    panic!("Should've gotten Module Location!");
                }
                is_first = false;
            }
        },
    );
}

// ==== BLOCK ALT ====

#[test]
fn test_block_alt_one_func_nested_block() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_alt/one_func_nested_block.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::Loop,
                    (
                        InstrumentationMode::BlockAlt,
                        vec![Operator::I32Const { value: 12 }, Operator::Drop],
                    ),
                )],
            );
        },
    );
}

#[test]
fn test_block_alt_one_func_remove_else() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_alt/one_func_remove_else.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::Else,
                    (InstrumentationMode::BlockAlt, vec![]),
                )],
            );
        },
    );
}

#[test]
fn test_block_alt_one_func_replace_else() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_alt/one_func_replace_else.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::Else,
                    (
                        InstrumentationMode::BlockAlt,
                        vec![Operator::I32Const { value: 12 }, Operator::Drop],
                    ),
                )],
            );
        },
    );
}

#[test]
fn test_block_alt_one_func_two_blocks() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_alt/one_func_two_blocks.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::Block,
                    (
                        InstrumentationMode::BlockAlt,
                        vec![Operator::I32Const { value: 12 }, Operator::Drop],
                    ),
                )],
            );
        },
    );
}

#[test]
fn test_block_alt_remove_else_nested_if() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_alt/remove_else_nested_if.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::Else,
                    (InstrumentationMode::BlockAlt, vec![]),
                )],
            );
        },
    );
}

#[test]
fn test_block_alt_remove_else_with_instrumented_after_if() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_alt/remove_else_with_instr'd_after_if.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::If,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Else,
                        (InstrumentationMode::BlockAlt, vec![]),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_block_alt_remove_else_with_instrumented_exit_if() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_alt/remove_else_with_instr'd_exit_if.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::If,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Else,
                        (InstrumentationMode::BlockAlt, vec![]),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_block_alt_remove_entire_block() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_alt/remove_entire_block.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::Block,
                    (InstrumentationMode::BlockAlt, vec![]),
                )],
            );
        },
    );
}

#[test]
fn test_block_alt_remove_if_with_else() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_alt/remove_if_with_else.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::If,
                    (InstrumentationMode::BlockAlt, vec![]),
                )],
            );
        },
    );
}

#[test]
fn test_block_alt_remove_nested_block() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_alt/remove_nested_block.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::Block,
                    (InstrumentationMode::BlockAlt, vec![]),
                )],
            );
        },
    );
}

#[test]
fn test_block_alt_replace_else_nested_if() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_alt/replace_else_nested_if.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::Else,
                    (
                        InstrumentationMode::BlockAlt,
                        vec![Operator::I32Const { value: 12 }, Operator::Drop],
                    ),
                )],
            );
        },
    );
}

#[test]
fn test_block_alt_replace_if_with_else() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_alt/replace_if_with_else.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::If,
                    (
                        InstrumentationMode::BlockAlt,
                        vec![
                            Operator::Drop,
                            Operator::I32Const { value: 12 },
                            Operator::Drop,
                        ],
                    ),
                )],
            );
        },
    );
}

#[test]
fn test_block_alt_replace_nested_block() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_alt/replace_nested_block.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::Block,
                    (
                        InstrumentationMode::BlockAlt,
                        vec![Operator::I32Const { value: 12 }, Operator::Drop],
                    ),
                )],
            );
        },
    );
}

// ==== BLOCK ENTRY ====

#[test]
fn test_block_entry_one_func_nested_block() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_entry/one_func_nested_block.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::BlockEntry,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Loop,
                        (
                            InstrumentationMode::BlockEntry,
                            vec![Operator::I32Const { value: 34 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_block_entry_one_func_one_block() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_entry/one_func_one_block.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::BlockEntry,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Loop,
                        (
                            InstrumentationMode::BlockEntry,
                            vec![Operator::I32Const { value: 34 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_block_entry_one_func_two_blocks() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_entry/one_func_two_blocks.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::BlockEntry,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Loop,
                        (
                            InstrumentationMode::BlockEntry,
                            vec![Operator::I32Const { value: 34 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::If,
                        (
                            InstrumentationMode::BlockEntry,
                            vec![Operator::I32Const { value: 56 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_block_entry_two_funcs_nested_block() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_entry/two_funcs_nested_block.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::BlockEntry,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Loop,
                        (
                            InstrumentationMode::BlockEntry,
                            vec![Operator::I32Const { value: 34 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::If,
                        (
                            InstrumentationMode::BlockEntry,
                            vec![Operator::I32Const { value: 56 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Else,
                        (
                            InstrumentationMode::BlockEntry,
                            vec![Operator::I32Const { value: 78 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_block_entry_two_funcs_one_block() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_entry/two_funcs_one_block.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::BlockEntry,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Loop,
                        (
                            InstrumentationMode::BlockEntry,
                            vec![Operator::I32Const { value: 34 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::If,
                        (
                            InstrumentationMode::BlockEntry,
                            vec![Operator::I32Const { value: 56 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Else,
                        (
                            InstrumentationMode::BlockEntry,
                            vec![Operator::I32Const { value: 78 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_block_entry_two_funcs_two_blocks() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_entry/two_funcs_two_blocks.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::Block,
                    (
                        InstrumentationMode::BlockEntry,
                        vec![Operator::I32Const { value: 12 }, Operator::Drop],
                    ),
                )],
            );
        },
    );
}

// ==== BLOCK EXIT ====

#[test]
fn test_block_exit_one_func_nested_block() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_exit/one_func_nested_block.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Loop,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 34 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_block_exit_one_func_one_block() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_exit/one_func_one_block.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Loop,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 34 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_block_exit_one_func_two_blocks() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_exit/one_func_two_blocks.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Loop,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 34 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::If,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 56 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_block_exit_two_funcs_nested_block() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_exit/two_funcs_nested_block.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Loop,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 34 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::If,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 56 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Else,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 78 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_block_exit_two_funcs_one_block() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_exit/two_funcs_one_block.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Loop,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 34 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::If,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 56 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Else,
                        (
                            InstrumentationMode::BlockExit,
                            vec![Operator::I32Const { value: 78 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_block_exit_two_funcs_two_blocks() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/block_exit/two_funcs_two_blocks.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::Block,
                    (
                        InstrumentationMode::BlockExit,
                        vec![Operator::I32Const { value: 12 }, Operator::Drop],
                    ),
                )],
            );
        },
    );
}

// ==== FUNCTION ENTRY ====

#[test]
fn test_fn_entry_one_func() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/fn_entry/one_func.wat",
        |mod_it| {
            inject_function_entry(
                mod_it,
                vec![Operator::I32Const { value: 1 }, Operator::Drop],
            )
        },
    );
}

#[test]
fn test_fn_entry_two_funcs() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/fn_entry/two_funcs.wat",
        |mod_it| {
            inject_function_entry(
                mod_it,
                vec![Operator::I32Const { value: 1 }, Operator::Drop],
            )
        },
    );
}

// ==== FUNCTION EXIT ====

#[test]
fn test_fn_exit_one_func() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/fn_exit/one_func.wat",
        |mod_it| {
            inject_function_exit(
                mod_it,
                vec![Operator::I32Const { value: 1 }, Operator::Drop],
            )
        },
    );
}

#[test]
fn test_fn_exit_two_funcs() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/fn_exit/two_funcs.wat",
        |mod_it| {
            inject_function_exit(
                mod_it,
                vec![Operator::I32Const { value: 1 }, Operator::Drop],
            )
        },
    );
}

// ==== SEMANTIC AFTER ====

#[test]
fn test_semantic_after_complex_mult_nested_diff_opcodes() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/complex_mult_nested_diff_opcodes.wat",
        |mod_it| {
            run_block_injection(mod_it, &vec![
                (SupportedOperators::Block,   (InstrumentationMode::SemanticAfter, vec![Operator::I32Const { value: 12 }, Operator::Drop])),
                (SupportedOperators::Loop,    (InstrumentationMode::SemanticAfter, vec![Operator::I32Const { value: 23 }, Operator::Drop])),
                (SupportedOperators::If,      (InstrumentationMode::SemanticAfter, vec![Operator::I32Const { value: 34 }, Operator::Drop])),
                (SupportedOperators::Else,    (InstrumentationMode::SemanticAfter, vec![Operator::I32Const { value: 45 }, Operator::Drop])),
                (SupportedOperators::Br,      (InstrumentationMode::SemanticAfter, vec![Operator::I32Const { value: 56 }, Operator::Drop])),
                (SupportedOperators::BrIf,    (InstrumentationMode::SemanticAfter, vec![Operator::I32Const { value: 67 }, Operator::Drop])),
                (SupportedOperators::BrTable, (InstrumentationMode::SemanticAfter, vec![Operator::I32Const { value: 78 }, Operator::Drop])),
            ]);
        },
    );
}

#[test]
fn test_semantic_after_medium_1br() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/medium_1br.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Br,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 23 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_semantic_after_medium_1br_if() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/medium_1br_if.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::BrIf,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 23 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_semantic_after_medium_1br_table() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/medium_1br_table.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::BrTable,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 23 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_semantic_after_medium_2br() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/medium_2br.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Br,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 23 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_semantic_after_medium_2br_if() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/medium_2br_if.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::BrIf,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 23 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_semantic_after_medium_2br_table() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/medium_2br_table.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::BrTable,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 23 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_semantic_after_medium_blocks() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/medium_blocks.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::BrTable,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 34 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_semantic_after_medium_ifelse() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/medium_ifelse.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::If,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 23 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Else,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 34 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Br,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 45 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::BrTable,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 56 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_semantic_after_medium_ifs() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/medium_ifs.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::Block,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::If,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 23 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Br,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 34 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::BrTable,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 45 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_semantic_after_medium_multiple() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/medium_multiple.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::BrIf,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 1234 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::BrTable,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 5678 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_semantic_after_medium_other_operators() {
    let _file = "tests/test_inputs/instr_testing/modules/semantic_after/medium_other_operators.wat";
    // todo -- test the other operators (when I know how to write wat using them)
}

#[test]
fn test_semantic_after_simple_1br() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/simple_1br.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::Br,
                    (
                        InstrumentationMode::SemanticAfter,
                        vec![Operator::I32Const { value: 1234 }, Operator::Drop],
                    ),
                )],
            );
        },
    );
}

#[test]
fn test_semantic_after_simple_1br_if() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/simple_1br_if.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::BrIf,
                    (
                        InstrumentationMode::SemanticAfter,
                        vec![Operator::I32Const { value: 1234 }, Operator::Drop],
                    ),
                )],
            );
        },
    );
}

#[test]
fn test_semantic_after_simple_1br_table() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/simple_1br_table.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::BrTable,
                    (
                        InstrumentationMode::SemanticAfter,
                        vec![Operator::I32Const { value: 1234 }, Operator::Drop],
                    ),
                )],
            );
        },
    );
}

#[test]
fn test_semantic_after_simple_1if() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/simple_1if.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![
                    (
                        SupportedOperators::If,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 12 }, Operator::Drop],
                        ),
                    ),
                    (
                        SupportedOperators::Br,
                        (
                            InstrumentationMode::SemanticAfter,
                            vec![Operator::I32Const { value: 34 }, Operator::Drop],
                        ),
                    ),
                ],
            );
        },
    );
}

#[test]
fn test_semantic_after_simple_2br() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/simple_2br.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::Br,
                    (
                        InstrumentationMode::SemanticAfter,
                        vec![Operator::I32Const { value: 1234 }, Operator::Drop],
                    ),
                )],
            );
        },
    );
}

#[test]
fn test_semantic_after_simple_2br_if() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/simple_2br_if.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::BrIf,
                    (
                        InstrumentationMode::SemanticAfter,
                        vec![Operator::I32Const { value: 1234 }, Operator::Drop],
                    ),
                )],
            );
        },
    );
}

#[test]
fn test_semantic_after_simple_2br_table() {
    run_module_instr_test(
        "tests/test_inputs/instr_testing/modules/semantic_after/simple_2br_table.wat",
        |mod_it| {
            run_block_injection(
                mod_it,
                &vec![(
                    SupportedOperators::BrTable,
                    (
                        InstrumentationMode::SemanticAfter,
                        vec![Operator::I32Const { value: 1234 }, Operator::Drop],
                    ),
                )],
            );
        },
    );
}

#[test]
fn add_imports_when_has_start_func() {
    let file = "tests/test_inputs/instr_testing/modules/add-imports-when-has-start-func.wat";
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false, false, false).expect("Unable to parse");
    module.add_import_func("ima".to_string(), "new_import".to_string(), TypeID(0));
    module.add_import_func("ya_dont".to_string(), "say".to_string(), TypeID(0));
    let result = module.encode().expect("error");
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    check_instrumentation_encoding(&out, file).expect("instrumentation encoding mismatch");
}
