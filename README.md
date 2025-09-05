<div align="center">
  <h1>Wirm 🐉</h1>

  Wirm is a **W**ebAssembly **IR** **M**anipulation Library for the Component Model.

  **NOTE: This project used to be known as Orca, see discussion on refactor [here](https://github.com/thesuhas/wirm/issues/245).**

  It is based on [Dfinity's IC codebase](https://github.com/dfinity/ic/tree/master/rs/wasm_transform) and [Walrus](https://github.com/rustwasm/walrus/tree/main).
</div>

## About ##

`Wirm` is a light-weight, easy-to-use, Rust Library for performing WebAssembly transformations.
It uses [wasmparser](https://docs.rs/wasmparser/0.214.0/wasmparser/) and [wasm_encoder](https://docs.rs/wasm-encoder/0.214.0/wasm_encoder/) to parse and encode Wasm components/modules and maintains its own Intermediate Representation.

## Cargo Features ##

### Parallel Processing

Wirm supports parallel processing during module parsing and encoding to improve performance for modules with many functions. This feature uses [rayon](https://docs.rs/rayon/latest/rayon/) to process function bodies concurrently.

Enable the feature by adding the `parallel` feature to your `Cargo.toml`:

```toml
[dependencies]
wirm = { version = "1.1.0", features = ["parallel"] }
```

## Environment Setup ##

To install `wasm-tools`:
```shell
$ cargo install wasm-tools
```
