//! Coverage for the sign-extension operators proposal.

use crate::opcode_test;

const BASE_WAT: &str = r#"
    (module
        (func $target (local i32 i64) nop))
"#;

const TARGET: u32 = 0;

opcode_test!(sign_extension_ops, BASE_WAT, TARGET,
    .i32_const(1).i32_extend8_s().drop()
    .i32_const(1).i32_extend16_s().drop()
    .i64_const(1).i64_extend8_s().drop()
    .i64_const(1).i64_extend16_s().drop()
    .i64_const(1).i64_extend32_s().drop()
);
