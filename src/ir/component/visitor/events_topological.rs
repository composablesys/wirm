use crate::ir::component::idx_spaces::{IndexSpaceOf, Space, SpaceSubtype};
use crate::ir::component::refs::{RefKind, ReferencedIndices};
use crate::ir::component::scopes::GetScopeKind;
use crate::ir::component::section::ComponentSection;
use crate::ir::component::visitor::driver::VisitEvent;
use crate::ir::component::visitor::VisitCtx;
use crate::ir::types::CustomSection;
use crate::{Component, Module};
use std::collections::HashSet;
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance,
    ComponentStartFunction, ComponentType, ComponentTypeDeclaration, CoreType, Instance,
    InstanceTypeDeclaration, ModuleTypeDeclaration,
};

pub(crate) fn get_topological_events<'ir>(
    component: &'ir Component<'ir>,
    ctx: &mut VisitCtx<'ir>,
    out: &mut Vec<VisitEvent<'ir>>,
) {
    let mut topo = TopoCtx::default();

    ctx.inner.push_component(component);
    out.push(VisitEvent::enter_root_comp(component));

    // The root component is not declared in any enclosing section, so its own enter/exit
    // events have no parent section_idx. Collected items inherit their actual section
    // ordinals from the root's own section list, threaded through `collect_component`.
    topo.collect_component(component, None, None, ctx);
    out.extend(topo.events);

    out.push(VisitEvent::exit_root_comp(component));
    ctx.inner.pop_component();
}

#[derive(Default)]
struct TopoCtx<'ir> {
    seen: HashSet<NodeKey>,
    events: Vec<VisitEvent<'ir>>,
}
impl<'ir> TopoCtx<'ir> {
    /// `parent_section_idx` is the section ordinal in the *enclosing* component where this
    /// nested component was declared (used only for the EnterComp/ExitComp events). For the
    /// root component it is `None`. Items collected from `comp`'s own sections are tagged
    /// with section ordinals from `comp.sections`.
    fn collect_component(
        &mut self,
        comp: &'ir Component<'ir>,
        idx: Option<usize>,
        parent_section_idx: Option<usize>,
        ctx: &mut VisitCtx<'ir>,
    ) {
        let key = NodeKey::Component(id(comp));
        if !self.visit_once(key) {
            return;
        }

        if let Some(idx) = idx {
            // A nested component is always declared inside some Component section of the
            // parent, so `parent_section_idx` must be set when `idx` is set.
            let parent_section_idx =
                parent_section_idx.expect("nested component must have a parent section_idx");
            ctx.inner.push_component(comp);
            self.events
                .push(VisitEvent::enter_comp(parent_section_idx, idx, comp));
        }

        for (section_idx, (count, section)) in comp.sections.iter().enumerate() {
            let start_idx = ctx.inner.visit_section(section, *count as usize);
            self.collect_section_items(comp, section, section_idx, start_idx, *count as usize, ctx);
        }

        if let Some(idx) = idx {
            let parent_section_idx =
                parent_section_idx.expect("nested component must have a parent section_idx");
            ctx.inner.pop_component();
            self.events
                .push(VisitEvent::exit_comp(parent_section_idx, idx, comp));
        }
    }
    fn collect_module(
        &mut self,
        section_idx: usize,
        module: &'ir Module<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            module,
            NodeKey::Module(id(module)),
            ctx,
            None,
            VisitEvent::module(section_idx, module.index_space_of().into(), idx, module),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }
    fn collect_component_type(
        &mut self,
        section_idx: usize,
        node: &'ir ComponentType<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        let key = NodeKey::ComponentType(id(node));

        self.collect_node(
            node,
            key,
            ctx,
            Some(VisitEvent::enter_comp_type(
                section_idx,
                node.index_space_of().into(),
                idx,
                node,
            )),
            VisitEvent::exit_comp_type(section_idx, node.index_space_of().into(), idx, node),
            |this, node, ctx| {
                match node {
                    ComponentType::Component(decls) => {
                        for (i, item) in decls.iter().enumerate() {
                            this.collect_subitem(
                                decls,
                                item,
                                i,
                                NodeKey::component_type_decl,
                                |inner_this, item, i, cx| {
                                    inner_this.collect_component_type_decl(
                                        section_idx,
                                        node,
                                        item,
                                        i,
                                        cx,
                                    );
                                },
                                ctx,
                            );
                        }
                    }

                    ComponentType::Instance(decls) => {
                        for (i, item) in decls.iter().enumerate() {
                            this.collect_subitem(
                                decls,
                                item,
                                i,
                                NodeKey::inst_type_decl,
                                |inner_this, item, i, cx| {
                                    inner_this.collect_instance_type_decl(
                                        section_idx,
                                        node,
                                        item,
                                        i,
                                        cx,
                                    );
                                },
                                ctx,
                            );
                        }
                    }

                    // no sub-scoping for the below variants
                    ComponentType::Defined(_)
                    | ComponentType::Func(_)
                    | ComponentType::Resource { .. } => {}
                }
            },
        );
    }
    fn collect_component_type_decl(
        &mut self,
        section_idx: usize,
        parent: &'ir ComponentType<'ir>,
        decl: &'ir ComponentTypeDeclaration<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.events
            .push(VisitEvent::comp_type_decl(section_idx, parent, idx, decl));
        match decl {
            ComponentTypeDeclaration::Type(ty) => {
                self.collect_component_type(section_idx, ty, idx, ctx)
            }
            ComponentTypeDeclaration::CoreType(ty) => {
                self.collect_core_type(section_idx, ty, idx, ctx)
            }
            ComponentTypeDeclaration::Alias(_)
            | ComponentTypeDeclaration::Export { .. }
            | ComponentTypeDeclaration::Import(_) => {}
        }
    }
    fn collect_instance_type_decl(
        &mut self,
        section_idx: usize,
        parent: &'ir ComponentType<'ir>,
        decl: &'ir InstanceTypeDeclaration<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.events
            .push(VisitEvent::inst_type_decl(section_idx, parent, idx, decl));
        match decl {
            InstanceTypeDeclaration::Type(ty) => {
                self.collect_component_type(section_idx, ty, idx, ctx)
            }
            InstanceTypeDeclaration::CoreType(ty) => {
                self.collect_core_type(section_idx, ty, idx, ctx)
            }
            InstanceTypeDeclaration::Alias(_) | InstanceTypeDeclaration::Export { .. } => {}
        }
    }
    fn collect_comp_inst(
        &mut self,
        section_idx: usize,
        inst: &'ir ComponentInstance<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            inst,
            NodeKey::ComponentInstance(id(inst)),
            ctx,
            None,
            VisitEvent::comp_inst(section_idx, inst.index_space_of().into(), idx, inst),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }
    fn collect_core_inst(
        &mut self,
        section_idx: usize,
        inst: &'ir Instance<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            inst,
            NodeKey::CoreInst(id(inst)),
            ctx,
            None,
            VisitEvent::core_inst(section_idx, inst.index_space_of().into(), idx, inst),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }

