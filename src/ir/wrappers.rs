//! Wrapper functions
//!
//! `refers_to_*` and `update_*_instr` are generated from
//! `wasmparser::for_each_operator!`. Each operator is classified by the field
//! names of its argument struct:
//!
//!   * `function_index`                           → function category
//!   * `global_index`                             → global category
//!   * `memarg` / `mem` / `src_mem` (+ `dst_mem`) → memory category
//!
//! New opcodes added upstream are routed automatically; a field rename in
//! wasmparser would surface as a compile-time mismatch elsewhere rather than
//! silently miss classifications here.

use crate::error::Error::InstrumentationError;
use crate::ir::types;
use std::collections::HashMap;
use wasmparser::Operator;

pub fn indirect_namemap_parser2encoder(
    namemap: wasmparser::IndirectNameMap,
) -> wasm_encoder::IndirectNameMap {
    let mut names = wasm_encoder::IndirectNameMap::new();
    for name in namemap {
        let naming = name.unwrap();
        names.append(naming.index, &namemap_parser2encoder(naming.names));
    }
    names
}

pub fn namemap_parser2encoder(namemap: wasmparser::NameMap) -> wasm_encoder::NameMap {
    let mut names = wasm_encoder::NameMap::new();
    for name in namemap {
        let naming = name.unwrap();
        names.append(naming.index, naming.name);
    }
    names
}

// ── refers_to_* / update_*_instr generators ──────────────────────────────────
//
// Each macro receives the full operator list from `for_each_operator!` and
// expands to the entire `pub(crate) fn`. The fn body is a sequence of
// independent `if let` / `if matches!()` statements with early returns
// (rather than a single `match`). Generating arms via recursive macro calls
// inside `match { ... }` runs into the parser's pattern-macro restriction —
// expansions there must be a single pattern, not a complete `pat => body,`.
//
// `op` (and `mapping` for the update generators) is threaded through the
// recursive `@stmt` / `@scan` rules as a metavariable rather than as a free
// identifier. Each recursive expansion sits in a fresh syntax context, so a
// bare reference to `op` inside an inner template doesn't see the outer fn
// parameter; passing it as `$op_expr:ident` carries the original token (and
// its hygiene context) through unchanged.
//
// For each operator, the `@scan` rules pattern-match the bracketed arg list:
// literal idents (`memarg`, `mem`, `function_index`, ...) match only that
// exact field name; the catch-all `$_first:ident` arm peels one ident and
// recurses; `[]` falls through emitting nothing. Operators whose arg struct
// contains none of the target field names contribute no statement.

macro_rules! refers_to_func_match {
    ($( @$proposal:ident $op:ident $({ $($arg:ident: $argty:ty),* })? => $visit:ident ($($ann:tt)*))*) => {
        pub(crate) fn refers_to_func(op: &Operator) -> bool {
            $( refers_to_func_match!(@stmt op, $op $({ $($arg)* })?); )*
            false
        }
    };
    (@stmt $op_expr:ident, $op_name:ident) => {};
    (@stmt $op_expr:ident, $op_name:ident { $($args:ident)* }) => {
        refers_to_func_match!(@scan $op_expr, $op_name [$($args)*])
    };
    (@scan $op_expr:ident, $op_name:ident []) => {};
    (@scan $op_expr:ident, $op_name:ident [function_index $($_rest:ident)*]) => {
        if matches!($op_expr, Operator::$op_name { .. }) { return true; }
    };
    (@scan $op_expr:ident, $op_name:ident [$_first:ident $($rest:ident)*]) => {
        refers_to_func_match!(@scan $op_expr, $op_name [$($rest)*])
    };
}
wasmparser::for_each_operator!(refers_to_func_match);

