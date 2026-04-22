//! Coverage for the `MacroOpcode` trait (`src/opcode.rs`) — convenience methods built on top
//! of `Opcode` that reinterpret unsigned integer literals as their signed counterparts.

use crate::opcode_test;

const BASE_WAT: &str = r#"
    (module
        (func $target nop))
"#;

const TARGET: u32 = 0;

opcode_test!(u32_and_u64_const, BASE_WAT, TARGET,
    .u32_const(0xdead_beef).drop()
    .u64_const(0xdead_beef_cafe_babe).drop()
);
