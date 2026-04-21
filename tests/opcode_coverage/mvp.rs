//! Coverage for MVP (Wasm 1.0) opcodes.
//!
//! Each `opcode_test!` invocation:
//! 1. Replays the chain against an `OpRecorder` to derive the expected operator list.
//! 2. Replays the same chain against `mod_it.before()` to perform injection.
//! 3. Validates the encoded module and asserts the target function body starts with exactly
//!    the recorded operators (so a silent no-op or wrong variant fails the test).

use wasmparser::MemArg;
use wirm::ir::id::{FunctionID, GlobalID, LocalID, MemoryID, TableID, TypeID};
use wirm::ir::types::BlockType;

use crate::opcode_test;

const BASE_WAT: &str = r#"
    (module
        (type $void (func))
        (type $ii_i (func (param i32 i32) (result i32)))
        (memory $mem 1)
        (global $gi32 (mut i32) (i32.const 0))
        (global $gi64 (mut i64) (i64.const 0))
        (global $gf32 (mut f32) (f32.const 0))
        (global $gf64 (mut f64) (f64.const 0))
        (table $t 2 funcref)
        (elem (i32.const 0) $callee)
        (func $target (local i32 i64 f32 f64) nop)
        (func $callee (param i32 i32) (result i32) local.get 0))
"#;

const TARGET: u32 = 0;

const MEM0: MemArg = MemArg { align: 0, max_align: 0, offset: 0, memory: 0 };
const MEM2: MemArg = MemArg { align: 2, max_align: 2, offset: 0, memory: 0 };
const MEM3: MemArg = MemArg { align: 3, max_align: 3, offset: 0, memory: 0 };

opcode_test!(i32_arithmetic, BASE_WAT, TARGET,
    .i32_const(1).i32_const(2).i32_add().drop()
    .i32_const(5).i32_const(3).i32_sub().drop()
    .i32_const(4).i32_const(6).i32_mul().drop()
    .i32_const(10).i32_const(2).i32_div_s().drop()
    .i32_const(10).i32_const(2).i32_div_u().drop()
    .i32_const(10).i32_const(3).i32_rem_s().drop()
    .i32_const(10).i32_const(3).i32_rem_u().drop()
    .i32_const(0xff).i32_const(0x0f).i32_and().drop()
    .i32_const(0xf0).i32_const(0x0f).i32_or().drop()
    .i32_const(0xff).i32_const(0x0f).i32_xor().drop()
    .i32_const(1).i32_const(2).i32_shl().drop()
    .i32_const(-1).i32_const(2).i32_shr_s().drop()
    .i32_const(-1).i32_const(2).i32_shr_u().drop()
    .i32_const(1).i32_const(2).i32_rotl().drop()
    .i32_const(1).i32_const(2).i32_rotr().drop()
);

opcode_test!(i32_unary, BASE_WAT, TARGET,
    .i32_const(1).i32_clz().drop()
    .i32_const(1).i32_ctz().drop()
    .i32_const(1).i32_popcnt().drop()
);

opcode_test!(i32_comparisons, BASE_WAT, TARGET,
    .i32_const(0).i32_eqz().drop()
    .i32_const(1).i32_const(2).i32_eq().drop()
    .i32_const(1).i32_const(2).i32_ne().drop()
    .i32_const(1).i32_const(2).i32_lt_s().drop()
    .i32_const(1).i32_const(2).i32_lt_u().drop()
    .i32_const(1).i32_const(2).i32_gt_s().drop()
    .i32_const(1).i32_const(2).i32_gt_u().drop()
    .i32_const(1).i32_const(2).i32_le_s().drop()
    .i32_const(1).i32_const(2).i32_le_u().drop()
    .i32_const(1).i32_const(2).i32_ge_s().drop()
    .i32_const(1).i32_const(2).i32_ge_u().drop()
);

opcode_test!(i64_arithmetic, BASE_WAT, TARGET,
    .i64_const(1).i64_const(2).i64_add().drop()
    .i64_const(5).i64_const(3).i64_sub().drop()
    .i64_const(4).i64_const(6).i64_mul().drop()
    .i64_const(10).i64_const(2).i64_div_s().drop()
    .i64_const(10).i64_const(2).i64_div_u().drop()
    .i64_const(10).i64_const(3).i64_rem_s().drop()
    .i64_const(10).i64_const(3).i64_rem_u().drop()
    .i64_const(0xff).i64_const(0x0f).i64_and().drop()
    .i64_const(0xf0).i64_const(0x0f).i64_or().drop()
    .i64_const(0xff).i64_const(0x0f).i64_xor().drop()
    .i64_const(1).i64_const(2).i64_shl().drop()
    .i64_const(-1).i64_const(2).i64_shr_s().drop()
    .i64_const(-1).i64_const(2).i64_shr_u().drop()
    .i64_const(1).i64_const(2).i64_rotl().drop()
    .i64_const(1).i64_const(2).i64_rotr().drop()
);

