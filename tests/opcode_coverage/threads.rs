//! Coverage for the threads / atomics proposal.

use crate::opcode_test;
use wasmparser::MemArg;

const BASE_WAT: &str = r#"
    (module
        (memory 1 1 shared)
        (func $target nop))
"#;

const TARGET: u32 = 0;

const A0: MemArg = MemArg {
    align: 0,
    max_align: 0,
    offset: 0,
    memory: 0,
};
const A1: MemArg = MemArg {
    align: 1,
    max_align: 1,
    offset: 0,
    memory: 0,
};
const A2: MemArg = MemArg {
    align: 2,
    max_align: 2,
    offset: 0,
    memory: 0,
};
const A3: MemArg = MemArg {
    align: 3,
    max_align: 3,
    offset: 0,
    memory: 0,
};

opcode_test!(atomic_load, BASE_WAT, TARGET,
    .i32_const(0).i32_atomic_load(A2).drop()
    .i32_const(0).i64_atomic_load(A3).drop()
    .i32_const(0).i32_atomic_load8_u(A0).drop()
    .i32_const(0).i32_atomic_load16_u(A1).drop()
    .i32_const(0).i64_atomic_load8_u(A0).drop()
    .i32_const(0).i64_atomic_load16_u(A1).drop()
    .i32_const(0).i64_atomic_load32_u(A2).drop()
);

opcode_test!(atomic_store, BASE_WAT, TARGET,
    .i32_const(0).i32_const(1).i32_atomic_store(A2)
    .i32_const(0).i64_const(1).i64_atomic_store(A3)
    .i32_const(0).i32_const(1).i32_atomic_store8(A0)
    .i32_const(0).i32_const(1).i32_atomic_store16(A1)
    .i32_const(0).i64_const(1).i64_atomic_store8(A0)
    .i32_const(0).i64_const(1).i64_atomic_store16(A1)
    .i32_const(0).i64_const(1).i64_atomic_store32(A2)
);

opcode_test!(atomic_rmw_i32, BASE_WAT, TARGET,
    .i32_const(0).i32_const(1).i32_atomic_rmw_add(A2).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw_sub(A2).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw_and(A2).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw_or(A2).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw_xor(A2).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw_xchg(A2).drop()
    .i32_const(0).i32_const(1).i32_const(2).i32_atomic_rmw_cmpxchg(A2).drop()
);

opcode_test!(atomic_rmw_i32_narrow, BASE_WAT, TARGET,
    .i32_const(0).i32_const(1).i32_atomic_rmw8_add_u(A0).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw8_sub_u(A0).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw8_and_u(A0).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw8_or_u(A0).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw8_xor_u(A0).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw8_xchg_u(A0).drop()
    .i32_const(0).i32_const(1).i32_const(2).i32_atomic_rmw8_cmpxchg_u(A0).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw16_add_u(A1).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw16_sub_u(A1).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw16_and_u(A1).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw16_or_u(A1).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw16_xor_u(A1).drop()
    .i32_const(0).i32_const(1).i32_atomic_rmw16_xchg_u(A1).drop()
    .i32_const(0).i32_const(1).i32_const(2).i32_atomic_rmw16_cmpxchg_u(A1).drop()
);

opcode_test!(atomic_rmw_i64, BASE_WAT, TARGET,
    .i32_const(0).i64_const(1).i64_atomic_rmw_add(A3).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw_sub(A3).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw_and(A3).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw_or(A3).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw_xor(A3).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw_xchg(A3).drop()
    .i32_const(0).i64_const(1).i64_const(2).i64_atomic_rmw_cmpxchg(A3).drop()
);

opcode_test!(atomic_rmw_i64_narrow, BASE_WAT, TARGET,
    .i32_const(0).i64_const(1).i64_atomic_rmw8_add_u(A0).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw8_sub_u(A0).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw8_and_u(A0).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw8_or_u(A0).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw8_xor_u(A0).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw8_xchg_u(A0).drop()
    .i32_const(0).i64_const(1).i64_const(2).i64_atomic_rmw8_cmpxchg_u(A0).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw16_add_u(A1).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw16_sub_u(A1).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw16_and_u(A1).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw16_or_u(A1).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw16_xor_u(A1).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw16_xchg_u(A1).drop()
    .i32_const(0).i64_const(1).i64_const(2).i64_atomic_rmw16_cmpxchg_u(A1).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw32_add_u(A2).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw32_sub_u(A2).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw32_and_u(A2).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw32_or_u(A2).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw32_xor_u(A2).drop()
    .i32_const(0).i64_const(1).i64_atomic_rmw32_xchg_u(A2).drop()
    .i32_const(0).i64_const(1).i64_const(2).i64_atomic_rmw32_cmpxchg_u(A2).drop()
);

opcode_test!(atomic_misc, BASE_WAT, TARGET,
    .i32_const(0).i32_const(0).memory_atomic_notify(A2).drop()
    .i32_const(0).i32_const(0).i64_const(0).memory_atomic_wait32(A2).drop()
    .i32_const(0).i64_const(0).i64_const(0).memory_atomic_wait64(A3).drop()
    .atomic_fence()
);
