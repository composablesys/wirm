use crate::ir::types::{Instructions, InstrumentationMode};
use std::panic::{catch_unwind, UnwindSafe};
use wasmparser::{BlockType, Operator};

#[test]
fn test_block_alt_unsupported_ops() {
    run_check_panics(
        NON_SPECIAL_OPERATORS,
        InstrumentationMode::BlockAlt,
        "block alternate",
    );
    run_check_panics(
        BRANCHING_OPERATORS,
        InstrumentationMode::BlockAlt,
        "block alternate",
    );
}
#[test]
fn test_block_alt_supported_ops() {
    run_check_works(BLOCK_STYLE_OPERATORS, InstrumentationMode::BlockAlt);
}

#[test]
fn test_block_entry_unsupported_ops() {
    run_check_panics(
        NON_SPECIAL_OPERATORS,
        InstrumentationMode::BlockEntry,
        "block entry",
    );
    run_check_panics(
        BRANCHING_OPERATORS,
        InstrumentationMode::BlockEntry,
        "block entry",
    );
}
#[test]
fn test_block_entry_supported_ops() {
    run_check_works(BLOCK_STYLE_OPERATORS, InstrumentationMode::BlockEntry);
}

#[test]
fn test_block_exit_unsupported_ops() {
    run_check_panics(
        NON_SPECIAL_OPERATORS,
        InstrumentationMode::BlockExit,
        "block exit",
    );
    run_check_panics(
        BRANCHING_OPERATORS,
        InstrumentationMode::BlockExit,
        "block exit",
    );
}
#[test]
fn test_block_exit_supported_ops() {
    run_check_works(BLOCK_STYLE_OPERATORS, InstrumentationMode::BlockExit);
}

#[test]
fn test_semantic_after_unsupported_ops() {
    run_check_panics(
        NON_SPECIAL_OPERATORS,
        InstrumentationMode::SemanticAfter,
        "semantic after",
    );
}
#[test]
fn test_semantic_after_supported_ops() {
    run_check_works(BLOCK_STYLE_OPERATORS, InstrumentationMode::SemanticAfter);
    run_check_works(BRANCHING_OPERATORS, InstrumentationMode::SemanticAfter);
}

// ==== HELPER FUNCTIONS ====

pub fn run_check_works(operators: &[Operator], mode: InstrumentationMode) {
    for op in operators {
        assert!(check_instr_op(op, mode));
    }
}

pub fn run_check_panics(operators: &[Operator], mode: InstrumentationMode, mode_name: &str) {
    for op in operators {
        let str = format!(
            "Cannot apply {mode_name} instrumentation mode to op type: {:?}",
            op
        );
        assert_panics_with_message(
            || {
                check_instr_op(op, mode);
            },
            str.as_str(),
        )
    }
}

pub fn assert_panics_with_message(func: impl FnOnce() + UnwindSafe, msg: &str) {
    let err = catch_unwind(func).expect_err("Didn't panic!");

    let chk = |panic_msg: &'_ str| {
        if !panic_msg.contains(msg) {
            panic!(
                "Expected a panic message containing `{}`; got: `{}`.",
                msg, panic_msg
            );
        }
    };

    err.downcast::<String>()
        .map(|s| chk(&s))
        .or_else(|err| err.downcast::<&str>().map(|s| chk(*s)))
        .expect("Unexpected panic type!");
}

fn check_instr_op(op: &Operator, mode: InstrumentationMode) -> bool {
    let mut instr = Instructions::new(vec![op.clone()]);
    instr.set_current_mode(0, mode);
    instr.add_instr(0, Operator::Nop); // should OR should not panic!
    true
}

// non-exhaustive
const NON_SPECIAL_OPERATORS: &[Operator] = &[
    Operator::Unreachable,
    Operator::Nop,
    Operator::Throw { tag_index: 0 },
    Operator::End,
    Operator::Return,
    Operator::Call { function_index: 0 },
    Operator::CallIndirect {
        type_index: 0,
        table_index: 0,
    },
];

const BRANCHING_OPERATORS: &[Operator] = &[
    Operator::Br { relative_depth: 0 },
    Operator::BrIf { relative_depth: 0 },
    // Operator::BrTable { targets: BrTable } // todo unsure how to do this init logic
    // Operator::BrOnCast {
    //     relative_depth: 0,
    //     from_ref_type: (),
    //     to_ref_type: (),
    // },
    // Operator::BrOnCastFail {
    //     relative_depth: 0,
    //     from_ref_type: (),
    //     to_ref_type: (),
    // },
    Operator::BrOnNull { relative_depth: 0 },
    Operator::BrOnNonNull { relative_depth: 0 },
];

const BLOCK_STYLE_OPERATORS: &[Operator] = &[
    Operator::Block {
        blockty: BlockType::Empty,
    },
    Operator::Loop {
        blockty: BlockType::Empty,
    },
    Operator::If {
        blockty: BlockType::Empty,
    },
    Operator::Else,
];
