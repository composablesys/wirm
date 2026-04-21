//! Coverage for the reference-types proposal.

use wirm::ir::id::{FunctionID, TableID};
use wirm::ir::module::module_types::{AbstractHeapType, HeapType};

use crate::opcode_test;

const BASE_WAT: &str = r#"
    (module
        (table $t 2 funcref)
        (func $target nop)
        (func $f)
        (elem declare func $f))
"#;

const TARGET: u32 = 0;

fn funcref_ht() -> HeapType {
    HeapType::Abstract { shared: false, ty: AbstractHeapType::Func }
}

opcode_test!(ref_ops, BASE_WAT, TARGET,
    .ref_null(funcref_ht()).ref_is_null().drop()
    .ref_func(FunctionID(1)).drop()
);

opcode_test!(table_ref_ops, BASE_WAT, TARGET,
    .i32_const(0).table_get(TableID(0)).drop()
    .i32_const(0).ref_null(funcref_ht()).table_set(TableID(0))
    .table_size(TableID(0)).drop()
    .ref_null(funcref_ht()).i32_const(0).table_grow(TableID(0)).drop()
    .i32_const(0).ref_null(funcref_ht()).i32_const(0).table_fill(TableID(0))
);
