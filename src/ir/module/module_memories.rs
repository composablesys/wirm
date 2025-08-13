use crate::ir::id::{ImportsID, MemoryID};
use crate::ir::module::{GetID, Iter, LocalOrImport, ReIndexable};
use crate::ir::types::{InjectTag, Tag, TagUtils};
use std::vec::IntoIter;
use wasmparser::MemoryType;

/// Intermediate representation of all the memories in a module.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct Memories {
    memories: Vec<Memory>,
    pub(crate) recalculate_ids: bool,
}
impl ReIndexable<Memory> for Memories {
    /// Get the number of memories
    fn len(&self) -> usize {
        self.memories.len()
    }
    fn remove(&mut self, mem_id: u32) -> Memory {
        self.memories.remove(mem_id as usize)
    }

    fn insert(&mut self, mem_id: u32, mem: Memory) {
        self.memories.insert(mem_id as usize, mem);
    }
    /// Add a new memory
    fn push(&mut self, mem: Memory) {
        self.memories.push(mem);
    }
}

impl Iter<Memory> for Memories {
    /// Get an iterator for the memories.
    fn iter(&self) -> std::slice::Iter<'_, Memory> {
        self.memories.iter()
    }

    fn get_into_iter(&self) -> IntoIter<Memory> {
        self.memories.clone().into_iter()
    }
}

impl Memories {
    /// Create a new memories section
    pub fn new(memories: Vec<Memory>) -> Self {
        Self {
            memories,
            recalculate_ids: false,
        }
    }

    /// Iterate over the memories in the module.
    ///     
    /// Note that memories may have been deleted.
    pub fn iter(&self) -> std::slice::Iter<'_, Memory> {
        <Self as Iter<Memory>>::iter(self)
    }

    /// Iterate over the memories in the module.
    ///     
    /// Note that memories may have been deleted.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, Memory> {
        self.memories.iter_mut()
    }

    /// Get a memory by its MemoryID
    pub fn get_mem_by_id(&self, mem_id: MemoryID) -> Option<&Memory> {
        if *mem_id < self.memories.len() as u32 {
            return Some(&self.memories[*mem_id as usize]);
        }
        None
    }

    /// Checks if there are no functions
    pub fn is_empty(&self) -> bool {
        self.memories.is_empty()
    }

    // =================
    // ==== GETTERS ====
    // =================

    /// Get kind of memory
    pub fn get_kind(&self, mem_id: MemoryID) -> &MemKind {
        &self.memories[*mem_id as usize].kind
    }

    /// Get mutable memory by ID
    pub fn get_mut(&mut self, mem_id: MemoryID) -> Option<&mut Memory> {
        if *mem_id < self.memories.len() as u32 {
            return Some(&mut self.memories[*mem_id as usize]);
        }
        None
    }

    /// Check if a memory is a local
    pub fn is_local(&self, mem_id: MemoryID) -> bool {
        self.memories[*mem_id as usize].is_local()
    }

    /// Check if a memory is an import
    pub fn is_import(&self, mem_id: MemoryID) -> bool {
        self.memories[*mem_id as usize].is_import()
    }

    /// Check if it's deleted
    pub fn is_deleted(&self, mem_id: MemoryID) -> bool {
        self.memories[*mem_id as usize].is_deleted()
    }

    // =======================
    // ==== MANIPULATIONS ====
    // =======================

    /// Delete a memory
    pub(crate) fn delete(&mut self, mem_id: MemoryID) {
        self.recalculate_ids = true;
        if *mem_id < self.memories.len() as u32 {
            self.memories[*mem_id as usize].delete();
        }
    }

    fn next_id(&self) -> MemoryID {
        MemoryID(self.memories.len() as u32)
    }

    pub(crate) fn add_local_mem(
        &mut self,
        mut local_mem: LocalMemory,
        ty: MemoryType,
        tag: InjectTag,
    ) -> MemoryID {
        self.recalculate_ids = true;
        // fix the ID of the memory
        let id = self.next_id();
        local_mem.mem_id = id;

        self.push(Memory {
            ty,
            kind: MemKind::Local(local_mem),
            deleted: false,
            tag,
        });
        id
    }

    pub(crate) fn add_import_mem(
        &mut self,
        imp_id: ImportsID,
        ty: MemoryType,
        // The id of the memory we're using (at least until re-indexing)
        imp_mem_id: u32,
        tag: InjectTag,
    ) {
        self.recalculate_ids = true;
        assert_eq!(*self.next_id(), imp_mem_id);
        self.memories.push(Memory {
            ty,
            kind: MemKind::Import(ImportedMemory {
                import_id: imp_id,
                import_mem_id: MemoryID(imp_mem_id),
            }),
            deleted: false,
            tag,
        });
    }
}