    fn collect_core_type(
        &mut self,
        section_idx: usize,
        node: &'ir CoreType<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        let key = NodeKey::CoreType(id(node));

        let (enter_evt, exit_evt) = if let CoreType::Rec(group) = node {
            (
                VisitEvent::enter_rec_group(section_idx, group.types().len(), node),
                VisitEvent::exit_rec_group(section_idx),
            )
        } else {
            (
                VisitEvent::enter_core_type(section_idx, node.index_space_of().into(), idx, node),
                VisitEvent::exit_core_type(section_idx, node.index_space_of().into(), idx, node),
            )
        };

        self.collect_node(
            node,
            key,
            ctx,
            Some(enter_evt),
            exit_evt,
            |this, node, ctx| {
                match node {
                    CoreType::Module(decls) => {
                        for (i, item) in decls.iter().enumerate() {
                            this.collect_subitem(
                                decls,
                                item,
                                i,
                                NodeKey::module_type_decl,
                                |inner_this, item, i, cx| {
                                    inner_this.collect_module_type_decl(
                                        section_idx,
                                        node,
                                        item,
                                        i,
                                        cx,
                                    );
                                },
                                ctx,
                            );
                        }
                    }

                    // no sub-scoping for the below variant
                    CoreType::Rec(group) => {
                        for (subvec_idx, item) in group.types().enumerate() {
                            this.events.push(VisitEvent::core_subtype(
                                section_idx,
                                idx,
                                subvec_idx,
                                item,
                            ));
                        }
                    }
                }
            },
        );
    }
    fn collect_module_type_decl(
        &mut self,
        section_idx: usize,
        parent: &'ir CoreType<'ir>,
        decl: &'ir ModuleTypeDeclaration<'ir>,
        idx: usize,
        _: &mut VisitCtx<'ir>,
    ) {
        self.events
            .push(VisitEvent::mod_type_decl(section_idx, parent, idx, decl))
    }
    fn collect_canon(
        &mut self,
        section_idx: usize,
        canon: &'ir CanonicalFunction,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            canon,
            NodeKey::Canon(id(canon)),
            ctx,
            None,
            VisitEvent::canon(section_idx, canon.index_space_of().into(), idx, canon),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }
    fn collect_export(
        &mut self,
        section_idx: usize,
        export: &'ir ComponentExport<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            export,
            NodeKey::Export(id(export)),
            ctx,
            None,
            VisitEvent::export(section_idx, export.index_space_of().into(), idx, export),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }
    fn collect_import(
        &mut self,
        section_idx: usize,
        import: &'ir ComponentImport<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            import,
            NodeKey::Import(id(import)),
            ctx,
            None,
            VisitEvent::import(section_idx, import.index_space_of().into(), idx, import),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }
    fn collect_alias(
        &mut self,
        section_idx: usize,
        alias: &'ir ComponentAlias<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            alias,
            NodeKey::Alias(id(alias)),
            ctx,
            None,
            VisitEvent::alias(section_idx, alias.index_space_of().into(), idx, alias),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }
    fn collect_custom_section(
        &mut self,
        section_idx: usize,
        sect: &'ir CustomSection<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            sect,
            NodeKey::Custom(id(sect)),
            ctx,
            None,
            VisitEvent::custom_sect(section_idx, sect.index_space_of().into(), idx, sect),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }
    fn collect_start_section(
        &mut self,
        section_idx: usize,
        func: &'ir ComponentStartFunction,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            func,
            NodeKey::Start(id(func)),
            ctx,
            None,
            VisitEvent::start_func(section_idx, func.index_space_of().into(), idx, func),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }

