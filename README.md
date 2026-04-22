<div align="center">
  <h1>Wirm 🐉</h1>

  Wirm is a **W**ebAssembly **IR** **M**anipulation Library for the Component Model.

  **NOTE: This project used to be known as Orca, see discussion on refactor [here](https://github.com/thesuhas/wirm/issues/245).**

  It is based on [Dfinity's IC codebase](https://github.com/dfinity/ic/tree/master/rs/wasm_transform) and [Walrus](https://github.com/rustwasm/walrus/tree/main).
</div>

## About ##

`Wirm` is a light-weight, easy-to-use, Rust Library for performing WebAssembly transformations.
It uses [wasmparser](https://docs.rs/wasmparser/0.214.0/wasmparser/) and [wasm_encoder](https://docs.rs/wasm-encoder/0.214.0/wasm_encoder/) to parse and encode Wasm components/modules and maintains its own Intermediate Representation.

`Wirm` also includes some handy visitors for walking Wasm components:
- [`walk_structural`](https://docs.rs/wirm/latest/wirm/ir/component/visitor/fn.walk_structural.html): to walk a Component using its structural (in-file) order.
- [`walk_topological`](https://docs.rs/wirm/latest/wirm/ir/component/visitor/fn.walk_topological.html): to walk a Component in topological (dependency) order (useful when traversing a component post-instrumentation).

Several projects already leverage these visitors!
1. `Wirm` (that's right, we eat our own dogfood here): at encode time, `wirm` uses `walk_topological` to ensure that instrumented components get encoded without introducing forward references.
2. [`cviz`](https://github.com/cosmonic-labs/cviz): Uses `walk_structural` to discover the internal composition of a component.
3. [`splicer`](https://github.com/ejrgilbert/splicer): Uses `walk_structural` to split out subcomponents from their root.

## Cargo Features ##

### Parallel Processing

Wirm supports parallel processing during module parsing and encoding to improve performance for modules with many functions. This feature uses [rayon](https://docs.rs/rayon/latest/rayon/) to process function bodies concurrently.

Enable the feature by adding the `parallel` feature to your `Cargo.toml`:

```toml
[dependencies]
wirm = { version = "1.1.0", features = ["parallel"] }
```
