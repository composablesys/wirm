//! Container types for component-level item lists.
//!
//! Each holds an `AppendOnlyVec` of one IR variant and a thin `add` method
//! that pushes and returns the typed ID. They share the same shape today;
//! kept as separate structs because they're conceptually distinct and may
//! grow per-kind helpers as the mutation API rounds out.

use crate::ir::id::{AliasId, CanonicalFuncId, ComponentTypeId};
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

    pub(crate) fn add(&mut self, alias: ComponentAlias<'a>) -> AliasId {
        let id = AliasId(self.items.len() as u32);
        self.items.push(alias);
        id
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

    /// Add a new canonical function to the component.
    pub(crate) fn add(&mut self, canon: CanonicalFunction) -> CanonicalFuncId {
        let id = CanonicalFuncId(self.items.len() as u32);
        self.items.push(canon);
        id
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

    /// Add a new component type to the component.
    /// This assumes that scope registration is done by the caller!
    pub(crate) fn add(&mut self, ty: ComponentType<'a>) -> ComponentTypeId {
        let id = ComponentTypeId(self.items.len() as u32);
        self.items.push(Box::new(ty));
        id
    }
}
