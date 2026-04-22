//! Coverage for the function-references proposal.

use wirm::ir::id::{FunctionID, LocalID, TypeID};
use wirm::ir::types::{BlockType, DataType};

use crate::opcode_test;

const BASE_WAT: &str = r#"
    (module
        (type $v (func))
        (func $target (local $f (ref null $v)) nop)
        (func $dummy (type $v))
        (elem declare func $dummy))
"#;

const TARGET: u32 = 0;

opcode_test!(func_ref_ops, BASE_WAT, TARGET,
    .local_get(LocalID(0)).call_ref(TypeID(0))
    .local_get(LocalID(0)).return_call_ref(TypeID(0))
    .ref_func(FunctionID(1)).ref_as_non_null().drop()
);

opcode_test!(br_on_null_op, BASE_WAT, TARGET,
    .block(BlockType::Empty)
        .local_get(LocalID(0)).br_on_null(0).drop()
    .end()
);

opcode_test!(br_on_non_null_op, BASE_WAT, TARGET,
    .block(BlockType::Type(DataType::Module { ty_id: 0, nullable: true }))
        .local_get(LocalID(0)).br_on_non_null(0)
        .local_get(LocalID(0))
    .end()
    .drop()
);
