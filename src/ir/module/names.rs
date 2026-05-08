//! Conversions for the wasm `name` custom section between the wasmparser and
//! wasm-encoder representations.

pub(crate) fn indirect_namemap_parser2encoder(
    namemap: wasmparser::IndirectNameMap,
) -> wasm_encoder::IndirectNameMap {
    let mut names = wasm_encoder::IndirectNameMap::new();
    for name in namemap {
        let naming = name.unwrap();
        names.append(naming.index, &namemap_parser2encoder(naming.names));
    }
    names
}

pub(crate) fn namemap_parser2encoder(namemap: wasmparser::NameMap) -> wasm_encoder::NameMap {
    let mut names = wasm_encoder::NameMap::new();
    for name in namemap {
        let naming = name.unwrap();
        names.append(naming.index, naming.name);
    }
    names
}