/// Represents a memory. Local or Imported depends on the `MemKind`.
#[derive(Clone, Debug)]
pub struct Memory {
    pub ty: MemoryType,
    pub(crate) kind: MemKind,
    pub(crate) deleted: bool,
    pub tag: InjectTag,
}
impl GetID for Memory {
    /// Get the ID of the function
    fn get_id(&self) -> u32 {
        match &self.kind {
            MemKind::Import(i) => *i.import_mem_id,
            MemKind::Local(l) => *l.mem_id,
        }
    }
}
impl LocalOrImport for Memory {
    /// Check if it's a local memory
    fn is_local(&self) -> bool {
        matches!(&self.kind, MemKind::Local(_))
    }

    /// Check if it's an imported memory
    fn is_import(&self) -> bool {
        matches!(&self.kind, MemKind::Import(_))
    }

    /// Check if this memory has been deleted
    fn is_deleted(&self) -> bool {
        self.deleted
    }
}
impl TagUtils for Memory {
    fn get_or_create_tag(&mut self) -> &mut Tag {
        self.tag.get_or_insert_default()
    }

    fn get_tag(&self) -> &Option<Tag> {
        &self.tag
    }
}
impl Memory {
    /// Create a new memory
    pub fn new(ty: MemoryType, kind: MemKind, tag: InjectTag) -> Self {
        Self {
            ty,
            kind,
            deleted: false,
            tag,
        }
    }

    // /// Change the kind of the memory
    // pub(crate) fn set_kind(&mut self, kind: MemKind) {
    //     self.kind = kind;
    //     // Resets deletion
    //     self.deleted = false;
    // }

    /// Get the kind of the memory
    pub fn kind(&self) -> &MemKind {
        &self.kind
    }

    /// Unwrap a local memory. If it is an imported memory, it panics.
    pub fn unwrap_local(&self) -> &LocalMemory {
        self.kind.unwrap_local()
    }

    /// Unwrap a local memory as mutable. If it is an imported memory, it panics.
    pub fn unwrap_local_mut(&mut self) -> &mut LocalMemory {
        self.kind.unwrap_local_mut()
    }

    pub(crate) fn delete(&mut self) {
        self.deleted = true;
    }
}

/// Represents whether a memory is a Local Memory or an Imported Memory
#[derive(Clone, Debug)]
pub enum MemKind {
    Local(LocalMemory),
    Import(ImportedMemory),
}

impl MemKind {
    /// Unwrap a local memory as a read-only reference. If it is an imported memory, it panics.
    pub fn unwrap_local(&self) -> &LocalMemory {
        match &self {
            MemKind::Local(l) => l,
            MemKind::Import(_) => panic!("Attempting to unwrap an imported memory as a local!!"),
        }
    }
    /// Unwrap a local memory as a mutable reference. If it is an imported memory, it panics.
    pub fn unwrap_local_mut(&mut self) -> &mut LocalMemory {
        match self {
            MemKind::Local(l) => l,
            MemKind::Import(_) => panic!("Attempting to unwrap an imported memory as a local!!"),
        }
    }
}

/// Intermediate Representation of a Local Memory
#[derive(Clone, Debug)]
pub struct LocalMemory {
    pub mem_id: MemoryID,
}

/// Intermediate representation of an Imported Memory. The actual Import is stored in the Imports field of the module.
#[derive(Clone, Debug)]
pub struct ImportedMemory {
    pub import_id: ImportsID,           // Maps to location in a module's imports
    pub(crate) import_mem_id: MemoryID, // Maps to location in a module's imported memories
}
