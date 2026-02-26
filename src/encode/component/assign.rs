use crate::ir::component::idx_spaces::{IndexSpaceOf, ScopeId, Space};
use crate::ir::component::refs::{Depth, IndexedRef};
use crate::ir::component::visitor::driver::{drive_event, VisitEvent};
use crate::ir::component::visitor::utils::VisitCtxInner;
use crate::ir::component::visitor::{ComponentVisitor, ItemKind, VisitCtx};
use crate::{Component, Module};
use std::collections::HashMap;
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance,
    ComponentType, ComponentTypeDeclaration, CoreType, Instance, InstanceTypeDeclaration,
    ModuleTypeDeclaration, SubType,
};

pub(crate) fn assign_indices<'ir>(
    ctx: &mut VisitCtx<'ir>,
    events: &Vec<VisitEvent<'ir>>,
) -> ActualIds {
    let mut assigner = Assigner::default();
    for event in events {
        drive_event(event, &mut assigner, ctx);
    }

    assigner.ids
}

#[derive(Default)]
struct Assigner {
    ids: ActualIds,
}
impl Assigner {
    /// When performing an ID _assignment_, we MUST consider whether the node we're assigning an ID for
    /// has a nested scope! If it does, this node's ID lives in its parent index space.
    fn assign_actual_id(
        &mut self,
        cx: &VisitCtx<'_>,
        is_inner_node: bool,
        space: &Space,
        assumed_id: u32,
    ) {
        let nested = cx.inner.node_has_nested_scope.last().unwrap_or(&false);
        let scope_id = if *nested && !is_inner_node {
            cx.inner.scope_stack.scope_at_depth(&Depth::parent())
        } else {
            cx.inner.scope_stack.curr_scope_id()
        };
        self.ids
            .assign_actual_id(scope_id, space, assumed_id as usize)
    }
}
impl ComponentVisitor<'_> for Assigner {
    fn exit_component(&mut self, cx: &VisitCtx<'_>, id: u32, component: &Component<'_>) {
        self.assign_actual_id(cx, false, &component.index_space_of(), id)
    }
    fn visit_module(&mut self, cx: &VisitCtx<'_>, id: u32, module: &Module<'_>) {
        self.assign_actual_id(cx, false, &module.index_space_of(), id)
    }
    fn visit_comp_type_decl(
        &mut self,
        cx: &VisitCtx<'_>,
        _decl_idx: usize,
        id: u32,
        _parent: &ComponentType<'_>,
        decl: &ComponentTypeDeclaration<'_>,
    ) {
        if matches!(
            decl,
            ComponentTypeDeclaration::CoreType(_) | ComponentTypeDeclaration::Type(_)
        ) {
            // this ID assignment will be handled by the type handler!
            return;
        }
        self.assign_actual_id(cx, true, &decl.index_space_of(), id)
    }
    fn visit_inst_type_decl(
        &mut self,
        cx: &VisitCtx<'_>,
        _decl_idx: usize,
        id: u32,
        _parent: &ComponentType<'_>,
        decl: &InstanceTypeDeclaration<'_>,
    ) {
        if matches!(
            decl,
            InstanceTypeDeclaration::CoreType(_) | InstanceTypeDeclaration::Type(_)
        ) {
            // this ID assignment will be handled by the type handler!
            return;
        }

        self.assign_actual_id(cx, true, &decl.index_space_of(), id)
    }
    fn exit_comp_type(&mut self, cx: &VisitCtx<'_>, id: u32, ty: &ComponentType<'_>) {
        // This node COULD have a nested scope (so pass false to is_inner_node)
        self.assign_actual_id(cx, false, &ty.index_space_of(), id)
    }
    fn visit_comp_instance(
        &mut self,
        cx: &VisitCtx<'_>,
        id: u32,
        instance: &ComponentInstance<'_>,
    ) {
        self.assign_actual_id(cx, true, &instance.index_space_of(), id)
    }
    fn visit_canon(
        &mut self,
        cx: &VisitCtx<'_>,
        _kind: ItemKind,
        id: u32,
        canon: &CanonicalFunction,
    ) {
        self.assign_actual_id(cx, true, &canon.index_space_of(), id)
    }
    fn visit_alias(
        &mut self,
        cx: &VisitCtx<'_>,
        _kind: ItemKind,
        id: u32,
        alias: &ComponentAlias<'_>,
    ) {
        self.assign_actual_id(cx, true, &alias.index_space_of(), id)
    }
    fn visit_comp_import(
        &mut self,
        cx: &VisitCtx<'_>,
        _kind: ItemKind,
        id: u32,
        import: &ComponentImport<'_>,
    ) {
        self.assign_actual_id(cx, true, &import.index_space_of(), id)
    }
    fn visit_comp_export(
        &mut self,
        cx: &VisitCtx<'_>,
        _kind: ItemKind,
        id: u32,
        export: &ComponentExport<'_>,
    ) {
        self.assign_actual_id(cx, true, &export.index_space_of(), id)
    }
    fn visit_module_type_decl(
        &mut self,
        cx: &VisitCtx<'_>,
        _decl_idx: usize,
        id: u32,
        _parent: &CoreType<'_>,
        decl: &ModuleTypeDeclaration<'_>,
    ) {
        self.assign_actual_id(cx, true, &decl.index_space_of(), id)
    }
    fn enter_core_rec_group(
        &mut self,
        cx: &VisitCtx<'_>,
        _count: usize,
        _core_type: &CoreType<'_>,
    ) {
        // just need to make sure there's a scope built :)
        // this is relevant for: (component (core rec) )
        self.ids.add_scope(cx.inner.scope_stack.curr_scope_id());
    }
    fn visit_core_subtype(&mut self, cx: &VisitCtx<'_>, id: u32, subtype: &SubType) {
        self.assign_actual_id(cx, true, &subtype.index_space_of(), id)
    }
    fn exit_core_type(&mut self, cx: &VisitCtx<'_>, id: u32, core_type: &CoreType<'_>) {
        // This node COULD have a nested scope (so pass false to is_inner_node)
        self.assign_actual_id(cx, false, &core_type.index_space_of(), id)
    }
    fn visit_core_instance(&mut self, cx: &VisitCtx<'_>, id: u32, inst: &Instance<'_>) {
        self.assign_actual_id(cx, true, &inst.index_space_of(), id)
    }
}

