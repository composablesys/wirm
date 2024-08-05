//! Contructs a wasm module from scratch with orca
//! from https://github.com/rustwasm/walrus/blob/main/examples/build-wasm-from-scratch.rs
//!
//! You can run this wasm file with `fact.js`

use orca::ir::function::FunctionBuilder;
use orca::ir::module::*;
use orca::ir::types::*;
use orca::opcode::Opcode;

fn main() {
    let mut module = Module::new();
    let log_type_id = module.add_type(&[DataType::I32], &[]);
    let log_func_id = module.add_import_func("env".to_string(), "log".to_string(), log_type_id);

    let mut factorial = FunctionBuilder::new(&[DataType::I32], &[DataType::I32]);

    // Create our parameter and our two locals.
    let n = 0u32;
    let i = factorial.add_local(DataType::I32);
    let res = factorial.add_local(DataType::I32);

    #[rustfmt::skip]
    factorial
        // (local.set $i (local.get $n))
        .local_get(n)
        .local_set(i)
        // (local.set $res (i32.const 1))
        .i32_const(1)
        .local_set(res)
            .block(BlockType::Empty) // label 1
                .loop_stmt(BlockType::Empty) // label 2
                    // (call $log (local.get $res))
                    .local_get(res)
                    .call(log_func_id)
                    // (i32.eq (local.get $i) (i32.const 0))
                    .local_get(i)
                    .i32_const(0)
                    .i32_eq()
                    .if_stmt(BlockType::Empty)
                        // (then (br to outside block @1))
                        .br(2)
                    .else_stmt()
                        // (local.set $res (i32.mul (local.get $i) (local.get $res)))
                        .local_get(i)
                        .local_get(res)
                        .i32_mul()
                        .local_set(res)
                        // (local.set $i (i32.sub (local.get $i) (i32.const 1))))
                        .local_get(i)
                        .i32_const(1)
                        .i32_sub()
                        .local_set(i)
                    .end()
                    .br(0) // back to loop @2
                .end()
            .end()
        .local_get(res);

    let fact_id = factorial.finish_module(&mut module);

    // Export the `factorial` function.
    module.add_export_func("factorial".to_string(), fact_id);

    module.emit_wasm("target/out.wasm").unwrap();

    // debug use: print the wat file
    // let bytes = module.encode_only_module();
    // let wat = wasmprinter::print_bytes(&bytes).unwrap();
    // println!("{}", wat);
}
