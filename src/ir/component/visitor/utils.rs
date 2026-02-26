//! Internal traversal and resolution machinery.
//!
//! This module mirrors the encode traversal logic but operates in a
//! read-only mode. It maintains:
//!
//! - A stack of component identities
//! - A stack of active index scopes
//! - A reference to the scope registry
//!
//! It is intentionally not exposed publicly to avoid leaking implementation
//! details such as pointer identity or scope IDs.
//!
//! # Safety and Invariants
//!
//! This traversal logic relies on the following invariants:
//!
//! - Component IDs are stable for the lifetime of the IR.
//! - Scoped IR nodes are stored in stable allocations.
//! - The scope registry is fully populated before traversal begins.
//! - No mutation of the component occurs during traversal.
//!
//! These guarantees allow resolution to rely on structural identity
//! without exposing internal identity mechanisms publicly.

use crate::ir::component::idx_spaces::{IndexSpaceOf, ScopeId, Space, SpaceSubtype, StoreHandle};
use crate::ir::component::refs::{Depth, IndexedRef, RefKind};
use crate::ir::component::scopes::{
    build_component_store, ComponentStore, GetScopeKind, RegistryHandle,
};
use crate::ir::component::section::ComponentSection;
use crate::ir::component::visitor::driver::VisitEvent;
use crate::ir::component::visitor::{ItemKind, ResolvedItem};
use crate::ir::id::ComponentId;
use crate::Component;

pub struct VisitCtxInner<'a> {
    pub(crate) registry: RegistryHandle,
    pub(crate) component_stack: Vec<ComponentId>, // may not need
    pub(crate) scope_stack: ScopeStack,
    pub(crate) node_has_nested_scope: Vec<bool>,
    pub(crate) store: StoreHandle,
    pub(crate) comp_store: ComponentStore<'a>,
    section_tracker_stack: Vec<SectionTracker>,
}

// =======================================
// =========== SCOPE INTERNALS ===========
// =======================================

impl<'a> VisitCtxInner<'a> {
    pub fn new(root: &'a Component<'a>) -> Self {
        let comp_store = build_component_store(root);
        Self {
            registry: root.scope_registry.clone(),
            component_stack: Vec::new(),
            section_tracker_stack: Vec::new(),
            scope_stack: ScopeStack::new(),
            node_has_nested_scope: Vec::new(),
            store: root.index_store.clone(),
            comp_store,
        }
    }

    pub fn visit_section(&mut self, section: &ComponentSection, num: usize) -> usize {
        self.section_tracker_stack
            .last_mut()
            .unwrap()
            .visit_section(section, num)
    }

    pub fn push_comp_section_tracker(&mut self) {
        self.section_tracker_stack.push(SectionTracker::default());
    }
    pub fn pop_comp_section_tracker(&mut self) {
        self.section_tracker_stack.pop();
    }

    pub fn push_component(&mut self, component: &Component) {
        let id = component.id;
        self.component_stack.push(id);
        self.push_comp_section_tracker();
        self.enter_comp_scope(id);
    }

