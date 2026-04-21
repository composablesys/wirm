//! Coverage tests for the `Opcode` trait.
//!
//! Each submodule exercises the injection methods for one Wasm proposal. Tests
//! inject a stack-neutral sequence before the first instruction of a target
//! function and validate the encoded module with `wasmparser::Validator`
//! (features = all).

mod bulk_memory;
mod exceptions;
mod function_references;
mod gc;
mod memory64;
mod mvp;
mod reference_types;
mod saturating_float_to_int;
mod sign_extension;
mod simd;
mod tail_call;
mod threads;
