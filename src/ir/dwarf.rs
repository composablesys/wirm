use std::collections::HashMap;

use crate::error::Error;
use crate::ir::types::CustomSection;

/// Wasm DWARF uses 32-bit code addresses.
const WASM_DWARF_ADDRESS_SIZE: u8 = 4;
/// Byte length of the unit-length field at the start of a DWARF32 line program.
const DWARF32_LENGTH_FIELD_BYTES: usize = 4;
/// Byte length of the unit-length field at the start of a DWARF64 line program
/// (4-byte 0xffffffff magic + 8-byte length).
const DWARF64_LENGTH_FIELD_BYTES: usize = 12;

/// Per-function input-side metadata the DWARF rewriter consults to translate
/// in-function PCs to/from DWARF row addresses.
#[derive(Clone, Debug)]
pub struct OrigFuncDebugData {
    /// DWARF address of the function's first instruction (= bytes of size LEB
    /// + locals declarations, counted from the body's size LEB byte).
    pub first_instr_dwarf_offset: usize,
}

#[derive(Clone, Debug, Default)]
pub struct ModuleDebugData<'a> {
    /// `.debug_*` sections in the order they appeared in the input module.
    /// Encode emits them in this order after the rest of `custom_sections`.
    pub(crate) sections: Vec<CustomSection<'a>>,
    /// Per-local-function metadata, keyed by function index.
    pub(crate) per_func: HashMap<u32, OrigFuncDebugData>,
}

impl<'a> ModuleDebugData<'a> {
    pub(crate) fn new(
        sections: Vec<CustomSection<'a>>,
        per_func: HashMap<u32, OrigFuncDebugData>,
    ) -> Self {
        Self { sections, per_func }
    }

    /// Read-only view of the underlying `.debug_*` custom sections.
    pub fn sections(&self) -> &[CustomSection<'a>] {
        &self.sections
    }

    /// Whether a custom-section name matches the DWARF convention. Includes
    /// any `.debug_*` name so non-standard extensions still round-trip via
    /// `ModuleDebugData` rather than escaping into `custom_sections`.
    pub fn is_dwarf_section_name(name: &str) -> bool {
        name.starts_with(".debug_")
    }
}

/// New per-function maps the rewriter consults to emit translated DWARF rows.
pub(crate) struct PerFuncEncodeMaps<'m> {
    /// Start byte of each emitted op within the function body, emit order.
    pub pcs: &'m [usize],
    /// Original-instruction index each emitted op anchors to.
    pub anchors: &'m [usize],
    /// DWARF address of the function's first instruction in the new module.
    pub first_instr_dwarf_offset: usize,
    /// Total bytes of the new function body (size LEB + content).
    pub body_total_size: usize,
}

