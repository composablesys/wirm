//! Coverage for the memory64 proposal.

use wasmparser::MemArg;
use wirm::ir::id::MemoryID;

use crate::opcode_test;

const BASE_WAT: &str = r#"
    (module
        (memory i64 1)
        (func $target (local i64) nop))
"#;

const TARGET: u32 = 0;

const MEM2_64: MemArg = MemArg {
    align: 2,
    max_align: 2,
    offset: 0,
    memory: 0,
};

opcode_test!(memory64_ops, BASE_WAT, TARGET,
    .i64_const(0).i32_load(MEM2_64).drop()
    .i64_const(0).i32_const(1).i32_store(MEM2_64)
    .memory_size(MemoryID(0)).drop()
    .i64_const(1).memory_grow(MemoryID(0)).drop()
);