    fn collect_section_items(
        &mut self,
        comp: &'ir Component<'ir>,
        section: &ComponentSection,
        section_idx: usize,
        start_idx: usize,
        count: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        for i in 0..count {
            let idx = start_idx + i;

            match section {
                ComponentSection::Component => {
                    self.collect_component(&comp.components[idx], Some(idx), Some(section_idx), ctx)
                }

                ComponentSection::Module => {
                    self.collect_module(section_idx, &comp.modules[idx], idx, ctx)
                }

                ComponentSection::ComponentType => self.collect_component_type(
                    section_idx,
                    &comp.component_types.items[idx],
                    idx,
                    ctx,
                ),

                ComponentSection::ComponentInstance => {
                    self.collect_comp_inst(section_idx, &comp.component_instance[idx], idx, ctx)
                }

                ComponentSection::Canon => {
                    self.collect_canon(section_idx, &comp.canons.items[idx], idx, ctx)
                }

                ComponentSection::Alias => {
                    self.collect_alias(section_idx, &comp.alias.items[idx], idx, ctx)
                }

                ComponentSection::ComponentImport => {
                    self.collect_import(section_idx, &comp.imports[idx], idx, ctx)
                }

                ComponentSection::ComponentExport => {
                    self.collect_export(section_idx, &comp.exports[idx], idx, ctx)
                }

                ComponentSection::CoreType => {
                    self.collect_core_type(section_idx, &comp.core_types[idx], idx, ctx)
                }

                ComponentSection::CoreInstance => {
                    self.collect_core_inst(section_idx, &comp.instances[idx], idx, ctx)
                }

                ComponentSection::CustomSection => self.collect_custom_section(
                    section_idx,
                    &comp.custom_sections.custom_sections[idx],
                    idx,
                    ctx,
                ),

                ComponentSection::ComponentStartSection => {
                    self.collect_start_section(section_idx, &comp.start_section[idx], idx, ctx)
                }
            }
        }
    }

