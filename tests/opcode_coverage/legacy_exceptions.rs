//! Coverage for the legacy exceptions proposal.
//!
//! Every op validates round-trip now that `try_stmt` is a manual impl — the legacy
//! try/catch block family can be synthesized end to end via the `Opcode` trait.

use wirm::ir::types::BlockType;

use crate::opcode_test;

const BASE_WAT: &str = r#"
    (module
        (tag $e (param i32))
        (func $target nop))
"#;

const TARGET: u32 = 0;

opcode_test!(legacy_try_catch, BASE_WAT, TARGET,
    .try_stmt(BlockType::Empty)
        .i32_const(1).throw(0)
    .catch(0)
        .drop()
    .end()
);

opcode_test!(legacy_try_catch_all, BASE_WAT, TARGET,
    .try_stmt(BlockType::Empty)
        .i32_const(1).throw(0)
    .catch_all()
        .nop()
    .end()
);

opcode_test!(legacy_try_delegate, BASE_WAT, TARGET,
    .block(BlockType::Empty)
        .try_stmt(BlockType::Empty)
            .i32_const(1).throw(0)
        .delegate(1)
    .end()
);

opcode_test!(legacy_try_rethrow, BASE_WAT, TARGET,
    .try_stmt(BlockType::Empty)
        .i32_const(1).throw(0)
    .catch(0)
        .drop()
        .rethrow(0)
    .end()
);