    pub fn pop_component(&mut self) {
        let id = self.component_stack.pop().unwrap();
        self.pop_comp_section_tracker();
        self.exit_comp_scope(id);
    }
    pub fn curr_component(&self) -> &Component<'_> {
        let id = self.comp_at(Depth::default());
        self.comp_store.get(id)
    }

    pub fn maybe_enter_scope<T: GetScopeKind>(&mut self, node: &T) {
        let mut nested = false;
        if let Some(scope_entry) = self.registry.borrow().scope_entry(node) {
            nested = true;
            self.scope_stack.enter_scope(scope_entry.space);
        }
        self.node_has_nested_scope.push(nested);
    }

    pub fn maybe_exit_scope<T: GetScopeKind>(&mut self, node: &T) {
        let nested = self.node_has_nested_scope.pop().unwrap();
        if let Some(scope_entry) = self.registry.borrow().scope_entry(node) {
            // Exit the nested index space...should be equivalent to the ID
            // of the scope that was entered by this node
            let exited_from = self.scope_stack.exit_scope();
            debug_assert!(nested);
            debug_assert_eq!(scope_entry.space, exited_from);
        } else {
            debug_assert!(!nested);
        }
    }

    pub(crate) fn enter_comp_scope(&mut self, comp_id: ComponentId) {
        let scope_id = self
            .registry
            .borrow()
            .scope_of_comp(comp_id)
            .expect("Internal error: no scope found for component");
        self.node_has_nested_scope
            .push(!self.scope_stack.stack.is_empty());
        self.scope_stack.enter_scope(scope_id);
    }

    pub(crate) fn exit_comp_scope(&mut self, comp_id: ComponentId) {
        let scope_id = self
            .registry
            .borrow()
            .scope_of_comp(comp_id)
            .unwrap_or_else(|| panic!("Internal error: no scope found for component {comp_id:?}"));
        let exited_from = self.scope_stack.exit_scope();
        debug_assert_eq!(scope_id, exited_from);
    }

    pub(crate) fn comp_at(&self, depth: Depth) -> &ComponentId {
        let idx = self.component_stack.len() - depth.val() - 1;
        self.component_stack.get(idx).unwrap_or_else(|| {
            panic!(
                "Internal error: couldn't find component at depth {}; stack: {:?}",
                depth.val(),
                self.component_stack
            )
        })
    }
}

// ===============================================
// =========== ID RESOLUTION INTERNALS ===========
// ===============================================

impl VisitCtxInner<'_> {
    /// When looking up the ID of some node, we MUST consider whether the node we're assigning an ID for
    /// has a nested scope! If it does, this node's ID lives in its parent index space.
    pub(crate) fn lookup_id_for(
        &self,
        space: &Space,
        section: &ComponentSection,
        vec_idx: usize,
    ) -> u32 {
        let nested = self.node_has_nested_scope.last().unwrap_or(&false);
        let scope_id = if *nested {
            self.scope_stack.scope_at_depth(&Depth::parent())
        } else {
            self.scope_stack.curr_scope_id()
        };
        self.store
            .borrow()
            .scopes
            .get(&scope_id)
            .unwrap()
            .lookup_assumed_id(space, section, vec_idx) as u32
    }
    /// When looking up the ID of some node, we MUST consider whether the node we're assigning an ID for
    /// has a nested scope! If it does, this node's ID lives in its parent index space.
    pub(crate) fn lookup_id_with_subvec_for(
        &self,
        space: &Space,
        section: &ComponentSection,
        vec_idx: usize,
        subvec_idx: usize,
    ) -> u32 {
        let nested = self.node_has_nested_scope.last().unwrap_or(&false);
        let scope_id = if *nested {
            self.scope_stack.scope_at_depth(&Depth::parent())
        } else {
            self.scope_stack.curr_scope_id()
        };
        self.store
            .borrow()
            .scopes
            .get(&scope_id)
            .unwrap()
            .lookup_assumed_id_with_subvec(space, section, vec_idx, subvec_idx) as u32
    }

    /// Looking up a reference should always be relative to the scope of the node that
    /// contained the reference! No need to think about whether the node has a nested scope.
    pub(crate) fn index_from_assumed_id_no_cache(
        &self,
        r: &IndexedRef,
    ) -> (SpaceSubtype, usize, Option<usize>) {
        let scope_id = self.scope_stack.scope_at_depth(&r.depth);
        self.store
            .borrow()
            .scopes
            .get(&scope_id)
            .unwrap()
            .index_from_assumed_id_no_cache(r)
    }

    /// Looking up a reference should always be relative to the scope of the node that
    /// contained the reference! No need to think about whether the node has a nested scope.
    pub(crate) fn index_from_assumed_id(
        &mut self,
        r: &IndexedRef,
    ) -> (SpaceSubtype, usize, Option<usize>) {
        let scope_id = self.scope_stack.scope_at_depth(&r.depth);
        self.store
            .borrow_mut()
            .scopes
            .get_mut(&scope_id)
            .unwrap()
            .index_from_assumed_id(r)
    }
}

