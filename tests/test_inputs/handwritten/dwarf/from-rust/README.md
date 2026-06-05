# From-rust DWARF fixture

`from-rust.wasm` is the output of compiling `src.rs` with `rustc`. Unlike the
sibling `add.wasm` / `two_funcs.wasm` (which use `wasm-tools --generate-dwarf`
to produce synthetic DWARF), this fixture is real compiler output and exercises
DIE shapes the hand-written fixtures don't cover:

- **DWARF v4** (rustc's default). The handwritten fixtures are v5.
- **`DW_FORM_addr` low/high_pc** alongside `DW_AT_ranges` rangelists on the CU.
- **`DW_TAG_inlined_subroutine`** for the `#[inline(always)]` helper, with its
  own `low_pc` / `high_pc` that the address translator must rewrite.
- A `DW_AT_low_pc(dead code)` tombstone on the `panic` subprogram DIE (the
  function is optimized away, but the DIE survives).

## Regenerating

```sh
# From this directory:
rustc --edition 2021 \
      --crate-type cdylib \
      --target wasm32-unknown-unknown \
      -C debuginfo=2 \
      -C opt-level=1 \
      -C panic=abort \
      -C strip=none \
      --remap-path-prefix "$(pwd)=." \
      src.rs -o from-rust.wasm
```

`opt-level=1` keeps the binary small (~3 KB) by letting LLVM eliminate unreached
core code; the `#[inline(always)]` attribute still forces the helper to be
inlined into both callers, so we get inlined-subroutine DIEs. `panic=abort`
avoids pulling unwinding infrastructure into the debug info.

`rustc 1.93.0 (254b59607 2026-01-19)` was used to produce the committed
`.wasm`. Other versions are likely fine but the byte layout shifts —
tests should assert semantic properties (round-trip equivalence, strong
invariant), not exact bytes.

## Inspecting

```sh
llvm-dwarfdump --debug-line from-rust.wasm
llvm-dwarfdump --debug-info from-rust.wasm
```
