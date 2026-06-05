//! Shared DWARF inspection helpers for the rewriter tests.
//!
//! Loaded both by the integration tests in `tests/dwarf.rs` (via the normal
//! `mod common::dwarf` path) and by the in-crate unit tests in
//! `src/ir/module/test.rs` (via `#[path]` include). Keeping the two sides on
//! the same parse path avoids drift on details like `address_size` and the
//! sort-order assumption baked into `lookup_src_at`.

#![allow(dead_code)]

/// Parse a wasm's `.debug_line` and return `(address, line, column)` for each
/// non-end-of-sequence row, in the order gimli emits them.
///
/// Assumes a single line program per `.debug_line` section (the rewriter
/// refuses multi-program inputs, so the helper does too). `address_size = 4`
/// is the wasm DWARF convention.
pub fn line_rows(wasm: &[u8]) -> Vec<(u64, u64, u64)> {
    use wasmparser::{Parser, Payload};
    let mut line_bytes: Vec<u8> = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::CustomSection(cs) = payload.expect("valid wasm") {
            if cs.name() == ".debug_line" {
                line_bytes = cs.data().to_vec();
                break;
            }
        }
    }
    let endian = gimli::LittleEndian;
    let dl = gimli::read::DebugLine::new(&line_bytes, endian);
    let program = dl
        .program(gimli::DebugLineOffset(0), 4, None, None)
        .expect("line program parses");
    let mut rows = program.rows();
    let mut out = Vec::new();
    while let Some((_header, row)) = rows.next_row().expect("row reads") {
        if row.end_sequence() {
            continue;
        }
        let line = row.line().map(|n| n.get()).unwrap_or(0);
        let column = match row.column() {
            gimli::ColumnType::LeftEdge => 0,
            gimli::ColumnType::Column(c) => c.get(),
        };
        out.push((row.address(), line, column));
    }
    out
}

/// Source-location lookup: find the row whose address is the largest
/// `≤ addr`, return its `(line, column)`.
///
/// Requires `rows` to be sorted ascending by address (the rewriter's
/// invariant on single-program inputs). Asserts in debug builds.
pub fn lookup_src_at(rows: &[(u64, u64, u64)], addr: u64) -> Option<(u64, u64)> {
    debug_assert!(
        rows.windows(2).all(|w| w[0].0 <= w[1].0),
        "lookup_src_at expects rows sorted by address; got {rows:?}",
    );
    rows.iter()
        .rev()
        .find(|(a, _, _)| *a <= addr)
        .map(|(_, l, c)| (*l, *c))
}

/// Walks `.debug_info` and collects each DIE's `(low_pc, high_pc)` pair.
/// Handles both `DW_FORM_addr` (absolute high_pc) and `DW_FORM_data*` /
/// `DW_FORM_udata` (high_pc as length, per the DWARF spec).
pub fn debug_info_pcs(wasm: &[u8]) -> Vec<(u64, u64)> {
    use std::collections::BTreeMap;
    use wasmparser::{Parser, Payload};
    let mut sections: BTreeMap<&'static str, Vec<u8>> = BTreeMap::new();
    let known: &[&'static str] = &[
        ".debug_info",
        ".debug_abbrev",
        ".debug_str",
        ".debug_line",
        ".debug_line_str",
    ];
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::CustomSection(cs) = payload.expect("valid wasm") {
            for name in known {
                if cs.name() == *name {
                    sections.insert(name, cs.data().to_vec());
                }
            }
        }
    }
    let endian = gimli::LittleEndian;
    let lookup = |name: &str| -> gimli::EndianSlice<'_, gimli::LittleEndian> {
        gimli::EndianSlice::new(
            sections.get(name).map(|v| v.as_slice()).unwrap_or(&[]),
            endian,
        )
    };
    let dwarf = gimli::read::Dwarf::load(|id| -> Result<_, gimli::Error> {
        Ok(match id {
            gimli::SectionId::DebugInfo => lookup(".debug_info"),
            gimli::SectionId::DebugAbbrev => lookup(".debug_abbrev"),
            gimli::SectionId::DebugStr => lookup(".debug_str"),
            gimli::SectionId::DebugLine => lookup(".debug_line"),
            gimli::SectionId::DebugLineStr => lookup(".debug_line_str"),
            _ => gimli::EndianSlice::new(&[], endian),
        })
    })
    .expect("load DWARF");

    let read_uint = |v: gimli::read::AttributeValue<_>| -> Option<u64> {
        match v {
            gimli::read::AttributeValue::Addr(a) => Some(a),
            gimli::read::AttributeValue::Data1(d) => Some(d as u64),
            gimli::read::AttributeValue::Data2(d) => Some(d as u64),
            gimli::read::AttributeValue::Data4(d) => Some(d as u64),
            gimli::read::AttributeValue::Data8(d) => Some(d),
            gimli::read::AttributeValue::Udata(d) => Some(d),
            _ => None,
        }
    };

    let mut out = Vec::new();
    let mut units = dwarf.units();
    while let Some(header) = units.next().expect("unit header") {
        let unit = dwarf.unit(header).expect("unit");
        let mut entries = unit.entries();
        while let Some(entry) = entries.next_dfs().expect("dfs") {
            let low = entry.attr_value(gimli::DW_AT_low_pc).and_then(read_uint);
            let high_raw = entry.attr_value(gimli::DW_AT_high_pc).and_then(read_uint);
            let high = match entry.attr_value(gimli::DW_AT_high_pc) {
                Some(gimli::read::AttributeValue::Addr(_)) => high_raw,
                Some(_) => high_raw.zip(low).map(|(l_len, l_addr)| l_addr + l_len),
                None => None,
            };
            if let (Some(l), Some(h)) = (low, high) {
                out.push((l, h));
            }
        }
    }
    out
}