// =================================================
// =========== NODE RESOLUTION INTERNALS ===========
// =================================================

impl VisitCtxInner<'_> {
    pub fn lookup_root_comp_name(&self) -> Option<&str> {
        self.curr_component().component_name.as_deref()
    }
    pub fn lookup_comp_name(&self, id: u32) -> Option<&str> {
        self.curr_component().components_names.get(id)
    }
    pub fn lookup_comp_inst_name(&self, id: u32) -> Option<&str> {
        self.curr_component().instance_names.get(id)
    }
    pub fn lookup_comp_type_name(&self, id: u32) -> Option<&str> {
        self.curr_component().type_names.get(id)
    }
    pub fn lookup_comp_func_name(&self, id: u32) -> Option<&str> {
        self.curr_component().func_names.get(id)
    }
    pub fn lookup_module_name(&self, id: u32) -> Option<&str> {
        self.curr_component().module_names.get(id)
    }
    pub fn lookup_core_inst_name(&self, id: u32) -> Option<&str> {
        self.curr_component().core_instances_names.get(id)
    }
    pub fn lookup_core_type_name(&self, id: u32) -> Option<&str> {
        self.curr_component().core_type_names.get(id)
    }
    pub fn lookup_core_func_name(&self, id: u32) -> Option<&str> {
        self.curr_component().core_func_names.get(id)
    }
    pub fn lookup_global_name(&self, id: u32) -> Option<&str> {
        self.curr_component().global_names.get(id)
    }
    pub fn lookup_memory_name(&self, id: u32) -> Option<&str> {
        self.curr_component().memory_names.get(id)
    }
    pub fn lookup_tag_name(&self, id: u32) -> Option<&str> {
        self.curr_component().tag_names.get(id)
    }
    pub fn lookup_table_name(&self, id: u32) -> Option<&str> {
        self.curr_component().table_names.get(id)
    }
    pub fn lookup_value_name(&self, id: u32) -> Option<&str> {
        self.curr_component().value_names.get(id)
    }

    pub fn resolve_all(&self, refs: &[RefKind]) -> Vec<ResolvedItem<'_, '_>> {
        let mut items = vec![];
        for r in refs.iter() {
            items.push(self.resolve(&r.ref_));
        }

        items
    }

    pub fn resolve(&self, r: &IndexedRef) -> ResolvedItem<'_, '_> {
        let (vec, idx, subidx) = self.index_from_assumed_id_no_cache(r);
        if r.space != Space::CoreType {
            assert!(
                subidx.is_none(),
                "only core types (with rec groups) should ever have subvec indices!"
            );
        }

        let comp_id = self.comp_at(r.depth);
        let referenced_comp = self.comp_store.get(comp_id);

        let space = r.space;
        match vec {
            SpaceSubtype::Main => match space {
                Space::Comp => ResolvedItem::Component(r.index, &referenced_comp.components[idx]),
                Space::CompType => {
                    ResolvedItem::CompType(r.index, &referenced_comp.component_types.items[idx])
                }
                Space::CompInst => {
                    ResolvedItem::CompInst(r.index, &referenced_comp.component_instance[idx])
                }
                Space::CoreInst => ResolvedItem::CoreInst(r.index, &referenced_comp.instances[idx]),
                Space::CoreModule => ResolvedItem::Module(r.index, &referenced_comp.modules[idx]),
                Space::CoreType => {
                    ResolvedItem::CoreType(r.index, &referenced_comp.core_types[idx])
                }
                Space::CompFunc | Space::CoreFunc => {
                    ResolvedItem::Func(r.index, &referenced_comp.canons.items[idx])
                }
                Space::CompVal
                | Space::CoreMemory
                | Space::CoreTable
                | Space::CoreGlobal
                | Space::CoreTag
                | Space::NA => unreachable!(
                    "This spaces don't exist in a main vector on the component IR: {vec:?}"
                ),
            },
            SpaceSubtype::Export => ResolvedItem::Export(r.index, &referenced_comp.exports[idx]),
            SpaceSubtype::Import => ResolvedItem::Import(r.index, &referenced_comp.imports[idx]),
            SpaceSubtype::Alias => ResolvedItem::Alias(r.index, &referenced_comp.alias.items[idx]),
        }
    }
}