macro_rules! refers_to_global_match {
    ($( @$proposal:ident $op:ident $({ $($arg:ident: $argty:ty),* })? => $visit:ident ($($ann:tt)*))*) => {
        pub(crate) fn refers_to_global(op: &Operator) -> bool {
            $( refers_to_global_match!(@stmt op, $op $({ $($arg)* })?); )*
            false
        }
    };
    (@stmt $op_expr:ident, $op_name:ident) => {};
    (@stmt $op_expr:ident, $op_name:ident { $($args:ident)* }) => {
        refers_to_global_match!(@scan $op_expr, $op_name [$($args)*])
    };
    (@scan $op_expr:ident, $op_name:ident []) => {};
    (@scan $op_expr:ident, $op_name:ident [global_index $($_rest:ident)*]) => {
        if matches!($op_expr, Operator::$op_name { .. }) { return true; }
    };
    (@scan $op_expr:ident, $op_name:ident [$_first:ident $($rest:ident)*]) => {
        refers_to_global_match!(@scan $op_expr, $op_name [$($rest)*])
    };
}
wasmparser::for_each_operator!(refers_to_global_match);

macro_rules! refers_to_memory_match {
    ($( @$proposal:ident $op:ident $({ $($arg:ident: $argty:ty),* })? => $visit:ident ($($ann:tt)*))*) => {
        pub(crate) fn refers_to_memory(op: &Operator) -> bool {
            $( refers_to_memory_match!(@stmt op, $op $({ $($arg)* })?); )*
            false
        }
    };
    (@stmt $op_expr:ident, $op_name:ident) => {};
    (@stmt $op_expr:ident, $op_name:ident { $($args:ident)* }) => {
        refers_to_memory_match!(@scan $op_expr, $op_name [$($args)*])
    };
    (@scan $op_expr:ident, $op_name:ident []) => {};
    (@scan $op_expr:ident, $op_name:ident [memarg  $($_rest:ident)*]) => {
        if matches!($op_expr, Operator::$op_name { .. }) { return true; }
    };
    (@scan $op_expr:ident, $op_name:ident [mem     $($_rest:ident)*]) => {
        if matches!($op_expr, Operator::$op_name { .. }) { return true; }
    };
    (@scan $op_expr:ident, $op_name:ident [src_mem $($_rest:ident)*]) => {
        if matches!($op_expr, Operator::$op_name { .. }) { return true; }
    };
    (@scan $op_expr:ident, $op_name:ident [$_first:ident $($rest:ident)*]) => {
        refers_to_memory_match!(@scan $op_expr, $op_name [$($rest)*])
    };
}
wasmparser::for_each_operator!(refers_to_memory_match);

macro_rules! update_fn_match {
    ($( @$proposal:ident $op:ident $({ $($arg:ident: $argty:ty),* })? => $visit:ident ($($ann:tt)*))*) => {
        pub(crate) fn update_fn_instr(op: &mut Operator, mapping: &HashMap<u32, u32>) -> types::Result<()> {
            $( update_fn_match!(@stmt op, mapping, $op $({ $($arg)* })?); )*
            panic!("Internal error: Operation doesn't need to be checked for function IDs!")
        }
    };
    (@stmt $op_expr:ident, $mapping:ident, $op_name:ident) => {};
    (@stmt $op_expr:ident, $mapping:ident, $op_name:ident { $($args:ident)* }) => {
        update_fn_match!(@scan $op_expr, $mapping, $op_name [$($args)*])
    };
    (@scan $op_expr:ident, $mapping:ident, $op_name:ident []) => {};
    (@scan $op_expr:ident, $mapping:ident, $op_name:ident [function_index $($_rest:ident)*]) => {
        if let Operator::$op_name { function_index, .. } = $op_expr {
            match $mapping.get(&*function_index) {
                Some(new_index) => *function_index = *new_index,
                None => return Err(InstrumentationError("Called a deleted function!".to_string())),
            }
            return Ok(());
        }
    };
    (@scan $op_expr:ident, $mapping:ident, $op_name:ident [$_first:ident $($rest:ident)*]) => {
        update_fn_match!(@scan $op_expr, $mapping, $op_name [$($rest)*])
    };
}
wasmparser::for_each_operator!(update_fn_match);

