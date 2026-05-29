//! DWARF debug-info lifted aside from a Wasm module's custom sections.
//!
//! When `Module::parse` is called with `with_dwarf = true`, `.debug_*` custom
//! sections are diverted here instead of flowing through `custom_sections`
//! opaquely. Keeping them separate lets DWARF be handled coherently with an
//! instrumented module rather than passed through byte-for-byte.

use crate::ir::types::CustomSection;

/// Holds the `.debug_*` custom sections lifted out of a Wasm module's
/// `custom_sections` list at parse time.
#[derive(Clone, Debug, Default)]
pub struct ModuleDebugData<'a> {
    /// `.debug_*` sections in the order they appeared in the input module.
    /// Encode emits them in this order after the rest of `custom_sections`.
    pub(crate) sections: Vec<CustomSection<'a>>,
}

impl<'a> ModuleDebugData<'a> {
    pub(crate) fn from_sections(sections: Vec<CustomSection<'a>>) -> Self {
        Self { sections }
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
