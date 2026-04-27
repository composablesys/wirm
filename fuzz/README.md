# wirm fuzz harness

wasm-smith-based fuzzing for wirm. See [`DECISIONS.md`](DECISIONS.md) for
scope, target roadmap, and design rationale.

## One-time setup

```
cargo install cargo-fuzz
```

cargo-fuzz needs a nightly toolchain for libFuzzer sanitizers:

```
rustup toolchain install nightly
```

## Running a target

```
cargo +nightly fuzz run module_roundtrip
```

The fuzzer runs until you `Ctrl-C` it (or it finds a crash). Crash inputs
are written to `fuzz/artifacts/module_roundtrip/`. Reproduce with:

```
cargo +nightly fuzz run module_roundtrip fuzz/artifacts/module_roundtrip/<hash>
```

To bound the run (useful in CI):

```
cargo +nightly fuzz run module_roundtrip -- -max_total_time=300
```

## Current targets

| Target                  | Exercises                                                                                          |
|-------------------------|----------------------------------------------------------------------------------------------------|
| `module_roundtrip`      | `Module::parse` → `Module::encode` → `wasmparser::Validator`                                       |
| `module_instrument`     | `module_roundtrip` + iterate and inject `nop` before every op                                      |
| `component_roundtrip`   | `Component::parse` → `Component::encode` → `wasmparser::Validator`                                 |
| `component_instrument`  | `component_roundtrip` + iterate and inject `nop` before every op                                   |
| `component_concretize`  | `Component::concretize_import` / `concretize_export` on every named import/export                  |
| `component_walks`       | `walk_structural` ≡ `walk_topological`; root `section_idx` in range + monotonic vs. wasmparser     |

More targets planned — see `DECISIONS.md`.

## Reproducing a CI crash locally

When the weekly `Fuzz` workflow finds a crash, the job fails and uploads
a `fuzz-artifacts-<target>` artifact containing the raw crash input(s).

1. Download the artifact from the failed workflow run (Actions tab →
   the failed run → "Artifacts" section at the top).
2. Unzip into this directory so paths line up:
   ```
   wirm/fuzz/artifacts/<target>/crash-<hash>
   ```
3. Reproduce:
   ```
   cargo +nightly fuzz run <target> fuzz/artifacts/<target>/crash-<hash>
   ```
4. (Optional) Shrink the input to the smallest reproducer:
   ```
   cargo +nightly fuzz tmin <target> fuzz/artifacts/<target>/crash-<hash>
   ```

The CI log also prints an `Arbitrary`-decoded dump of each crash input
(via `cargo fuzz fmt`) before the upload step — useful when you want to
see the shape of the offending module without downloading anything.

### Reproducing from the CI log alone

The libFuzzer crash report in the log already contains the input bytes,
so you can skip the artifact download. Look for the `Base64:` line right
above the artifact path, e.g.:

```
Test unit written to /home/runner/work/wirm/wirm/fuzz/artifacts/module_roundtrip/crash-d12c78f2571c0c34df011c59a12562e44e399053
Base64: rQEOrP////8A//v//wAAzgBv/wAAAAAAAAA3/wH/EW8AAAAAAAD/+f//////EUD/Ef////9vKw==
```

Decode it into the matching artifact path and rerun the target:

```
mkdir -p fuzz/artifacts/module_roundtrip
echo 'rQEOrP////8A//v//wAAzgBv/wAAAAAAAAA3/wH/EW8AAAAAAAD/+f//////EUD/Ef////9vKw==' \
    | base64 -d > fuzz/artifacts/module_roundtrip/crash-d12c78f2571c0c34df011c59a12562e44e399053
cargo +nightly fuzz run module_roundtrip \
    fuzz/artifacts/module_roundtrip/crash-d12c78f2571c0c34df011c59a12562e44e399053
```

The filename only has to match what you pass on the command line — the
hash from the CI log is convenient but not required.

## When upgrading wasmparser

`wasm-smith`'s version in `fuzz/Cargo.toml` must stay in sync with the
`wasmparser` version in the parent crate's `Cargo.toml`. The wasm-tools
crates release in lockstep; bump both together.
