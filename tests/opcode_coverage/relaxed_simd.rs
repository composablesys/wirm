//! Coverage for the relaxed-simd proposal (auto-generated ops).
//!
//! All ops take v128 inputs; we synthesize them via `i{8,16,32,64}x{16,8,4,2}.splat` from
//! plain scalar consts since `wasmparser::V128` has no public constructor (see
//! `simd.rs` for the same workaround).

use crate::opcode_test;

const BASE_WAT: &str = r#"
    (module
        (memory 1)
        (func $target nop))
"#;

const TARGET: u32 = 0;

opcode_test!(relaxed_swizzle_op, BASE_WAT, TARGET,
    .i32_const(0).i8x16_splat().i32_const(0).i8x16_splat().i8x16_relaxed_swizzle().drop()
);

opcode_test!(relaxed_trunc_ops, BASE_WAT, TARGET,
    .f32_const(1.0).f32x4_splat().i32x4_relaxed_trunc_f32x4_s().drop()
    .f32_const(1.0).f32x4_splat().i32x4_relaxed_trunc_f32x4_u().drop()
    .f64_const(1.0).f64x2_splat().i32x4_relaxed_trunc_f64x2_s_zero().drop()
    .f64_const(1.0).f64x2_splat().i32x4_relaxed_trunc_f64x2_u_zero().drop()
);

opcode_test!(relaxed_madd_ops, BASE_WAT, TARGET,
    .f32_const(1.0).f32x4_splat().f32_const(2.0).f32x4_splat().f32_const(3.0).f32x4_splat()
        .f32x4_relaxed_madd().drop()
    .f32_const(1.0).f32x4_splat().f32_const(2.0).f32x4_splat().f32_const(3.0).f32x4_splat()
        .f32x4_relaxed_nmadd().drop()
    .f64_const(1.0).f64x2_splat().f64_const(2.0).f64x2_splat().f64_const(3.0).f64x2_splat()
        .f64x2_relaxed_madd().drop()
    .f64_const(1.0).f64x2_splat().f64_const(2.0).f64x2_splat().f64_const(3.0).f64x2_splat()
        .f64x2_relaxed_nmadd().drop()
);

opcode_test!(relaxed_laneselect_ops, BASE_WAT, TARGET,
    .i32_const(0).i8x16_splat().i32_const(0).i8x16_splat().i32_const(0).i8x16_splat()
        .i8x16_relaxed_laneselect().drop()
    .i32_const(0).i16x8_splat().i32_const(0).i16x8_splat().i32_const(0).i16x8_splat()
        .i16x8_relaxed_laneselect().drop()
    .i32_const(0).i32x4_splat().i32_const(0).i32x4_splat().i32_const(0).i32x4_splat()
        .i32x4_relaxed_laneselect().drop()
    .i64_const(0).i64x2_splat().i64_const(0).i64x2_splat().i64_const(0).i64x2_splat()
        .i64x2_relaxed_laneselect().drop()
);

opcode_test!(relaxed_min_max_ops, BASE_WAT, TARGET,
    .f32_const(1.0).f32x4_splat().f32_const(2.0).f32x4_splat().f32x4_relaxed_min().drop()
    .f32_const(1.0).f32x4_splat().f32_const(2.0).f32x4_splat().f32x4_relaxed_max().drop()
    .f64_const(1.0).f64x2_splat().f64_const(2.0).f64x2_splat().f64x2_relaxed_min().drop()
    .f64_const(1.0).f64x2_splat().f64_const(2.0).f64x2_splat().f64x2_relaxed_max().drop()
);

opcode_test!(relaxed_integer_ops, BASE_WAT, TARGET,
    .i32_const(0).i16x8_splat().i32_const(0).i16x8_splat().i16x8_relaxed_q15mulr_s().drop()
    .i32_const(0).i8x16_splat().i32_const(0).i8x16_splat().i16x8_relaxed_dot_i8x16_i7x16_s().drop()
    .i32_const(0).i8x16_splat().i32_const(0).i8x16_splat().i32_const(0).i32x4_splat()
        .i32x4_relaxed_dot_i8x16_i7x16_add_s().drop()
);
