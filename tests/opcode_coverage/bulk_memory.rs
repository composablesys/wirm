//! Coverage for the bulk-memory proposal.

use wirm::ir::id::{DataSegmentID, ElementID, MemoryID, TableID};

use crate::opcode_test;

const BASE_WAT: &str = r#"
    (module
        (memory 1)
        (table $t 2 funcref)
        (func $target
            i32.const 0 i32.const 0 i32.const 0 memory.init 0
            data.drop 0)
        (func $f)
        (data $d "\00\01\02\03")
        (elem $e funcref (ref.func $f)))
"#;

const TARGET: u32 = 0;

opcode_test!(memory_bulk, BASE_WAT, TARGET,
    .i32_const(0).i32_const(0).i32_const(0).memory_init(DataSegmentID(0), MemoryID(0))
    .i32_const(0).i32_const(0).i32_const(0).memory_copy(MemoryID(0), MemoryID(0))
    .i32_const(0).i32_const(0).i32_const(0).memory_fill(MemoryID(0))
    .data_drop(DataSegmentID(0))
);

opcode_test!(table_bulk, BASE_WAT, TARGET,
    .i32_const(0).i32_const(0).i32_const(0).table_init(ElementID(0), TableID(0))
    .i32_const(0).i32_const(0).i32_const(0).table_copy(TableID(0), TableID(0))
    .elem_drop(ElementID(0))
);
