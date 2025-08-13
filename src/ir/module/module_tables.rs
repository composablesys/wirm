//! Intermediate representation of the Tables in a Module

use crate::ir::id::TableID;
use crate::ir::types::{ElementItems, ElementKind, InjectTag, Tag, TagUtils};
use wasmparser::{RefType, TableType};

/// Tables Section of a module
#[derive(Clone, Debug, Default)]
pub struct ModuleTables<'a> {
    tables: Vec<Table<'a>>,
}

impl<'a> ModuleTables<'a> {
    /// Create a new table section
    pub fn new(tables: Vec<Table<'a>>) -> Self {
        ModuleTables { tables }
    }

    /// Check if there are any tables
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    /// Create an iterable over the table section
    pub fn iter(&self) -> std::slice::Iter<'_, Table> {
        self.tables.iter()
    }

    /// Finds a unique function table in a module.
    ///
    /// Modules produced by compilers like LLVM typically have one function
    /// table for indirect function calls. This function will look for a single
    /// function table inside this module, and return that if found. If no
    /// function tables are present `None` will be returned
    ///
    /// # Errors
    ///
    /// Returns an error if there are two function tables in this module
    ///
    /// Inspired from [walrus' implementation]
    ///
    /// [walrus' implementation]: https://docs.rs/walrus/latest/walrus/struct.ModuleTables.html#method.main_function_table
    pub fn main_function(&self) -> Option<TableID> {
        let mut tables = self
            .tables
            .iter()
            .enumerate()
            .filter(|(_, t)| t.ty.element_type == RefType::FUNCREF);
        let id = match tables.next() {
            Some((index, _)) => Some(TableID(index as u32)),
            None => return None,
        };
        if tables.next().is_some() {
            panic!("module contains more than one function table");
        }
        id
    }

    /// Get a table
    pub fn get(&self, table_id: TableID) -> Option<TableType> {
        if *table_id < self.tables.len() as u32 {
            return Some(self.tables[*table_id as usize].ty);
        }
        None
    }

    /// Get a mutable reference to a table
    pub fn get_mut(&mut self, table_id: TableID) -> &mut TableType {
        if *table_id < self.tables.len() as u32 {
            return &mut self.tables[*table_id as usize].ty;
        }
        panic!("Invalid Table ID")
    }
}

#[derive(Clone, Debug)]
pub struct Table<'a> {
    pub ty: TableType,
    pub init_expr: Option<wasmparser::ConstExpr<'a>>,
    tag: InjectTag,
}
impl TagUtils for Table<'_> {
    fn get_or_create_tag(&mut self) -> &mut Tag {
        self.tag.get_or_insert_default()
    }

    fn get_tag(&self) -> &Option<Tag> {
        &self.tag
    }
}
impl<'a> Table<'a> {
    pub fn new(
        ty: TableType,
        init_expr: Option<wasmparser::ConstExpr<'a>>,
        tag: InjectTag,
    ) -> Self {
        Self { ty, init_expr, tag }
    }
}

#[derive(Clone, Debug)]
pub struct Element {
    pub kind: ElementKind,
    pub items: ElementItems,
    tag: InjectTag,
}

impl TagUtils for Element {
    fn get_or_create_tag(&mut self) -> &mut Tag {
        self.tag.get_or_insert_default()
    }

    fn get_tag(&self) -> &Option<Tag> {
        &self.tag
    }
}

impl Element {
    pub fn new(kind: ElementKind, items: ElementItems, tag: InjectTag) -> Self {
        Self { kind, items, tag }
    }
}
