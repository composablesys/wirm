//! Shared helpers for the wirm-fuzz targets.

use wasmparser::{Operator, Parser, Payload};

/// Total number of operators across every function body in a core wasm
/// module or in every nested core module of a component. Used by the
/// instrumentation targets to verify that injected ops survive encode.
pub fn count_ops(bytes: &[u8]) -> usize {
    let mut count = 0usize;
    for payload in Parser::new(0).parse_all(bytes) {
        if let Ok(Payload::CodeSectionEntry(body)) = payload {
            if let Ok(mut reader) = body.get_operators_reader() {
                while !reader.eof() {
                    if reader.read().is_err() {
                        break;
                    }
                    count += 1;
                }
            }
        }
    }
    count
}

/// Walks the encoded module `bytes` and panics if any operator references
/// one of the supplied "forbidden" indices for its kind.
///
/// The intended setup: caller adds an imported function/global/memory before
/// re-encoding. Imports append, so each new import lands at index =
/// `original_imported_count_of_kind` and any local entries shift up by one.
/// No operator that existed in the source bytes knew about the new import,
/// so post-reindex none should reference it. If reindex skipped an op, that
/// op would still point at its original local index — which is now the
/// freshly-added import.
///
/// This check is intentionally **independent** of wirm's own classification
/// (`refers_to_*` / `update_*_instr` in `ir/module/reindex.rs`): if both
/// shared a missed operator the fuzz target would mask the corresponding
/// bug. Both happen to draw the same field-name → category mapping from
/// `wasmparser::for_each_operator!`, so they stay in sync with wasmparser
/// independently rather than via shared macro definitions.
pub fn assert_no_reference_to(
    bytes: &[u8],
    forbidden_func: u32,
    forbidden_global: u32,
    forbidden_mem: u32,
) {
    for payload in Parser::new(0).parse_all(bytes) {
        if let Ok(Payload::CodeSectionEntry(body)) = payload {
            if let Ok(mut reader) = body.get_operators_reader() {
                while !reader.eof() {
                    let op = match reader.read() {
                        Ok(o) => o,
                        Err(_) => break,
                    };
                    check_op(&op, forbidden_func, forbidden_global, forbidden_mem);
                }
            }
        }
    }
}

// Macro-driven exhaustive checker. For each operator that has a
// function/global/memory index field, emit an `if let` that asserts the
// referenced index is not the just-added "forbidden" one for that kind.
// See `src/ir/module/reindex.rs` for the same generation shape and the
// rationale for sequential `if let`s instead of recursive match arms.
macro_rules! check_op_match {
    ($( @$proposal:ident $op:ident $({ $($arg:ident: $argty:ty),* })? => $visit:ident ($($ann:tt)*))*) => {
        fn check_op(op: &Operator<'_>, forbidden_func: u32, forbidden_global: u32, forbidden_mem: u32) {
            $( check_op_match!(@stmt op, forbidden_func, forbidden_global, forbidden_mem, $op $({ $($arg)* })?); )*
        }
    };
    (@stmt $op:ident, $ff:ident, $fg:ident, $fm:ident, $op_name:ident) => {};
    (@stmt $op:ident, $ff:ident, $fg:ident, $fm:ident, $op_name:ident { $($args:ident)* }) => {
        check_op_match!(@scan $op, $ff, $fg, $fm, $op_name [$($args)*])
    };
    (@scan $op:ident, $ff:ident, $fg:ident, $fm:ident, $op_name:ident []) => {};
    (@scan $op:ident, $ff:ident, $fg:ident, $fm:ident, $op_name:ident [function_index $($_rest:ident)*]) => {
        if let Operator::$op_name { function_index, .. } = $op {
            assert_ne!(
                *function_index, $ff,
                "operator references the just-added imported function (idx {}); reindex appears to have skipped this op",
                $ff,
            );
            return;
        }
    };
    (@scan $op:ident, $ff:ident, $fg:ident, $fm:ident, $op_name:ident [global_index $($_rest:ident)*]) => {
        if let Operator::$op_name { global_index, .. } = $op {
            assert_ne!(
                *global_index, $fg,
                "operator references the just-added imported global (idx {}); reindex appears to have skipped this op",
                $fg,
            );
            return;
        }
    };
    (@scan $op:ident, $ff:ident, $fg:ident, $fm:ident, $op_name:ident [memarg $($_rest:ident)*]) => {
        if let Operator::$op_name { memarg, .. } = $op {
            assert_ne!(
                memarg.memory, $fm,
                "operator references the just-added imported memory (idx {}); reindex appears to have skipped this op",
                $fm,
            );
            return;
        }
    };
    (@scan $op:ident, $ff:ident, $fg:ident, $fm:ident, $op_name:ident [mem $($_rest:ident)*]) => {
        if let Operator::$op_name { mem, .. } = $op {
            assert_ne!(
                *mem, $fm,
                "operator references the just-added imported memory (idx {}); reindex appears to have skipped this op",
                $fm,
            );
            return;
        }
    };
    (@scan $op:ident, $ff:ident, $fg:ident, $fm:ident, $op_name:ident [src_mem $($_rest:ident)*]) => {
        if let Operator::$op_name { src_mem, dst_mem, .. } = $op {
            assert_ne!(
                *src_mem, $fm,
                "operator references the just-added imported memory (idx {}) as src_mem; reindex skipped this op",
                $fm,
            );
            assert_ne!(
                *dst_mem, $fm,
                "operator references the just-added imported memory (idx {}) as dst_mem; reindex skipped this op",
                $fm,
            );
            return;
        }
    };
    (@scan $op:ident, $ff:ident, $fg:ident, $fm:ident, $op_name:ident [$_first:ident $($rest:ident)*]) => {
        check_op_match!(@scan $op, $ff, $fg, $fm, $op_name [$($rest)*])
    };
}
wasmparser::for_each_operator!(check_op_match);
