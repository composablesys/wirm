//! Coverage for the custom-descriptors proposal (auto-generated subset).
//!
//! `ref_cast_desc_*` and `br_on_cast_desc{,_fail}` are skipped by wirm's macro (manual
//! impl required — see `src/opcode.rs:182`). Only the three auto-generated ops land here.

use wirm::ir::id::TypeID;

use crate::opcode_test;

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
