//! Shared helper for walking spec-test-style `.wast` files.
//!
//! Lives here so integration tests can `use common::wast_iter::…` and the
//! in-crate lib test in `src/ir/component/visitor/tests.rs` can reach in
//! with `#[path = "…/tests/common/wast_iter.rs"]`. Keeping it here (rather
//! than under `src/`) keeps test-only code out of the library tree.

use std::fs;
use std::path::Path;

/// Parse a `.wast` file and feed each valid top-level module/component's
/// encoded bytes to `f` as `(label, bytes)`. Assert-style directives
/// (`assert_malformed`, `assert_invalid`, `assert_return`, invokes, thread
/// primitives, …) are skipped — wirm's tests only care about the happy-path
/// modules that a compliant toolchain would decode cleanly.
///
/// `label` is `"<path>#<idx>"` so panics from the caller point to a specific
/// directive within the `.wast`.
pub fn for_each_valid_wasm_in_wast(wast_path: &Path, mut f: impl FnMut(&str, &[u8])) {
    let source = fs::read_to_string(wast_path)
        .unwrap_or_else(|e| panic!("couldn't read {}: {e}", wast_path.display()));
    let buf = wast::parser::ParseBuffer::new(&source)
        .unwrap_or_else(|e| panic!("couldn't lex {}: {e}", wast_path.display()));
    let wast: wast::Wast = wast::parser::parse(&buf)
        .unwrap_or_else(|e| panic!("couldn't parse {}: {e}", wast_path.display()));

    let mut idx = 0;
    for directive in wast.directives {
        let mut quoted = match directive {
            wast::WastDirective::Module(q) | wast::WastDirective::ModuleDefinition(q) => q,
            _ => continue,
        };
        let bytes = quoted.encode().unwrap_or_else(|e| {
            panic!(
                "couldn't encode module #{idx} in {}: {e}",
                wast_path.display()
            )
        });
        let label = format!("{}#{idx}", wast_path.display());
        f(&label, &bytes);
        idx += 1;
    }
}
