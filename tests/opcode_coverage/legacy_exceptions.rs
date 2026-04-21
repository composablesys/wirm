//! Coverage for the legacy exceptions proposal (auto-generated subset).
//!
//! **Why these tests don't use `opcode_test!`:** `catch`, `rethrow`, `delegate`, and
//! `catch_all` are block-structure markers that only validate when nested inside a
//! legacy `try` block. Wirm's `Opcode` trait skips `try` (manual impl required —
//! `BlockType` argument; see `src/opcode.rs:207`), so we can't synthesize the
//! enclosing `try` needed to produce a validatable module from injection alone.
//!
//! Instead, we record each op against an `OpRecorder` and assert the correct
//! `Operator` variant is emitted. This covers:
//!   - the method compiles (the `Opcode` trait method exists),
//!   - the method produces the expected `wasmparser::Operator` variant.
//!
//! Full round-trip coverage (emit + validate) unblocks once wirm grows a manual
//! `try()` helper.

use wasmparser::Operator;
use wirm::Opcode as _;

use crate::common::OpRecorder;

#[test]
fn legacy_catch_op() {
    let mut rec = OpRecorder::new();
    rec.catch(7);
    let ops = rec.finish();
    assert_eq!(ops.len(), 1);
    assert!(
        matches!(ops[0], Operator::Catch { tag_index: 7 }),
        "expected Catch {{ tag_index: 7 }}, got {:?}",
        ops[0],
    );
}

#[test]
fn legacy_rethrow_op() {
    let mut rec = OpRecorder::new();
    rec.rethrow(2);
    let ops = rec.finish();
    assert!(
        matches!(ops[0], Operator::Rethrow { relative_depth: 2 }),
        "got {:?}",
        ops[0],
    );
}

#[test]
fn legacy_delegate_op() {
    let mut rec = OpRecorder::new();
    rec.delegate(1);
    let ops = rec.finish();
    assert!(
        matches!(ops[0], Operator::Delegate { relative_depth: 1 }),
        "got {:?}",
        ops[0],
    );
}

#[test]
fn legacy_catch_all_op() {
    let mut rec = OpRecorder::new();
    rec.catch_all();
    let ops = rec.finish();
    assert!(matches!(ops[0], Operator::CatchAll), "got {:?}", ops[0]);
}
