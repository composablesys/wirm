# DWARF test fixtures

Wasm modules carrying hand-authored DWARF debug info, used by the rewriter
tests in `tests/dwarf.rs` and the proptest in `src/ir/module/test.rs`.

## Regenerating

The `.wasm` files are produced from their sibling `.wat` files by
`wasm-tools parse --generate-dwarf full`, which synthesizes line-number
information from the `.wat`'s source positions. Re-run the command after
editing a `.wat` so the bundled `.wasm` stays in sync — otherwise tests will
either fail in surprising ways or, worse, keep passing against stale
expectations.

```sh
# From this directory:
wasm-tools parse --generate-dwarf full add.wat       -o add.wasm
wasm-tools parse --generate-dwarf full two_funcs.wat -o two_funcs.wasm
```

`wasm-tools 1.247.0` was used to produce the currently checked-in `.wasm`
files; other versions are likely fine but the byte layout of the emitted
DWARF may shift in ways that move pinned addresses in the tests.

## Inspecting

`llvm-dwarfdump` is the quickest way to confirm the line program / DIE
addresses match what the tests assert:

```sh
llvm-dwarfdump --debug-line add.wasm
llvm-dwarfdump --debug-info add.wasm
```
