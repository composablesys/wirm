# Fuzzing Decisions

Cross-session log for wirm's wasm-smith-based fuzzing. Current targets
are listed in `README.md`; this file is for decisions that aren't
derivable from the code.

---

## Next

- Tighten `module_instrument` and `component_instrument` to verify the
  injected ops are observable in the re-encoded body (not just "still
  valid wasm"). Catches silent no-op bugs where instrumentation is
  accepted but dropped during encode.

---

## Design choices

### Validation strategy: "wasmparser accepts = good" (loose)

Roundtrip targets check that `encode()`'s output is accepted by
`wasmparser::Validator::new_with_features(WasmFeatures::all())`. They
do **not** structurally diff the re-encoded output against the
original.

Rationale:
- Loose validation catches the high-value class of bugs — wirm emitting
  garbage or invalid wasm.
- Structural diffing would flag formatting/ordering differences
  (section reordering, collapsed entries, `max_align` normalization)
  that aren't bugs, creating noise.
- Can revisit with a stricter target later (`*_roundtrip_strict`) if
  the loose one stops finding things.

### Parse failures are silent, not crashes

If `wirm::Module::parse` (or `Component::parse`) returns `Err`, the
fuzz target returns early without panicking. wasm-smith can emit
binaries using features wirm doesn't support (e.g.
`shared_everything_threads`, `stack_switching`), and "wirm doesn't
parse this" is not a bug.

Only post-parse failures (encode error, validation error on re-encoded
bytes, walker/concretize divergence) are treated as crashes.

### Inputs that wasmparser itself rejects are silent, not crashes

Each target pre-validates the smith-produced bytes with `wasmparser`
before handing them to wirm. If wasmparser rejects the input (e.g.
wasm-smith producing an empty-`flags` component type, which is
structurally parseable but semantically invalid), the target returns
early.

The comparison we care about is apples-to-apples: "if wasmparser
accepts the input AND wirm parses it, then wirm's re-encoded output
should also be accepted by wasmparser". Treating wasm-smith-side
invalid output as a wirm bug would create false positives (we hit one
on the first component run — empty flags).

### Feature configuration for wasm-smith

Default wasm-smith `Config` — generates modules exercising all
features it knows about. Combined with "parse failure = silent skip",
wirm-unsupported features naturally filter themselves out.

If a specific feature starts dominating the fuzzer's time in
uninteresting ways (e.g. too many legacy-exceptions skips), narrow
the config.

### Version pinning

`wasm-smith` in `fuzz/Cargo.toml` is pinned to the same `0.X` version
as `wasmparser` / `wasm-encoder` in the parent crate. When bumping
`wasmparser` in `../Cargo.toml`, bump `wasm-smith` to match — the
wasm-tools crates release in lockstep.

### CI: periodic, not per-PR

Fuzzing runs on a weekly cron in `.github/workflows/fuzz.yml`, not
per-PR:
- Fuzz runs should be long (minutes to hours) to be useful — wrong
  shape for a per-PR gate.
- New fuzz-discovered crashes shouldn't block unrelated PRs.
- Weekly is enough for a library at wirm's pace of change.

If a cron run finds a crash, the artifact is uploaded and a separate
issue should be opened to investigate — don't auto-file.
