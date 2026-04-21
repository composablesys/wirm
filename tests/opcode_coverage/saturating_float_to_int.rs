//! Coverage for the nontrapping-float-to-int conversions proposal.

use crate::opcode_test;

const BASE_WAT: &str = r#"
    (module
        (func $target (local i32 i64 f32 f64) nop))
"#;

const TARGET: u32 = 0;

opcode_test!(sat_trunc_ops, BASE_WAT, TARGET,
    .f32_const(1.0).i32_trunc_sat_f32_s().drop()
    .f32_const(1.0).i32_trunc_sat_f32_u().drop()
    .f64_const(1.0).i32_trunc_sat_f64_s().drop()
    .f64_const(1.0).i32_trunc_sat_f64_u().drop()
    .f32_const(1.0).i64_trunc_sat_f32_s().drop()
    .f32_const(1.0).i64_trunc_sat_f32_u().drop()
    .f64_const(1.0).i64_trunc_sat_f64_s().drop()
    .f64_const(1.0).i64_trunc_sat_f64_u().drop()
);
