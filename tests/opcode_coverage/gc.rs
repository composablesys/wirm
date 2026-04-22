//! Coverage for the GC proposal.
//!
//! Deferred: `br_on_cast`, `br_on_cast_fail` (wirm skips — manual impl required, need
//! block context); `br_on_null`/`br_on_non_null` (per plan use SemanticAfter injection).

use wirm::ir::id::{DataSegmentID, ElementID, FieldID, TypeID};
use wirm::ir::module::module_types::{AbstractHeapType, HeapType};
use wirm::ir::types::{BlockType, DataType};

use crate::opcode_test;

// Type index assignments:
//   0: $s   struct (field (mut i32))        — existing, used by struct/cast tests
//   1: $a   array  (mut i32)                — existing, used by array_ops
//   2: $sp  struct (field (mut i8))         — packed struct, for struct_get_s/_u
//   3: $ab  array  (mut i8)                 — packed array, for array_get_s/_u
//   4: $af  array  (ref null func)          — ref array, for array_new_elem/init_elem
// Segment indices: $d is DataSegmentID(0), $e is ElementID(0).
const BASE_WAT: &str = r#"
    (module
        (type $s (struct (field (mut i32))))
        (type $a (array (mut i32)))
        (type $sp (struct (field (mut i8))))
        (type $ab (array (mut i8)))
        (type $af (array (mut (ref null func))))
        (func $target
            (local $sref (ref null $s))
            (local $aref (ref null $a))
            ;; trailing data.drop / elem.drop ensures the module declares a
            ;; data count section, which is required once any
            ;; array.new_data / array.init_data op is present (injected or not).
            data.drop 0
            elem.drop 0)
        (func $f)
        (data $d "\00\01\02\03\04\05\06\07")
        (elem $e func $f))
"#;

const TARGET: u32 = 0;

fn any_ht() -> HeapType {
    HeapType::Abstract {
        shared: false,
        ty: AbstractHeapType::Any,
    }
}

fn extern_null_ht() -> HeapType {
    HeapType::Abstract {
        shared: false,
        ty: AbstractHeapType::Extern,
    }
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

opcode_test!(array_copy_op, BASE_WAT, TARGET,
    .i32_const(3).array_new_default(TypeID(1))
    .i32_const(0)
    .i32_const(3).array_new_default(TypeID(1))
    .i32_const(0)
    .i32_const(1)
    .array_copy(TypeID(1), TypeID(1))
);

opcode_test!(array_new_from_segment, BASE_WAT, TARGET,
    .i32_const(0).i32_const(2).array_new_data(TypeID(1), DataSegmentID(0)).drop()
    .i32_const(0).i32_const(1).array_new_elem(TypeID(4), ElementID(0)).drop()
);

opcode_test!(array_init_from_segment, BASE_WAT, TARGET,
    .i32_const(2).array_new_default(TypeID(1))
    .i32_const(0)
    .i32_const(0)
    .i32_const(2)
    .array_init_data(TypeID(1), DataSegmentID(0))
    .i32_const(1).array_new_default(TypeID(4))
    .i32_const(0)
    .i32_const(0)
    .i32_const(1)
    .array_init_elem(TypeID(4), ElementID(0))
);

opcode_test!(packed_struct_get, BASE_WAT, TARGET,
    .struct_new_default(TypeID(2)).struct_get_s(TypeID(2), FieldID(0)).drop()
    .struct_new_default(TypeID(2)).struct_get_u(TypeID(2), FieldID(0)).drop()
);

opcode_test!(packed_array_get, BASE_WAT, TARGET,
    .i32_const(3).array_new_default(TypeID(3)).i32_const(0).array_get_s(TypeID(3)).drop()
    .i32_const(3).array_new_default(TypeID(3)).i32_const(0).array_get_u(TypeID(3)).drop()
);

opcode_test!(ref_cast_test_ops, BASE_WAT, TARGET,
    .struct_new_default(TypeID(0)).ref_test(any_ht()).drop()
    .struct_new_default(TypeID(0)).ref_test_null(any_ht()).drop()
    .struct_new_default(TypeID(0)).ref_cast(any_ht()).drop()
    .struct_new_default(TypeID(0)).ref_cast_null(any_ht()).drop()
);

// br_on_cast / br_on_cast_fail: inside a block whose result type matches the
// from-type (nullable anyref), cast to a subtype. Block consumes the ref whether
// the branch is taken or not.
opcode_test!(br_on_cast_op, BASE_WAT, TARGET,
    .block(BlockType::Type(DataType::AnyNull))
        .struct_new_default(TypeID(0))
        .br_on_cast(0, wasmparser::RefType::ANYREF, wasmparser::RefType::ANYREF)
    .end()
    .drop()
);

opcode_test!(br_on_cast_fail_op, BASE_WAT, TARGET,
    .block(BlockType::Type(DataType::AnyNull))
        .struct_new_default(TypeID(0))
        .br_on_cast_fail(0, wasmparser::RefType::ANYREF, wasmparser::RefType::ANYREF)
    .end()
    .drop()
);

opcode_test!(ref_gc_misc, BASE_WAT, TARGET,
    .ref_null(extern_null_ht()).any_convert_extern().drop()
    .ref_null(any_ht()).extern_convert_any().drop()
    .i32_const(1).ref_i31().drop()
    .i32_const(1).ref_i31().i31_get_s().drop()
    .i32_const(1).ref_i31().i31_get_u().drop()
);