opcode_test!(i64_unary, BASE_WAT, TARGET,
    .i64_const(1).i64_clz().drop()
    .i64_const(1).i64_ctz().drop()
    .i64_const(1).i64_popcnt().drop()
);

opcode_test!(i64_comparisons, BASE_WAT, TARGET,
    .i64_const(0).i64_eqz().drop()
    .i64_const(1).i64_const(2).i64_eq().drop()
    .i64_const(1).i64_const(2).i64_ne().drop()
    .i64_const(1).i64_const(2).i64_lt_s().drop()
    .i64_const(1).i64_const(2).i64_lt_u().drop()
    .i64_const(1).i64_const(2).i64_gt_s().drop()
    .i64_const(1).i64_const(2).i64_gt_u().drop()
    .i64_const(1).i64_const(2).i64_le_s().drop()
    .i64_const(1).i64_const(2).i64_le_u().drop()
    .i64_const(1).i64_const(2).i64_ge_s().drop()
    .i64_const(1).i64_const(2).i64_ge_u().drop()
);

opcode_test!(f32_arithmetic, BASE_WAT, TARGET,
    .f32_const(1.0).f32_const(2.0).f32_add().drop()
    .f32_const(5.0).f32_const(3.0).f32_sub().drop()
    .f32_const(4.0).f32_const(6.0).f32_mul().drop()
    .f32_const(10.0).f32_const(2.0).f32_div().drop()
    .f32_const(1.0).f32_const(2.0).f32_min().drop()
    .f32_const(1.0).f32_const(2.0).f32_max().drop()
    .f32_const(1.0).f32_const(-2.0).f32_copysign().drop()
    .f32_const(-1.0).f32_abs().drop()
    .f32_const(1.0).f32_neg().drop()
    .f32_const(1.5).f32_ceil().drop()
    .f32_const(1.5).f32_floor().drop()
    .f32_const(1.5).f32_trunc().drop()
    .f32_const(1.5).f32_nearest().drop()
    .f32_const(4.0).f32_sqrt().drop()
);

opcode_test!(f32_comparisons, BASE_WAT, TARGET,
    .f32_const(1.0).f32_const(2.0).f32_eq().drop()
    .f32_const(1.0).f32_const(2.0).f32_ne().drop()
    .f32_const(1.0).f32_const(2.0).f32_lt().drop()
    .f32_const(1.0).f32_const(2.0).f32_gt().drop()
    .f32_const(1.0).f32_const(2.0).f32_le().drop()
    .f32_const(1.0).f32_const(2.0).f32_ge().drop()
);

opcode_test!(f64_arithmetic, BASE_WAT, TARGET,
    .f64_const(1.0).f64_const(2.0).f64_add().drop()
    .f64_const(5.0).f64_const(3.0).f64_sub().drop()
    .f64_const(4.0).f64_const(6.0).f64_mul().drop()
    .f64_const(10.0).f64_const(2.0).f64_div().drop()
    .f64_const(1.0).f64_const(2.0).f64_min().drop()
    .f64_const(1.0).f64_const(2.0).f64_max().drop()
    .f64_const(1.0).f64_const(-2.0).f64_copysign().drop()
    .f64_const(-1.0).f64_abs().drop()
    .f64_const(1.0).f64_neg().drop()
    .f64_const(1.5).f64_ceil().drop()
    .f64_const(1.5).f64_floor().drop()
    .f64_const(1.5).f64_trunc().drop()
    .f64_const(1.5).f64_nearest().drop()
    .f64_const(4.0).f64_sqrt().drop()
);

opcode_test!(f64_comparisons, BASE_WAT, TARGET,
    .f64_const(1.0).f64_const(2.0).f64_eq().drop()
    .f64_const(1.0).f64_const(2.0).f64_ne().drop()
    .f64_const(1.0).f64_const(2.0).f64_lt().drop()
    .f64_const(1.0).f64_const(2.0).f64_gt().drop()
    .f64_const(1.0).f64_const(2.0).f64_le().drop()
    .f64_const(1.0).f64_const(2.0).f64_ge().drop()
);

opcode_test!(conversions, BASE_WAT, TARGET,
    .i64_const(1).i32_wrap_i64().drop()
    .i32_const(1).i64_extend_i32_s().drop()
    .i32_const(1).i64_extend_i32_u().drop()
    .f32_const(1.0).i32_trunc_f32_s().drop()
    .f32_const(1.0).i32_trunc_f32_u().drop()
    .f64_const(1.0).i32_trunc_f64_s().drop()
    .f64_const(1.0).i32_trunc_f64_u().drop()
    .f32_const(1.0).i64_trunc_f32_s().drop()
    .f32_const(1.0).i64_trunc_f32_u().drop()
    .f64_const(1.0).i64_trunc_f64_s().drop()
    .f64_const(1.0).i64_trunc_f64_u().drop()
    .i32_const(1).f32_convert_i32_s().drop()
    .i32_const(1).f32_convert_i32_u().drop()
    .i64_const(1).f32_convert_i64_s().drop()
    .i64_const(1).f32_convert_i64_u().drop()
    .i32_const(1).f64_convert_i32_s().drop()
    .i32_const(1).f64_convert_i32_u().drop()
    .i64_const(1).f64_convert_i64_s().drop()
    .i64_const(1).f64_convert_i64_u().drop()
    .f32_const(1.0).f64_promote_f32().drop()
    .f64_const(1.0).f32_demote_f64().drop()
    .i32_const(1).f32_reinterpret_i32().drop()
    .i64_const(1).f64_reinterpret_i64().drop()
    .f32_const(1.0).i32_reinterpret_f32().drop()
    .f64_const(1.0).i64_reinterpret_f64().drop()
);

