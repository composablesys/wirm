//! Coverage for the tail-call proposal.

use wirm::ir::id::{FunctionID, TableID, TypeID};

use crate::opcode_test;

const BASE_WAT: &str = r#"
    (module
        (type $ii_i (func (param i32 i32) (result i32)))
        (table $t 1 funcref)
        (func $target (result i32) i32.const 0)
        (func $callee (type $ii_i) local.get 0)
        (elem (i32.const 0) $callee))
"#;

const TARGET: u32 = 0;

opcode_test!(tail_call_ops, BASE_WAT, TARGET,
    .i32_const(1).i32_const(2).return_call(FunctionID(1))
    .i32_const(1).i32_const(2).i32_const(0).return_call_indirect(TypeID(0), TableID(0))
);
