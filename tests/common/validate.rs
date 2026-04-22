//! Shared `wasmparser`-based validator for test helpers.
//!
//! Centralises the "validate with every feature enabled" choice so changing
//! it (e.g. to disable a proposal) only happens in one place. Path-reading
//! and error-logging stay per-caller so each site can pick the most natural
//! shape for its test context (panic / log / return bool).

/// Validate a wasm binary with `WasmFeatures::all()`. Returns `Ok(())` on
/// success or the validator's error otherwise.
pub fn validate_bytes(bytes: &[u8]) -> Result<(), wasmparser::BinaryReaderError> {
    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(bytes)
        .map(|_| ())
}
