# Fuzzing Decisions

Cross-session log for wirm's wasm-smith-based fuzzing. Update whenever a
design choice is made so later sessions can pick up without re-deriving.

---

## Scope layering

Build out fuzz coverage in roughly this order. Each tier is a plateau —
land it, let it bake in CI, then expand to the next.

### Tier 1 — Modules (in progress)

- [x] `module_roundtrip` — wasm-smith → wirm parse → wirm encode → wasmparser validate.
- [x] `module_instrument` — wasm-smith → wirm parse → iterate + inject a trivial instruction (e.g. `nop` before every op) → encode → validate.

### Tier 2 — Components

- [ ] `component_roundtrip` — wasm-smith Component → wirm parse → encode → validate.
- [ ] `component_instrument` — same as Tier 1's instrument, but driving a `ComponentIterator`.
- [ ] `component_concretize` — for every import/export, call `concretize_{import,export}` and just verify no panic.
- [ ] `component_walk_topological` / `component_walk_structural` — drive both walkers on smith-produced components with a no-op visitor; assert no panic, no divergent section_idx between walkers.

wasm-smith's component support is less complete than its module support;
Tier 2 may hit smith-side gaps before wirm-side ones. Revisit dep
version if smith improves component coverage upstream.

---

## Design choices

### Validation strategy: "wasmparser accepts = good" (loose)

`module_roundtrip` checks that the output of `module.encode()` is accepted by
`wasmparser::Validator::new_with_features(WasmFeatures::all())`. It does
**not** structurally diff the re-encoded module against the original.

Rationale:
- Loose validation catches the high-value class of bugs — wirm emitting
  garbage or invalid wasm.
- Structural diffing would flag formatting/ordering differences (section
  reordering, collapsed entries, `max_align` normalization) that aren't
  bugs, creating noise.
- Can revisit with a stricter target later ("module_roundtrip_strict")
  if the loose one stops finding things.

### Parse failures are silent, not crashes

If `wirm::Module::parse` returns `Err`, the fuzz target returns early
without panicking. wasm-smith can emit modules using features wirm
doesn't support (e.g. `shared_everything_threads`, `stack_switching`),
and "wirm doesn't parse this" is not a bug.

Only post-parse failures (encode error, validation error on re-encoded
bytes) are treated as crashes.

### Feature configuration for wasm-smith

Default wasm-smith `Config` is used — generates modules exercising all
features it knows about. Combined with "parse failure = silent skip",
wirm-unsupported features naturally filter themselves out.

If a specific feature starts dominating the fuzzer's time in uninteresting
ways (e.g. too many legacy-exceptions skips), we can narrow the config.

### Version pinning

`wasm-smith` pinned to the same `0.X` version as `wasmparser` /
`wasm-encoder` in the parent crate. When bumping `wasmparser` in
`../Cargo.toml`, bump `wasm-smith` in `fuzz/Cargo.toml` to match.
The crates release in lockstep.

### CI: periodic, not per-PR

Fuzzing runs on a weekly cron in `.github/workflows/fuzz.yml`, not on
every PR. Reasons:
- Fuzz runs should be long (minutes to hours) to be useful; wrong shape
  for a per-PR gate.
- New fuzz-discovered crashes shouldn't block unrelated PRs.
- Weekly is enough for a library at wirm's pace of change.

If a cron run finds a crash, the artifact is uploaded and a separate
issue should be opened to investigate — don't try to auto-file.

---

## Running locally

See `fuzz/README.md`. TL;DR:

```
cargo install cargo-fuzz              # one-time
cargo +nightly fuzz run module_roundtrip
```

---

## Open questions (surface here when they arise)

- Do we want a long-form corpus checked in, or rely purely on fuzzer-generated coverage? (Currently: no committed corpus; cargo-fuzz starts from scratch each run.)
- Should the instrumentation targets validate *semantically* (the injected ops are observable in the encoded body) rather than just "valid wasm"? Probably yes once the round-trip target is stable.