opcode_test!(memory_load, BASE_WAT, TARGET,
    .i32_const(0).i32_load(MEM2).drop()
    .i32_const(0).i64_load(MEM3).drop()
    .i32_const(0).f32_load(MEM2).drop()
    .i32_const(0).f64_load(MEM3).drop()
    .i32_const(0).i32_load8_s(MEM0).drop()
    .i32_const(0).i32_load8_u(MEM0).drop()
    .i32_const(0).i32_load16_s(MEM0).drop()
    .i32_const(0).i32_load16_u(MEM0).drop()
    .i32_const(0).i64_load8_s(MEM0).drop()
    .i32_const(0).i64_load8_u(MEM0).drop()
    .i32_const(0).i64_load16_s(MEM0).drop()
    .i32_const(0).i64_load16_u(MEM0).drop()
    .i32_const(0).i64_load32_s(MEM0).drop()
    .i32_const(0).i64_load32_u(MEM0).drop()
);

opcode_test!(memory_store, BASE_WAT, TARGET,
    .i32_const(0).i32_const(1).i32_store(MEM2)
    .i32_const(0).i64_const(1).i64_store(MEM3)
    .i32_const(0).f32_const(1.0).f32_store(MEM2)
    .i32_const(0).f64_const(1.0).f64_store(MEM3)
    .i32_const(0).i32_const(1).i32_store8(MEM0)
    .i32_const(0).i32_const(1).i32_store16(MEM0)
    .i32_const(0).i64_const(1).i64_store8(MEM0)
    .i32_const(0).i64_const(1).i64_store16(MEM0)
    .i32_const(0).i64_const(1).i64_store32(MEM0)
);

opcode_test!(memory_misc, BASE_WAT, TARGET,
    .memory_size(MemoryID(0)).drop()
    .i32_const(1).memory_grow(MemoryID(0)).drop()
);

opcode_test!(local_ops, BASE_WAT, TARGET,
    .local_get(LocalID(0)).drop()
    .local_get(LocalID(1)).drop()
    .local_get(LocalID(2)).drop()
    .local_get(LocalID(3)).drop()
    .i32_const(1).local_set(LocalID(0))
    .i64_const(1).local_set(LocalID(1))
    .f32_const(1.0).local_set(LocalID(2))
    .f64_const(1.0).local_set(LocalID(3))
    .i32_const(1).local_tee(LocalID(0)).drop()
    .i64_const(1).local_tee(LocalID(1)).drop()
    .f32_const(1.0).local_tee(LocalID(2)).drop()
    .f64_const(1.0).local_tee(LocalID(3)).drop()
);

opcode_test!(global_ops, BASE_WAT, TARGET,
    .global_get(GlobalID(0)).drop()
    .global_get(GlobalID(1)).drop()
    .global_get(GlobalID(2)).drop()
    .global_get(GlobalID(3)).drop()
    .i32_const(1).global_set(GlobalID(0))
    .i64_const(1).global_set(GlobalID(1))
    .f32_const(1.0).global_set(GlobalID(2))
    .f64_const(1.0).global_set(GlobalID(3))
);

opcode_test!(parametric_ops, BASE_WAT, TARGET,
    .i32_const(1).drop()
    .i32_const(1).i32_const(2).i32_const(0).select().drop()
);

opcode_test!(call_ops, BASE_WAT, TARGET,
    .i32_const(1).i32_const(2).call(FunctionID(1)).drop()
);

opcode_test!(call_indirect_ops, BASE_WAT, TARGET,
    .i32_const(1).i32_const(2).i32_const(0).call_indirect(TypeID(1), TableID(0)).drop()
);

opcode_test!(control_flow_simple, BASE_WAT, TARGET,
    .nop()
    .block(BlockType::Empty).nop().end()
    .loop_stmt(BlockType::Empty).nop().end()
);

opcode_test!(if_else_flow, BASE_WAT, TARGET,
    .i32_const(1).if_stmt(BlockType::Empty).nop().else_stmt().nop().end()
    .i32_const(0).if_stmt(BlockType::Empty).nop().end()
);

opcode_test!(branch_flow, BASE_WAT, TARGET,
    .block(BlockType::Empty).br(0).end()
    .block(BlockType::Empty).i32_const(1).br_if(0).end()
);

opcode_test!(return_and_unreachable, BASE_WAT, TARGET,
    .return_stmt()
    .unreachable()
);
