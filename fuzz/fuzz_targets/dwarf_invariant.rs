//! Coverage-guided fuzz of the DWARF rewriter against the parse-aside seeds.
//!
//! Companion to the in-crate proptest at
//! `src/ir/module/test.rs::rewriter_preserves_source_location_under_random_injection`.
//! The proptest does the *strong* invariant check
//! (`lookup(new_pc) == lookup(anchor_orig_pc)`) on 256 random plans per
//! `cargo test`; that check needs `pub(crate)` access to `DwarfEncodeMaps`
//! which the fuzz crate can't reach across the crate boundary.
//!
//! This target trades the strong check for *broader* exploration. The
//! `Plan.fixture` byte selects between the single-function (`add.wasm`) and
//! multi-function (`two_funcs.wasm`) seeds; libfuzzer generates the
//! instrumentation actions. Each plan is applied, the module is re-encoded,
//! and the output is checked against three weaker invariants that still catch
//! real regressions:
//!
//! 1. The encoded output validates under all wasmparser features.
//! 2. Gimli successfully re-parses the rewritten `.debug_line`.
//! 3. Every `(line, column)` pair the input carried still appears in the
//!    output's line program (no source mapping was silently dropped).
//!
//! Plans that exercise paths the proptest hasn't seen accumulate in the
//! libfuzzer corpus and stay around across runs.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeSet;
use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
use wirm::iterator::module_iterator::ModuleIterator;
use wirm::Opcode;

/// Per-op instrumentation action. `BeforeNops` / `AfterNops` accept zero (a
/// no-op); `AltNops(0)` would replace the orig op with nothing, which the
/// applier promotes to one nop so we never produce an empty alternate.
#[derive(Debug, Clone, Arbitrary)]
enum Action {
    Skip,
    BeforeNops(u8),
    AfterNops(u8),
    AltNops(u8),
}

/// One arbitrary byte chooses the fixture by its low bit: even → `add.wasm`
/// (single function), odd → `two_funcs.wasm` (multi-function). Splitting 50/50
/// (rather than the natural-looking `fixture == 0` test) keeps both code paths
/// exercised across the corpus instead of skewing ~1/256 to single-function.
#[derive(Debug, Arbitrary)]
struct Plan {
    fixture: u8,
    actions: Vec<Action>,
}

const ADD_FIXTURE: &[u8] =
    include_bytes!("../../tests/test_inputs/handwritten/dwarf/add.wasm");
const TWO_FUNCS_FIXTURE: &[u8] =
    include_bytes!("../../tests/test_inputs/handwritten/dwarf/two_funcs.wasm");

fuzz_target!(|plan: Plan| {
    let fixture: &[u8] = if plan.fixture & 1 == 0 {
        ADD_FIXTURE
    } else {
        TWO_FUNCS_FIXTURE
    };

    let mut module = match wirm::Module::parse(fixture, false, false, true) {
        Ok(m) => m,
        // Fixtures are committed and known-parseable, but keep this defensive
        // in case a future wasmparser bump rejects them.
        Err(_) => return,
    };

    {
        let mut it = ModuleIterator::new(&mut module, &vec![]);
        let mut idx = 0usize;
        loop {
            if it.curr_op().is_some() {
                let action = plan.actions.get(idx).cloned().unwrap_or(Action::Skip);
                apply_action(&mut it, action);
                idx += 1;
            }
            if it.next().is_none() {
                break;
            }
        }
    }

    let encoded = module
        .encode()
        .expect("encode of instrumented DWARF fixture should succeed");

    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&encoded)
        .expect("rewritten DWARF module failed wasmparser validation");

    let in_locs = collect_line_col_pairs(fixture);
    let out_locs = collect_line_col_pairs(&encoded);

    for loc in &in_locs {
        assert!(
            out_locs.contains(loc),
            "input (line, col) {loc:?} missing from output's .debug_line \
             after instrumentation",
        );
    }
});

/// Apply a single action. Clamps counts to a small upper bound so a wildly
/// large plan doesn't blow up the body size (we want lots of distinct cases
/// per second, not one giant case per minute).
fn apply_action(it: &mut ModuleIterator<'_, '_>, action: Action) {
    const MAX_PER_OP: u8 = 8;
    match action {
        Action::Skip => {}
        Action::BeforeNops(n) => {
            for _ in 0..n.min(MAX_PER_OP) {
                it.before().nop();
            }
        }
        Action::AfterNops(n) => {
            for _ in 0..n.min(MAX_PER_OP) {
                it.after().nop();
            }
        }
        Action::AltNops(n) => {
            // An empty alternate would delete the orig op; promote 0 → 1.
            let n = n.max(1).min(MAX_PER_OP);
            for _ in 0..n {
                it.alternate().nop();
            }
        }
    }
}

/// Pulls `.debug_line` out of `wasm`, parses it with gimli, and returns the
/// set of `(line, column)` pairs from non-end-of-sequence rows. Used to
/// check the weak "no source mapping dropped" invariant. Errors during
/// parse return an empty set, which would cause the input-subset check to
/// fail loudly for any non-trivial input.
fn collect_line_col_pairs(wasm: &[u8]) -> BTreeSet<(u64, u64)> {
    use wasmparser::{Parser, Payload};

    let mut line_bytes: Vec<u8> = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Ok(Payload::CustomSection(cs)) = payload {
            if cs.name() == ".debug_line" {
                line_bytes = cs.data().to_vec();
                break;
            }
        }
    }

    let endian = gimli::LittleEndian;
    let dl = gimli::read::DebugLine::new(&line_bytes, endian);
    let program = match dl.program(gimli::DebugLineOffset(0), 4, None, None) {
        Ok(p) => p,
        Err(_) => return BTreeSet::new(),
    };

    let mut rows = program.rows();
    let mut out = BTreeSet::new();
    while let Ok(Some((_header, row))) = rows.next_row() {
        if row.end_sequence() {
            continue;
        }
        let line = row.line().map(|n| n.get()).unwrap_or(0);
        let col = match row.column() {
            gimli::ColumnType::LeftEdge => 0,
            gimli::ColumnType::Column(c) => c.get(),
        };
        out.insert((line, col));
    }
    out
}
