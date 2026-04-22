//! Coverage for the reference-types proposal.

use wasmparser::Operator;
use wirm::ir::id::{FunctionID, TableID};
use wirm::ir::module::module_types::{AbstractHeapType, HeapType};
use wirm::Opcode as _;

use crate::common::OpRecorder;
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
    HeapType::Abstract {
        shared: false,
        ty: AbstractHeapType::Func,
    }
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

opcode_test!(typed_select_op, BASE_WAT, TARGET,
    .ref_null(funcref_ht()).ref_null(funcref_ht()).i32_const(0)
        .typed_select(wasmparser::ValType::Ref(wasmparser::RefType::FUNCREF))
        .drop()
);

// `typed_select_multi` with vec length > 1 is currently invalid Wasm (see
// wasm-encoder's `typed_select_multi` doc comment). Vec length 1 round-trips as
// `TypedSelect`, hiding the multi variant. Use the recorder to prove the method
// exists and emits the correct `Operator` variant.
#[test]
fn typed_select_multi_op() {
    let mut rec = OpRecorder::new();
    rec.typed_select_multi(vec![wasmparser::ValType::I32, wasmparser::ValType::I64]);
    let ops = rec.finish();
    assert!(
        matches!(&ops[0], Operator::TypedSelectMulti { tys } if tys.len() == 2),
        "expected TypedSelectMulti with 2 tys, got {:?}",
        ops[0],
    );
}