macro_rules! update_global_match {
    ($( @$proposal:ident $op:ident $({ $($arg:ident: $argty:ty),* })? => $visit:ident ($($ann:tt)*))*) => {
        pub(crate) fn update_global_instr(
            op: &mut Operator,
            mapping: &HashMap<u32, u32>,
        ) -> types::Result<()> {
            $( update_global_match!(@stmt op, mapping, $op $({ $($arg)* })?); )*
            panic!("Internal error: Operation doesn't need to be checked for global IDs!")
        }
    };
    (@stmt $op_expr:ident, $mapping:ident, $op_name:ident) => {};
    (@stmt $op_expr:ident, $mapping:ident, $op_name:ident { $($args:ident)* }) => {
        update_global_match!(@scan $op_expr, $mapping, $op_name [$($args)*])
    };
    (@scan $op_expr:ident, $mapping:ident, $op_name:ident []) => {};
    (@scan $op_expr:ident, $mapping:ident, $op_name:ident [global_index $($_rest:ident)*]) => {
        if let Operator::$op_name { global_index, .. } = $op_expr {
            match $mapping.get(&*global_index) {
                Some(new_index) => *global_index = *new_index,
                None => return Err(InstrumentationError("Operation on a deleted global!".to_string())),
            }
            return Ok(());
        }
    };
    (@scan $op_expr:ident, $mapping:ident, $op_name:ident [$_first:ident $($rest:ident)*]) => {
        update_global_match!(@scan $op_expr, $mapping, $op_name [$($rest)*])
    };
}
wasmparser::for_each_operator!(update_global_match);

macro_rules! update_memory_match {
    ($( @$proposal:ident $op:ident $({ $($arg:ident: $argty:ty),* })? => $visit:ident ($($ann:tt)*))*) => {
        pub(crate) fn update_memory_instr(
            op: &mut Operator,
            mapping: &HashMap<u32, u32>,
        ) -> types::Result<()> {
            $( update_memory_match!(@stmt op, mapping, $op $({ $($arg)* })?); )*
            panic!("Internal error: Operation doesn't need to be checked for memory IDs!")
        }
    };
    (@stmt $op_expr:ident, $mapping:ident, $op_name:ident) => {};
    (@stmt $op_expr:ident, $mapping:ident, $op_name:ident { $($args:ident)* }) => {
        update_memory_match!(@scan $op_expr, $mapping, $op_name [$($args)*])
    };
    (@scan $op_expr:ident, $mapping:ident, $op_name:ident []) => {};
    // memarg-bearing ops (loads, stores, atomic rmw, etc.) — remap memarg.memory.
    (@scan $op_expr:ident, $mapping:ident, $op_name:ident [memarg $($_rest:ident)*]) => {
        if let Operator::$op_name { memarg, .. } = $op_expr {
            match $mapping.get(&memarg.memory) {
                Some(new_index) => memarg.memory = *new_index,
                None => return Err(InstrumentationError(format!(
                    "Attempting to reference a deleted memory, ID: {}", memarg.memory
                ))),
            }
            return Ok(());
        }
    };
    // Bulk-memory ops with a single `mem` field (size, grow, fill, init, discard).
    (@scan $op_expr:ident, $mapping:ident, $op_name:ident [mem $($_rest:ident)*]) => {
        if let Operator::$op_name { mem, .. } = $op_expr {
            match $mapping.get(&*mem) {
                Some(new_index) => *mem = *new_index,
                None => return Err(InstrumentationError(format!(
                    "Attempting to reference a deleted memory, ID: {}", mem
                ))),
            }
            return Ok(());
        }
    };
    // MemoryCopy: both src_mem and dst_mem must be remapped.
    (@scan $op_expr:ident, $mapping:ident, $op_name:ident [src_mem $($_rest:ident)*]) => {
        if let Operator::$op_name { src_mem, dst_mem, .. } = $op_expr {
            match $mapping.get(&*src_mem) {
                Some(new_index) => *src_mem = *new_index,
                None => return Err(InstrumentationError(format!(
                    "Attempting to reference a deleted memory, ID: {}", src_mem
                ))),
            }
            match $mapping.get(&*dst_mem) {
                Some(new_index) => *dst_mem = *new_index,
                None => return Err(InstrumentationError(format!(
                    "Attempting to reference a deleted memory, ID: {}", dst_mem
                ))),
            }
            return Ok(());
        }
    };
    (@scan $op_expr:ident, $mapping:ident, $op_name:ident [$_first:ident $($rest:ident)*]) => {
        update_memory_match!(@scan $op_expr, $mapping, $op_name [$($rest)*])
    };
}
wasmparser::for_each_operator!(update_memory_match);