    fn collect_node<T>(
        &mut self,
        node: &'ir T,
        key: NodeKey,
        ctx: &mut VisitCtx<'ir>,
        enter_event: Option<VisitEvent<'ir>>,
        exit_event: VisitEvent<'ir>,
        walk: impl FnOnce(&mut Self, &'ir T, &mut VisitCtx<'ir>),
    ) where
        T: GetScopeKind + ReferencedIndices + 'ir,
    {
        if !self.visit_once(key) {
            return;
        }

        if let Some(evt) = enter_event {
            self.events.push(evt)
        }

        // walk inner declarations
        ctx.inner.maybe_enter_scope(node);
        walk(self, node, ctx);
        ctx.inner.maybe_exit_scope(node);

        self.events.push(exit_event);
    }
    /// Walks an item's outgoing references and topologically collects each dependency.
    /// Each dep lives in its own section in `referenced_comp` — we look that up via
    /// [`section_idx_for_main_vec`] and [`section_idx_of_kth_item`] so the queued event
    /// carries the correct ordinal even when the dep crosses an outer-component boundary.
    fn collect_deps<T: ReferencedIndices + 'ir>(&mut self, item: &'ir T, ctx: &mut VisitCtx<'ir>) {
        let refs = item.referenced_indices();
        for RefKind { ref_, .. } in refs.iter() {
            let (vec, idx, subidx) = ctx.inner.index_from_assumed_id(ref_);
            if ref_.space != Space::CoreType {
                assert!(
                    subidx.is_none(),
                    "only core types (with rec groups) should ever have subvec indices!"
                );
            }

            let comp_id = ctx.inner.comp_at(ref_.depth);
            let referenced_comp = ctx.inner.comp_store.get(comp_id);

            let space = ref_.space;
            match vec {
                SpaceSubtype::Main => match space {
                    Space::Comp => {
                        let dep_section = section_idx_for_main_vec(referenced_comp, space, idx);
                        self.collect_component(
                            &referenced_comp.components[idx],
                            Some(idx),
                            Some(dep_section),
                            ctx,
                        )
                    }
                    Space::CompType => {
                        let dep_section = section_idx_for_main_vec(referenced_comp, space, idx);
                        self.collect_component_type(
                            dep_section,
                            &referenced_comp.component_types.items[idx],
                            idx,
                            ctx,
                        )
                    }
                    Space::CompInst => {
                        let dep_section = section_idx_for_main_vec(referenced_comp, space, idx);
                        self.collect_comp_inst(
                            dep_section,
                            &referenced_comp.component_instance[idx],
                            idx,
                            ctx,
                        )
                    }
                    Space::CoreInst => {
                        let dep_section = section_idx_for_main_vec(referenced_comp, space, idx);
                        self.collect_core_inst(
                            dep_section,
                            &referenced_comp.instances[idx],
                            idx,
                            ctx,
                        )
                    }
                    Space::CoreModule => {
                        let dep_section = section_idx_for_main_vec(referenced_comp, space, idx);
                        self.collect_module(dep_section, &referenced_comp.modules[idx], idx, ctx)
                    }
                    Space::CoreType => {
                        let dep_section = section_idx_for_main_vec(referenced_comp, space, idx);
                        self.collect_core_type(
                            dep_section,
                            &referenced_comp.core_types[idx],
                            idx,
                            ctx,
                        )
                    }
                    Space::CompFunc | Space::CoreFunc => {
                        let dep_section = section_idx_for_main_vec(referenced_comp, space, idx);
                        self.collect_canon(
                            dep_section,
                            &referenced_comp.canons.items[idx],
                            idx,
                            ctx,
                        )
                    }
                    Space::CompVal => {
                        // CompVal lands in the main subvec only when the
                        // value was produced by a start section's result —
                        // imports/exports/aliases route through their own
                        // subtypes. Schedule the start as the dep so it's
                        // emitted (and its actual ids are assigned) before
                        // anything that references the result.
                        let dep_section = section_idx_of_kth_item(
                            referenced_comp,
                            ComponentSection::ComponentStartSection,
                            idx,
                        );
                        self.collect_start_section(
                            dep_section,
                            &referenced_comp.start_section[idx],
                            idx,
                            ctx,
                        )
                    }
                    Space::CoreMemory
                    | Space::CoreTable
                    | Space::CoreGlobal
                    | Space::CoreTag
                    | Space::NA => unreachable!(
                        "This spaces don't exist in a main vector on the component IR: {vec:?}"
                    ),
                },
                SpaceSubtype::Export => {
                    let dep_section = section_idx_of_kth_item(
                        referenced_comp,
                        ComponentSection::ComponentExport,
                        idx,
                    );
                    self.collect_export(dep_section, &referenced_comp.exports[idx], idx, ctx)
                }
                SpaceSubtype::Import => {
                    let dep_section = section_idx_of_kth_item(
                        referenced_comp,
                        ComponentSection::ComponentImport,
                        idx,
                    );
                    self.collect_import(dep_section, &referenced_comp.imports[idx], idx, ctx)
                }
                SpaceSubtype::Alias => {
                    let dep_section =
                        section_idx_of_kth_item(referenced_comp, ComponentSection::Alias, idx);
                    self.collect_alias(dep_section, &referenced_comp.alias.items[idx], idx, ctx)
                }
            }
        }
    }

    /// Walk the deps of a sub-decl item and emit it. The enclosing section_idx for the
    /// queued events is captured by the `emit_item` closure at the call site, so it isn't
    /// passed as a parameter here.
    fn collect_subitem<T: ReferencedIndices + GetScopeKind + 'ir>(
        &mut self,
        all: &'ir [T],
        item: &'ir T,
        item_idx: usize,
        gen_key: fn(&T, usize) -> NodeKey,
        mut emit_item: impl FnMut(&mut Self, &'ir T, usize, &mut VisitCtx<'ir>),
        ctx: &mut VisitCtx<'ir>,
    ) {
        if !self.visit_once(gen_key(item, item_idx)) {
            return;
        }

        // collect the dependencies of this guy
        ctx.inner.maybe_enter_scope(item);
        let refs = item.referenced_indices();
        for RefKind { ref_, .. } in refs.iter() {
            if !ref_.depth.is_curr() {
                continue;
            }
            let (vec, idx, ..) = ctx.inner.index_from_assumed_id(ref_);
            assert_eq!(vec, SpaceSubtype::Main);
            let dep_item = &all[idx];

            if !self.visit_once(gen_key(dep_item, idx)) {
                continue;
            }

            // collect subitem
            emit_item(self, dep_item, idx, ctx);
        }

        ctx.inner.maybe_exit_scope(item);

        // collect item
        emit_item(self, item, item_idx, ctx);
    }
    fn visit_once(&mut self, key: NodeKey) -> bool {
        self.seen.insert(key)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum NodeKey {
    Component(*const ()),
    Module(*const ()),
    ComponentType(*const ()),
    ComponentTypeDecl(*const (), usize), // decl ptr + index
    InstanceTypeDecl(*const (), usize),  // decl ptr + index
    CoreType(*const ()),
    ModuleTypeDecl(*const (), usize), // decl ptr + index
    ComponentInstance(*const ()),
    CoreInst(*const ()),
    Alias(*const ()),
    Import(*const ()),
    Export(*const ()),
    Canon(*const ()),
    Custom(*const ()),
    Start(*const ()),
}
impl NodeKey {
    fn inst_type_decl(decl: &InstanceTypeDeclaration, idx: usize) -> Self {
        Self::InstanceTypeDecl(id(decl), idx)
    }
    fn component_type_decl(decl: &ComponentTypeDeclaration, idx: usize) -> Self {
        Self::ComponentTypeDecl(id(decl), idx)
    }
    fn module_type_decl(decl: &ModuleTypeDeclaration, idx: usize) -> Self {
        Self::ModuleTypeDecl(id(decl), idx)
    }
}

fn id<T>(ptr: &T) -> *const () {
    ptr as *const T as *const ()
}

/// Map a `(space, vec_idx)` pair into the section ordinal where that item was declared.
///
/// The wirm IR stores items from each kind of section in a single per-kind vector
/// (e.g. all `ComponentType` section items end up in `comp.component_types.items`),
/// and `vec_idx` is the position within that vector. To find which section produced
/// a given item, we walk `comp.sections` in order, accumulating the per-section item
/// counts for the matching section kind, and return the section ordinal where the
/// running count first exceeds `vec_idx`.
///
/// This is only valid for spaces that have a single corresponding section kind in the
/// IR's main vectors. Spaces produced by imports / aliases / exports are looked up via
/// [`section_idx_of_kth_item`] directly.
fn section_idx_for_main_vec(comp: &Component, space: Space, vec_idx: usize) -> usize {
    let target = match space {
        Space::Comp => ComponentSection::Component,
        Space::CompType => ComponentSection::ComponentType,
        Space::CompInst => ComponentSection::ComponentInstance,
        Space::CoreInst => ComponentSection::CoreInstance,
        Space::CoreModule => ComponentSection::Module,
        Space::CoreType => ComponentSection::CoreType,
        Space::CompFunc | Space::CoreFunc => ComponentSection::Canon,
        Space::CompVal
        | Space::CoreMemory
        | Space::CoreTable
        | Space::CoreGlobal
        | Space::CoreTag
        | Space::NA => {
            panic!("section_idx_for_main_vec: space {space:?} has no main vector in the IR")
        }
    };
    section_idx_of_kth_item(comp, target, vec_idx)
}

/// Walk `comp.sections` in order and return the section ordinal of the section that
/// holds the `vec_idx`-th item among all sections of the given kind. Panics if the
/// component has fewer than `vec_idx + 1` items of that kind, since that indicates a
/// bug in the caller's index resolution.
fn section_idx_of_kth_item(comp: &Component, target: ComponentSection, vec_idx: usize) -> usize {
    let mut cumulative = 0usize;
    for (section_idx, (num, section)) in comp.sections.iter().enumerate() {
        if std::mem::discriminant(section) == std::mem::discriminant(&target) {
            let new_cum = cumulative + (*num as usize);
            if vec_idx < new_cum {
                return section_idx;
            }
            cumulative = new_cum;
        }
    }
    panic!(
        "section_idx_of_kth_item: vec_idx {vec_idx} not found in any {target:?} section \
         (component has only {cumulative} items of that kind)"
    );
}