#[derive(Clone, Default)]
pub struct ActualIds {
    scopes: HashMap<ScopeId, IdsForScope>,
}
impl ActualIds {
    pub fn add_scope(&mut self, id: ScopeId) {
        self.scopes.entry(id).or_default();
    }
    pub fn get_scope(&self, id: ScopeId) -> Option<&IdsForScope> {
        self.scopes.get(&id)
    }
    pub fn assign_actual_id(&mut self, id: ScopeId, space: &Space, assumed_id: usize) {
        let ids = self.scopes.entry(id).or_default();
        ids.assign_actual_id(space, assumed_id)
    }

    /// Looking up a reference should always be relative to the scope of the node that
    /// contained the reference! No need to think about whether the node has a nested scope.
    pub fn lookup_actual_id_or_panic(&self, cx: &VisitCtxInner, r: &IndexedRef) -> usize {
        let scope_id = cx.scope_stack.scope_at_depth(&r.depth);
        let ids = self.scopes.get(&scope_id).unwrap_or_else(|| {
            panic!("Internal error: Attempted to assign a non-existent scope: {scope_id}")
        });
        ids.lookup_actual_id_or_panic(r)
    }
}

/// This is used at encode time. It tracks the actual ID that has been assigned
/// to some item by allowing for lookup of the assumed ID: `assumed_id -> actual_id`
/// This is important since we know what ID should be associated with something only at encode time,
/// since instrumentation has finished at that point and encoding of component items
/// can be done out-of-order to satisfy possible forward-references injected during instrumentation.
#[derive(Clone, Default)]
pub struct IdsForScope {
    // Component-level spaces
    comp: IdTracker,
    comp_func: IdTracker,
    comp_val: IdTracker,
    comp_type: IdTracker,
    comp_inst: IdTracker,

