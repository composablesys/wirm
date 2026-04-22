//! Coverage for the custom-descriptors proposal (auto-generated subset).
//!
//! `ref_cast_desc_*` and `br_on_cast_desc{,_fail}` are skipped by wirm's macro (manual
//! impl required — see `src/opcode.rs:182`). Only the three auto-generated ops land here.

use wasmparser::{PackedIndex, RefType, UnpackedIndex};
use wirm::ir::id::TypeID;
use wirm::ir::module::module_types::HeapType;
use wirm::ir::types::{BlockType, DataType};

use crate::opcode_test;

fn x_ht() -> HeapType {
    HeapType::Concrete(UnpackedIndex::Module(0))
}

fn x_ref(nullable: bool) -> RefType {
    RefType::concrete(nullable, PackedIndex::from_module_index(0).expect("index"))
}

// A struct type $X described by descriptor struct type $D. Both types are empty (no
// fields) — sufficient for struct.new_desc / struct.new_default_desc / ref.get_desc to
// typecheck without any field plumbing. Type indices: $X = 0, $D = 1.
const BASE_WAT: &str = r#"
    (module
        (rec
            (type $X (descriptor $D) (struct))
            (type $D (describes $X) (struct)))
        (func $target nop))
"#;

const TARGET: u32 = 0;

// Each chain: build a $D-ref via `struct.new_default`, then consume it to produce an
// $X-ref via the op under test. `ref.get_desc` recovers the $D back out.
opcode_test!(struct_new_desc_ops, BASE_WAT, TARGET,
    .struct_new_default(TypeID(1)).struct_new_desc(TypeID(0)).drop()
    .struct_new_default(TypeID(1)).struct_new_default_desc(TypeID(0)).drop()
    .struct_new_default(TypeID(1)).struct_new_default_desc(TypeID(0)).ref_get_desc(TypeID(0)).drop()
);

// ref.cast_desc / ref.cast_desc null. Stack: (ref_to_cast, descriptor_ref) -> cast_ref.
// We build an $X-ref, then produce a fresh $D-ref, then cast-with-descriptor back to $X.
opcode_test!(ref_cast_desc_ops, BASE_WAT, TARGET,
    .struct_new_default(TypeID(1)).struct_new_default_desc(TypeID(0))
        .struct_new_default(TypeID(1))
        .ref_cast_desc(x_ht()).drop()
    .struct_new_default(TypeID(1)).struct_new_default_desc(TypeID(0))
        .struct_new_default(TypeID(1))
        .ref_cast_desc_null(x_ht()).drop()
);

// br_on_cast_desc / _fail: inside a block, cast-with-descriptor and branch on
// success/failure. Block result type matches both the branched and fall-through
// values (from == to == (ref $X)).
opcode_test!(br_on_cast_desc_op, BASE_WAT, TARGET,
    .block(BlockType::Type(DataType::Module { ty_id: 0, nullable: false }))
        .struct_new_default(TypeID(1)).struct_new_default_desc(TypeID(0))
        .struct_new_default(TypeID(1))
        .br_on_cast_desc(0, x_ref(false), x_ref(false))
    .end()
    .drop()
);

opcode_test!(br_on_cast_desc_fail_op, BASE_WAT, TARGET,
    .block(BlockType::Type(DataType::Module { ty_id: 0, nullable: false }))
        .struct_new_default(TypeID(1)).struct_new_default_desc(TypeID(0))
        .struct_new_default(TypeID(1))
        .br_on_cast_desc_fail(0, x_ref(false), x_ref(false))
    .end()
    .drop()
);
