//! Coverage for the exceptions proposal.
//!
//! `throw_ref` is deferred: it needs an `exnref` on the stack, which is only produced by
//! `try_table` / catch blocks.

use crate::opcode_test;

const BASE_WAT: &str = r#"
    (module
        (tag $e (param i32))
        (func $target nop))
"#;

const TARGET: u32 = 0;

opcode_test!(throw_ops, BASE_WAT, TARGET,
    .i32_const(1).throw(0)
);