/// Rewrite `.debug_line` so row addresses match the new code layout. Each row
/// inherits the source location of the original op its emitted op anchors to.
/// Assumes one sequence per local function in code-section order
/// (`wasm-tools parse --generate-dwarf`'s convention).
pub(crate) fn rewrite_debug_line(
    input_bytes: &[u8],
    debug: &ModuleDebugData,
    per_func_new: &HashMap<u32, PerFuncEncodeMaps<'_>>,
    orig_per_instr_pcs: &HashMap<u32, Vec<usize>>,
) -> Result<Vec<u8>, Error> {
    use gimli::{
        write::{self, Address, ConvertLineRow, DebugLine, EndianVec},
        EndianSlice, LittleEndian, SectionId,
    };

    let endian = LittleEndian;
    let dl_bytes = EndianSlice::new(input_bytes, endian);
    let empty = EndianSlice::new(&[], endian);

    let section_lookup = |name: &str| -> Option<EndianSlice<'_, LittleEndian>> {
        debug
            .sections()
            .iter()
            .find(|s| s.name == name)
            .map(|s| EndianSlice::new(s.data.as_ref(), endian))
    };
    let debug_str_bytes = section_lookup(".debug_str").unwrap_or(empty);
    let debug_line_str_bytes = section_lookup(".debug_line_str").unwrap_or(empty);
    let debug_str_offsets_bytes = section_lookup(".debug_str_offsets").unwrap_or(empty);

    let dwarf_read = gimli::read::Dwarf::load(|id| -> Result<_, gimli::Error> {
        Ok(match id {
            SectionId::DebugLine => dl_bytes,
            SectionId::DebugStr => debug_str_bytes,
            SectionId::DebugLineStr => debug_line_str_bytes,
            SectionId::DebugStrOffsets => debug_str_offsets_bytes,
            _ => empty,
        })
    })
    .map_err(|e| Error::DwarfError(format!("loading .debug_line: {e}")))?;

    // Parse the line program at offset 0. Multi-program (multi-CU) inputs are
    // refused below; the rewriter doesn't yet route per-CU.
    let from_program = dwarf_read
        .debug_line
        .program(
            gimli::DebugLineOffset(0),
            WASM_DWARF_ADDRESS_SIZE,
            None,
            None,
        )
        .map_err(|e| Error::DwarfError(format!("opening line program: {e}")))?;

    let header = from_program.header();
    let length_field_size = match header.encoding().format {
        gimli::Format::Dwarf32 => DWARF32_LENGTH_FIELD_BYTES,
        gimli::Format::Dwarf64 => DWARF64_LENGTH_FIELD_BYTES,
    };
    let first_program_bytes = length_field_size + header.unit_length();
    if first_program_bytes < input_bytes.len() {
        return Err(Error::DwarfError(format!(
            "multi-program .debug_line not yet supported ({first_program_bytes}/{} bytes consumed by the first program)",
            input_bytes.len()
        )));
    }

    let mut dwarf_write = write::Dwarf::new();
    let mut convert = dwarf_write
        .read_line_program(&dwarf_read, from_program, None, None)
        .map_err(|e| Error::DwarfError(format!("converter init: {e}")))?;

    // Walk input rows, grouping into sequences. We capture each row's
    // absolute address so we can rebuild the orig_pc → row lookup per
    // function below.
    struct CollectedRow {
        address: u64,
        row: write::LineRow,
    }
    let mut sequences: Vec<Vec<CollectedRow>> = Vec::new();
    let mut current_sequence: Vec<CollectedRow> = Vec::new();
    let mut current_base: u64 = 0;
    loop {
        let item = convert
            .read_row()
            .map_err(|e| Error::DwarfError(format!("reading row: {e}")))?;
        let Some(item) = item else {
            break;
        };
        match item {
            ConvertLineRow::SetAddress(addr) => {
                current_base = addr;
            }
            ConvertLineRow::Row(row) => {
                let address = current_base + row.address_offset;
                current_sequence.push(CollectedRow { address, row });
            }
            ConvertLineRow::EndSequence(_length) => {
                sequences.push(std::mem::take(&mut current_sequence));
                current_base = 0;
            }
        }
    }

    // sequence[i] is assigned to the i-th local function (sorted by index);
    // refuse mismatches so we never silently assign wrong source locations.
    if sequences.len() != per_func_new.len() {
        return Err(Error::DwarfError(format!(
            ".debug_line sequence/function count mismatch ({} vs {})",
            sequences.len(),
            per_func_new.len()
        )));
    }

    // `convert` already translated read FileIds into the write program's FileId
    // space, so we copy `row.file` verbatim and don't need the file mapping.
    let (mut new_program, _file_mapping) = convert.program();

    // Sorted so sequence index matches new function index deterministically.
    let mut func_indices: Vec<u32> = per_func_new.keys().copied().collect();
    func_indices.sort_unstable();

    for (seq_idx, func_idx) in func_indices.iter().enumerate() {
        let Some(orig_dbg) = debug.per_func.get(func_idx) else {
            // Should not happen if parse-side capture is complete.
            continue;
        };
        let Some(orig_pcs) = orig_per_instr_pcs.get(func_idx) else {
            continue;
        };
        let new_maps = per_func_new.get(func_idx).unwrap();

        // Build orig_pc → row lookup for this function's input sequence.
        let Some(input_seq) = sequences.get(seq_idx) else {
            // Input had no sequence for this function: leave it without
            // line-program coverage.
            continue;
        };
        let mut by_orig_pc: HashMap<usize, &write::LineRow> = HashMap::new();
        for cr in input_seq {
            let addr = cr.address as usize;
            if addr < orig_dbg.first_instr_dwarf_offset {
                continue;
            }
            let orig_pc = addr - orig_dbg.first_instr_dwarf_offset;
            by_orig_pc.insert(orig_pc, &cr.row);
        }

        new_program.begin_sequence(Some(Address::Constant(0)));
        for (emit_idx, &new_pc) in new_maps.pcs.iter().enumerate() {
            let anchor = new_maps.anchors[emit_idx];
            let Some(&orig_pc) = orig_pcs.get(anchor) else {
                continue;
            };
            let Some(src_row) = by_orig_pc.get(&orig_pc) else {
                continue;
            };
            let new_addr = (new_maps.first_instr_dwarf_offset + new_pc) as u64;
            let row = new_program.row();
            row.address_offset = new_addr;
            row.file = src_row.file;
            row.line = src_row.line;
            row.column = src_row.column;
            row.is_statement = src_row.is_statement;
            row.basic_block = src_row.basic_block;
            row.prologue_end = src_row.prologue_end;
            row.epilogue_begin = src_row.epilogue_begin;
            row.discriminator = src_row.discriminator;
            new_program.generate_row();
        }
        new_program.end_sequence(new_maps.body_total_size as u64);
    }

    // Use `dwarf_write`'s string tables — the program's row strings were
    // registered into them via `convert`, and gimli enforces matching BaseIds.
    let mut writer = DebugLine::from(EndianVec::new(endian));
    let encoding = new_program.encoding();
    new_program
        .write(
            &mut writer,
            encoding,
            &mut dwarf_write.line_strings,
            &mut dwarf_write.strings,
        )
        .map_err(|e| Error::DwarfError(format!("writing line program: {e}")))?;

    Ok(writer.0.into_vec())
}
