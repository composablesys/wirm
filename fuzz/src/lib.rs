//! Shared helpers for the wirm-fuzz targets.

use wasmparser::{Parser, Payload};

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
