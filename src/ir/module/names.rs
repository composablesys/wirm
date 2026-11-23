//! Conversions for the wasm `name` custom section between the wasmparser and
//! wasm-encoder representations.

use std::collections::HashMap;

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

pub(crate) fn namemap_parser2vec(namemap: wasmparser::NameMap) -> Vec<(u32, String)> {
    namemap
        .into_iter()
        .map(|name| {
            let naming = name.unwrap();
            (naming.index, naming.name.to_string())
        })
        .collect()
}

pub(crate) fn indirect_namemap_parser2vec(
    namemap: wasmparser::IndirectNameMap,
) -> Vec<(u32, wasm_encoder::NameMap)> {
    namemap
        .into_iter()
        .map(|name| {
            let naming = name.unwrap();
            (naming.index, namemap_parser2encoder(naming.names))
        })
        .collect()
}

/// Translate the indices of `names` through `mapping`, dropping entries whose
/// index has no mapping. The result is ordered by the new index.
pub(crate) fn remap_names<'n, T>(
    names: &'n [(u32, T)],
    mapping: &HashMap<u32, u32>,
) -> Vec<(u32, &'n T)> {
    let mut remapped: Vec<_> = names
        .iter()
        .filter_map(|(idx, name)| Some((*mapping.get(idx)?, name)))
        .collect();
    remapped.sort_by_key(|(idx, _)| *idx);
    remapped
}
