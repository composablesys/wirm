//! Coverage for the exceptions proposal.

use wasmparser::Catch;
use wirm::ir::types::{BlockType, DataType};

use crate::opcode_test;

const BASE_WAT: &str = r#"
    (module
        (tag $e (param i32))
        (tag $f)
        (func $target nop))
"#;

const TARGET: u32 = 0;

opcode_test!(throw_ops, BASE_WAT, TARGET,
    .i32_const(1).throw(0)
);

// try_table with no catches — the body runs and falls through normally.
opcode_test!(try_table_empty, BASE_WAT, TARGET,
    .try_table(BlockType::Empty, vec![])
        .nop()
    .end()
);

// try_table with a `catch` clause: exception with tag 0 branches to the outer
// block with the tag's i32 payload on the stack.
opcode_test!(try_table_catch, BASE_WAT, TARGET,
    .block(BlockType::Type(DataType::I32))
        .try_table(BlockType::Empty, vec![Catch::One { tag: 0, label: 0 }])
            .i32_const(42).throw(0)
        .end()
        .unreachable()
    .end()
    .drop()
);

// try_table with `catch_all_ref`: produces an exnref, which throw_ref then
// consumes. This is the canonical round-trip that unblocks throw_ref coverage.
opcode_test!(try_table_catch_all_ref_throw_ref, BASE_WAT, TARGET,
    .block(BlockType::Type(DataType::Exn))
        .try_table(BlockType::Empty, vec![Catch::AllRef { label: 0 }])
            .i32_const(1).throw(0)
        .end()
        .unreachable()
    .end()
    .throw_ref()
);

// try_table with `catch_ref` on a no-payload tag: pushes only the exnref. Using
// tag $f (no params) keeps the target block's signature simple — we don't need a
// multi-value function-type block just to exercise the `OneRef` variant.
opcode_test!(try_table_catch_ref, BASE_WAT, TARGET,
    .block(BlockType::Type(DataType::Exn))
        .try_table(BlockType::Empty, vec![Catch::OneRef { tag: 1, label: 0 }])
            .i32_const(7).throw(0)
        .end()
        .unreachable()
    .end()
    .throw_ref()
);

// try_table with `catch_all`: any exception goes to label 0, no payload.
opcode_test!(try_table_catch_all, BASE_WAT, TARGET,
    .try_table(BlockType::Empty, vec![Catch::All { label: 0 }])
        .i32_const(1).throw(0)
    .end()
);
