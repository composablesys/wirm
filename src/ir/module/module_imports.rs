//! Intermediate Representation of a Module's Imports

use crate::ir::id::{FunctionID, ImportsID};
use crate::ir::types::{InjectTag, Tag, TagUtils};
use wasmparser::TypeRef;

// TODO: Need to handle the relationship between Functions and Imports
/// Represents an import in a WebAssembly module.
#[derive(Debug, Clone)]
pub struct Import<'a> {
    /// The module being imported from.
    pub module: &'a str,
    /// The name of the imported item.
    pub name: &'a str,
    /// The type of the imported item.
    pub ty: TypeRef,
    /// The name (in the custom section) of the imported item.
    pub custom_name: Option<String>,
    pub(crate) deleted: bool,
    pub tag: InjectTag,
}
impl TagUtils for Import<'_> {
    fn get_or_create_tag(&mut self) -> &mut Tag {
        self.tag.get_or_insert_default()
    }

    fn get_tag(&self) -> &Option<Tag> {
        &self.tag
    }
}
impl<'a> From<wasmparser::Import<'a>> for Import<'a> {
    fn from(import: wasmparser::Import<'a>) -> Self {
        Import {
            module: import.module,
            name: import.name,
            ty: import.ty,
            custom_name: None,
            deleted: false,
            tag: None,
        }
    }
}

impl Import<'_> {
    /// Check whether this is an import for a function.
    pub fn is_function(&self) -> bool {
        matches!(self.ty, TypeRef::Func(_))
    }
    /// Check whether this is an import for a global.
    pub fn is_global(&self) -> bool {
        matches!(self.ty, TypeRef::Global(_))
    }
    /// Check whether this is an import for a table.
    pub fn is_table(&self) -> bool {
        matches!(self.ty, TypeRef::Table(_))
    }
    /// Check whether this is an import for a tag.
    pub fn is_tag(&self) -> bool {
        matches!(self.ty, TypeRef::Tag(_))
    }
    /// Check whether this is an import for a memory.
    pub fn is_memory(&self) -> bool {
        matches!(self.ty, TypeRef::Memory(_))
    }
}

/// Represents the Imports Section of a WASM Module
#[derive(Clone, Debug, Default)]
pub struct ModuleImports<'a> {
    /// The imports.
    imports: Vec<Import<'a>>,

    pub(crate) num_funcs: u32,
    pub(crate) num_funcs_added: u32,
    pub(crate) num_globals: u32,
    pub(crate) num_globals_added: u32,
    pub(crate) num_tables: u32,
    pub(crate) num_tables_added: u32,
    pub(crate) num_tags: u32,
    pub(crate) num_tags_added: u32,
    pub(crate) num_memories: u32,
    pub(crate) num_memories_added: u32,
}

impl<'a> ModuleImports<'a> {
    /// Creates a new `ModuleImports` struct given a Vec of Imports
    pub fn new(imports: Vec<Import<'a>>) -> Self {
        let mut def = Self::default();
        for import in imports.iter() {
            if import.is_function() {
                def.num_funcs += 1;
            } else if import.is_global() {
                def.num_globals += 1;
            } else if import.is_table() {
                def.num_tables += 1;
            } else if import.is_tag() {
                def.num_tags += 1;
            } else if import.is_memory() {
                def.num_memories += 1;
            }
        }
        def.imports = imports;
        def
    }

    /// Checks if there are no Imports
    pub fn is_empty(&self) -> bool {
        self.imports.is_empty()
    }

    /// Get an iterator over the imports.
    pub fn iter(&self) -> std::slice::Iter<'_, Import<'a>> {
        self.imports.iter()
    }

    /// Get a mutable iterator over the imports.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, Import<'a>> {
        self.imports.iter_mut()
    }

    /// Set the name of a given import using the ImportsID.
    pub fn set_name(&mut self, name: String, imports_id: ImportsID) {
        self.imports[*imports_id as usize].custom_name = Some(name)
    }

    /// Set the name of an imported function, using the FunctionID rather
    /// than the ImportsID. Note that these are not necessarily equal if
    /// the module has non-function imports! (It is more efficient to
    /// do this operation using the ImportsID.)
    pub fn set_fn_name(&mut self, name: String, func_id: FunctionID) {
        for (curr_fn_id, import) in (0_u32..).zip(self.imports.iter_mut()) {
            if import.is_function() && curr_fn_id == *func_id {
                import.custom_name = Some(name);
                return;
            }
        }
    }

    /// Returns the number of imports
    pub fn len(&self) -> usize {
        self.imports.len()
    }

    /// Add a new import to the module.
    pub(crate) fn add<'b>(&'b mut self, import: Import<'a>) -> ImportsID {
        // using a match instead of import.is_*() to make sure that we're
        // exhaustive due to the compiler guarantees.
        match import.ty {
            TypeRef::Func(..) => {
                self.num_funcs += 1;
                self.num_funcs_added += 1;
            }
            TypeRef::Global(..) => {
                self.num_globals += 1;
                self.num_globals_added += 1;
            }
            TypeRef::Table(..) => {
                self.num_tables += 1;
                self.num_tables_added += 1;
            }
            TypeRef::Tag(..) => {
                self.num_tags += 1;
                self.num_tags_added += 1;
            }
            TypeRef::Memory(..) => {
                self.num_memories += 1;
                self.num_memories_added += 1;
            }
        }
        self.imports.push(import);
        ImportsID((self.imports.len() - 1) as u32)
    }

    pub(crate) fn delete(&mut self, imports_id: ImportsID) {
        self.imports[*imports_id as usize].deleted = true;
    }

    /// Find an import by the `module` and `name` and return its `ImportsID` if found
    pub fn find(&self, module: String, name: String) -> Option<ImportsID> {
        for (id, imp) in self.imports.iter().enumerate() {
            if imp.module == module.as_str() && imp.name == name.as_str() {
                return Some(ImportsID(id as u32));
            }
        }
        None
    }

    /// Get the function ID of an Imported Function
    pub fn get_func(&self, module: String, name: String) -> Option<FunctionID> {
        for (idx, imp) in self.imports.iter().enumerate() {
            if imp.is_function() && imp.module == module.as_str() && *imp.name == name {
                return Some(FunctionID(idx as u32));
            }
        }
        None
    }

    /// Get an Import by its `ImportsID`
    pub fn get(&self, id: ImportsID) -> &Import<'_> {
        &self.imports[*id as usize]
    }

    /// Get the name of an Import
    pub fn get_import_name(&self, imports_id: ImportsID) -> &Option<String> {
        &self.imports[*imports_id as usize].custom_name
    }
}
