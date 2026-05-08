//! Container types for component-level item lists.
//!
//! Each holds an `AppendOnlyVec` of one IR variant and a thin `add` method
//! that pushes and returns the items-vec position. They share the same
//! shape today; kept as separate structs because they're conceptually
//! distinct and may grow per-kind helpers as the mutation API rounds out.

use crate::ir::AppendOnlyVec;
use wasmparser::{CanonicalFunction, ComponentAlias, ComponentType};

#[derive(Debug, Default)]
pub struct Aliases<'a> {
    pub items: AppendOnlyVec<ComponentAlias<'a>>,
}
impl<'a> Aliases<'a> {
    pub fn new(items: AppendOnlyVec<ComponentAlias<'a>>) -> Self {
        Self { items }
    }

    /// Push an alias and return its position in `items`. The caller is
    /// responsible for resolving the alias's index-space ID via
    /// `Component::add_section_and_get_id`.
    pub(crate) fn add(&mut self, alias: ComponentAlias<'a>) -> usize {
        let idx = self.items.len();
        self.items.push(alias);
        idx
    }
}

#[derive(Debug, Default)]
pub struct Canons {
    pub items: AppendOnlyVec<CanonicalFunction>,
}
impl Canons {
    pub fn new(items: AppendOnlyVec<CanonicalFunction>) -> Self {
        Self { items }
    }

    /// Push a canonical function and return its position in `items`. The
    /// caller is responsible for resolving the function index-space ID via
    /// `Component::add_section_and_get_id`.
    pub(crate) fn add(&mut self, canon: CanonicalFunction) -> usize {
        let idx = self.items.len();
        self.items.push(canon);
        idx
    }
}

#[derive(Debug, Default)]
pub struct ComponentTypes<'a> {
    pub items: AppendOnlyVec<Box<ComponentType<'a>>>,
}
impl<'a> ComponentTypes<'a> {
    pub fn new(items: AppendOnlyVec<Box<ComponentType<'a>>>) -> Self {
        Self { items }
    }

    /// Push a component type and return its position in `items`. The
    /// caller is responsible for resolving the index-space ID via
    /// `Component::add_section_and_get_id`, and for performing scope
    /// registration.
    pub(crate) fn add(&mut self, ty: ComponentType<'a>) -> usize {
        let idx = self.items.len();
        self.items.push(Box::new(ty));
        idx
    }
}
