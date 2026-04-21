//! Coverage for the GC proposal.
//!
//! Deferred: `br_on_cast`, `br_on_cast_fail` (wirm skips — manual impl required, need
//! block context); `br_on_null`/`br_on_non_null` (per plan use SemanticAfter injection).

use wirm::ir::id::{FieldID, TypeID};
use wirm::ir::module::module_types::{AbstractHeapType, HeapType};

use crate::opcode_test;

const BASE_WAT: &str = r#"
    (module
        (type $s (struct (field (mut i32))))
        (type $a (array (mut i32)))
        (func $target
            (local $sref (ref null $s))
            (local $aref (ref null $a))
            nop))
"#;

const TARGET: u32 = 0;

fn any_ht() -> HeapType {
    HeapType::Abstract { shared: false, ty: AbstractHeapType::Any }
}

fn extern_null_ht() -> HeapType {
    HeapType::Abstract { shared: false, ty: AbstractHeapType::Extern }
}

opcode_test!(struct_ops, BASE_WAT, TARGET,
    .i32_const(1).struct_new(TypeID(0)).drop()
    .struct_new_default(TypeID(0)).drop()
    .struct_new_default(TypeID(0)).struct_get(TypeID(0), FieldID(0)).drop()
    .struct_new_default(TypeID(0)).i32_const(1).struct_set(TypeID(0), FieldID(0))
);

opcode_test!(ref_eq_op, BASE_WAT, TARGET,
    .struct_new_default(TypeID(0)).struct_new_default(TypeID(0)).ref_eq().drop()
);

opcode_test!(array_ops, BASE_WAT, TARGET,
    .i32_const(0).i32_const(3).array_new(TypeID(1)).drop()
    .i32_const(3).array_new_default(TypeID(1)).drop()
    .i32_const(1).i32_const(2).i32_const(3).array_new_fixed(TypeID(1), 3).drop()
    .i32_const(3).array_new_default(TypeID(1)).i32_const(0).array_get(TypeID(1)).drop()
    .i32_const(3).array_new_default(TypeID(1)).i32_const(0).i32_const(1).array_set(TypeID(1))
    .i32_const(3).array_new_default(TypeID(1)).array_len().drop()
    .i32_const(3).array_new_default(TypeID(1)).i32_const(0).i32_const(1).i32_const(1).array_fill(TypeID(1))
);

opcode_test!(ref_cast_test_ops, BASE_WAT, TARGET,
    .struct_new_default(TypeID(0)).ref_test(any_ht()).drop()
    .struct_new_default(TypeID(0)).ref_test_null(any_ht()).drop()
    .struct_new_default(TypeID(0)).ref_cast(any_ht()).drop()
    .struct_new_default(TypeID(0)).ref_cast_null(any_ht()).drop()
);

opcode_test!(ref_gc_misc, BASE_WAT, TARGET,
    .ref_null(extern_null_ht()).any_convert_extern().drop()
    .ref_null(any_ht()).extern_convert_any().drop()
    .i32_const(1).ref_i31().drop()
    .i32_const(1).ref_i31().i31_get_s().drop()
    .i32_const(1).ref_i31().i31_get_u().drop()
);