    // Core space (added by component model)
    core_inst: IdTracker, // (these are module instances)
    module: IdTracker,

    // Core spaces that exist at the component-level
    core_type: IdTracker,
    core_func: IdTracker, // these are canonical function decls!
    core_memory: IdTracker,
    core_table: IdTracker,
    core_global: IdTracker,
    core_tag: IdTracker,
}
impl IdsForScope {
    pub fn assign_actual_id(&mut self, space: &Space, assumed_id: usize) {
        if let Some(space) = self.get_space_mut(space) {
            space.assign_actual_id(assumed_id);
        }
    }

    fn get_space_mut(&mut self, space: &Space) -> Option<&mut IdTracker> {
        let s = match space {
            Space::Comp => &mut self.comp,
            Space::CompFunc => &mut self.comp_func,
            Space::CompVal => &mut self.comp_val,
            Space::CompType => &mut self.comp_type,
            Space::CompInst => &mut self.comp_inst,
            Space::CoreInst => &mut self.core_inst,
            Space::CoreModule => &mut self.module,
            Space::CoreType => &mut self.core_type,
            Space::CoreFunc => &mut self.core_func,
            Space::CoreMemory => &mut self.core_memory,
            Space::CoreTable => &mut self.core_table,
            Space::CoreGlobal => &mut self.core_global,
            Space::CoreTag => &mut self.core_tag,
            Space::NA => return None,
        };
        Some(s)
    }

    pub(crate) fn get_space(&self, space: &Space) -> Option<&IdTracker> {
        let s = match space {
            Space::Comp => &self.comp,
            Space::CompFunc => &self.comp_func,
            Space::CompVal => &self.comp_val,
            Space::CompType => &self.comp_type,
            Space::CompInst => &self.comp_inst,
            Space::CoreInst => &self.core_inst,
            Space::CoreModule => &self.module,
            Space::CoreType => &self.core_type,
            Space::CoreFunc => &self.core_func,
            Space::CoreMemory => &self.core_memory,
            Space::CoreTable => &self.core_table,
            Space::CoreGlobal => &self.core_global,
            Space::CoreTag => &self.core_tag,
            Space::NA => return None,
        };
        Some(s)
    }

    pub(crate) fn lookup_actual_id_or_panic(&self, r: &IndexedRef) -> usize {
        *self
            .get_space(&r.space)
            .and_then(|space| space.lookup_actual_id(r.index as usize))
            .unwrap_or_else(|| {
                panic!(
                    "[{:?}] Internal error: Can't find assumed id {} in id-tracker",
                    r.space, r.index
                )
            })
    }
}

#[derive(Clone, Default)]
pub(crate) struct IdTracker {
    /// This is used at encode time. It tracks the actual ID that has been assigned
    /// to some item by allowing for lookup of the assumed ID: `assumed_id -> actual_id`
    /// This is important since we know what ID should be associated with something only at encode time,
    /// since instrumentation has finished at that point and encoding of component items
    /// can be done out-of-order to satisfy possible forward-references injected during instrumentation.
    actual_ids: HashMap<usize, usize>,

    /// This is the current ID that we've reached associated with this index space.
    current_id: usize,
}
impl IdTracker {
    pub fn curr_id(&self) -> usize {
        // This returns the ID that we've reached thus far while encoding
        self.current_id
    }

    pub fn assign_actual_id(&mut self, assumed_id: usize) {
        let id = self.curr_id();

        self.actual_ids.insert(assumed_id, id);
        self.next();
    }

    fn next(&mut self) -> usize {
        let curr = self.current_id;
        self.current_id += 1;
        curr
    }

    pub fn lookup_actual_id(&self, id: usize) -> Option<&usize> {
        self.actual_ids.get(&id)
    }
}
