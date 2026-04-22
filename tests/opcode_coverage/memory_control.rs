//! Coverage for the memory-control proposal (auto-generated).

use wirm::ir::id::MemoryID;

use crate::opcode_test;

const BASE_WAT: &str = r#"
    (module
        (memory 1 1 shared)
        (func $target nop))
"#;

const TARGET: u32 = 0;

opcode_test!(memory_discard_op, BASE_WAT, TARGET,
    .i32_const(0).i32_const(1).memory_discard(MemoryID(0))
);