#[derive(Clone)]
pub(crate) struct ScopeStack {
    pub(crate) stack: Vec<ScopeId>,
}
impl ScopeStack {
    fn new() -> Self {
        Self { stack: vec![] }
    }
    pub(crate) fn curr_scope_id(&self) -> ScopeId {
        self.stack.last().cloned().unwrap()
    }
    pub(crate) fn scope_at_depth(&self, depth: &Depth) -> ScopeId {
        *self
            .stack
            .get(self.stack.len() - depth.val() - 1)
            .unwrap_or_else(|| {
                panic!(
                    "couldn't find scope at depth {}; this is the current scope stack: {:?}",
                    depth.val(),
                    self.stack
                )
            })
    }
    pub fn enter_scope(&mut self, id: ScopeId) {
        self.stack.push(id)
    }
    pub fn exit_scope(&mut self) -> ScopeId {
        self.stack.pop().unwrap()
    }
}

// General trackers for indices of item vectors (used to track where i've been during visitation)
#[derive(Default)]
struct SectionTracker {
    last_processed_module: usize,
    last_processed_alias: usize,
    last_processed_core_ty: usize,
    last_processed_comp_ty: usize,
    last_processed_imp: usize,
    last_processed_exp: usize,
    last_processed_core_inst: usize,
    last_processed_comp_inst: usize,
    last_processed_canon: usize,
    last_processed_component: usize,
    last_processed_start: usize,
    last_processed_custom: usize,
}
impl SectionTracker {
    /// This function is used while traversing the component. This means that we
    /// should already know the space ID associated with the component section
    /// (if in visiting this next session we enter some inner index space).
    ///
    /// So, we use the associated space ID to return the inner index space. The
    /// calling function should use this return value to then context switch into
    /// this new index space. When we've finished visiting the section, swap back
    /// to the returned index space's `parent` (a field on the space).
    pub fn visit_section(&mut self, section: &ComponentSection, num: usize) -> usize {
        let tracker = match section {
            ComponentSection::Component => &mut self.last_processed_component,
            ComponentSection::Module => &mut self.last_processed_module,
            ComponentSection::Alias => &mut self.last_processed_alias,
            ComponentSection::CoreType => &mut self.last_processed_core_ty,
            ComponentSection::ComponentType => &mut self.last_processed_comp_ty,
            ComponentSection::ComponentImport => &mut self.last_processed_imp,
            ComponentSection::ComponentExport => &mut self.last_processed_exp,
            ComponentSection::CoreInstance => &mut self.last_processed_core_inst,
            ComponentSection::ComponentInstance => &mut self.last_processed_comp_inst,
            ComponentSection::Canon => &mut self.last_processed_canon,
            ComponentSection::CustomSection => &mut self.last_processed_custom,
            ComponentSection::ComponentStartSection => &mut self.last_processed_start,
        };

        let curr = *tracker;
        *tracker += num;
        curr
    }
}

pub fn for_each_indexed<'ir, T>(
    slice: &'ir [T],
    start: usize,
    count: usize,
    mut f: impl FnMut(usize, &'ir T),
) {
    debug_assert!(start + count <= slice.len());

    for i in 0..count {
        let idx = start + i;
        f(idx, &slice[idx]);
    }
}

pub fn emit_indexed<'ir, T: IndexSpaceOf>(
    out: &mut Vec<VisitEvent<'ir>>,
    idx: usize,
    item: &'ir T,
    make: fn(ItemKind, usize, &'ir T) -> VisitEvent<'ir>,
) {
    out.push(make(item.index_space_of().into(), idx, item));
}
