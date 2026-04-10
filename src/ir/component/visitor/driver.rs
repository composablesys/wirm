use crate::ir::component::idx_spaces::{IndexSpaceOf, Space};
use crate::ir::component::section::ComponentSection;
use crate::ir::component::visitor::utils::TypeBodyDecls;
use crate::ir::component::visitor::{ComponentVisitor, ItemKind, VisitCtx};
use crate::ir::types::CustomSection;
use crate::{Component, Module};
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance,
    ComponentStartFunction, ComponentType, ComponentTypeDeclaration, CoreType, Instance,
    InstanceTypeDeclaration, ModuleTypeDeclaration, SubType,
};

pub fn drive_event<'ir, V: ComponentVisitor<'ir>>(
    event: &VisitEvent<'ir>,
    visitor: &mut V,
    ctx: &mut VisitCtx<'ir>,
) {
    // Make the enclosing top-level section ordinal of the event we're about to drive
    // available to visitor callbacks via `VisitCtx::curr_section_idx()`. The ordinal is
    // recorded by the structural / topological walkers when they queue each event.
    // Root component enter/exit events are not associated with any section and yield `None`.
    ctx.inner.current_section_idx = event.section_idx();
    match event {
        VisitEvent::EnterRootComp { component } => {
            ctx.inner.push_component(component);
            visitor.enter_root_component(ctx, component);
        }

        VisitEvent::ExitRootComp { component } => {
            visitor.exit_root_component(ctx, component);
            ctx.inner.pop_component();
        }
        VisitEvent::EnterComp { component, idx, .. } => {
            ctx.inner.push_component(component);
            let id = ctx
                .inner
                .lookup_id_for(&Space::Comp, &ComponentSection::Component, *idx);
            visitor.enter_component(ctx, id, component);
        }

        VisitEvent::ExitComp { component, idx, .. } => {
            let id = ctx
                .inner
                .lookup_id_for(&Space::Comp, &ComponentSection::Component, *idx);
            visitor.exit_component(ctx, id, component);
            ctx.inner.pop_component();
        }

        VisitEvent::Module { idx, module, .. } => {
            ctx.inner.maybe_enter_scope(*module);
            let id = ctx
                .inner
                .lookup_id_for(&Space::CoreModule, &ComponentSection::Module, *idx);
            visitor.visit_module(ctx, id, module);
            ctx.inner.maybe_exit_scope(*module);
        }

        VisitEvent::CompInst { idx, inst, .. } => {
            ctx.inner.maybe_enter_scope(*inst);
            let id = ctx.inner.lookup_id_for(
                &Space::CompInst,
                &ComponentSection::ComponentInstance,
                *idx,
            );
            visitor.visit_comp_instance(ctx, id, inst);
            ctx.inner.maybe_exit_scope(*inst);
        }

        VisitEvent::EnterCompType { idx, ty, .. } => {
            ctx.inner.maybe_enter_scope(*ty);
            let id =
                ctx.inner
                    .lookup_id_for(&Space::CompType, &ComponentSection::ComponentType, *idx);
            match ty {
                ComponentType::Instance(decls) => {
                    ctx.inner.push_type_body(TypeBodyDecls::Inst(decls));
                    visitor.enter_component_type_inst(ctx, id, ty);
                }
                ComponentType::Component(decls) => {
                    ctx.inner.push_type_body(TypeBodyDecls::Comp(decls));
                    visitor.enter_component_type_comp(ctx, id, ty);
                }
                _ => visitor.visit_comp_type(ctx, id, ty),
            }
        }

        VisitEvent::CompTypeDecl {
            idx, parent, decl, ..
        } => {
            ctx.inner.maybe_enter_scope(*decl);
            let id = ctx.inner.lookup_id_for(
                &decl.index_space_of(),
                &ComponentSection::ComponentType,
                *idx,
            );
            visitor.visit_comp_type_decl(ctx, *idx, id, parent, decl);
            ctx.inner.maybe_exit_scope(*decl);
        }

        VisitEvent::InstTypeDecl {
            idx, parent, decl, ..
        } => {
            ctx.inner.maybe_enter_scope(*decl);
            let id = ctx.inner.lookup_id_for(
                &decl.index_space_of(),
                &ComponentSection::ComponentType,
                *idx,
            );
            visitor.visit_inst_type_decl(ctx, *idx, id, parent, decl);
            ctx.inner.maybe_exit_scope(*decl);
        }

        VisitEvent::ExitCompType { idx, ty, .. } => {
            let id =
                ctx.inner
                    .lookup_id_for(&Space::CompType, &ComponentSection::ComponentType, *idx);
            match ty {
                ComponentType::Instance(_) => {
                    visitor.exit_component_type_inst(ctx, id, ty);
                    ctx.inner.pop_type_body();
                }
                ComponentType::Component(_) => {
                    visitor.exit_component_type_comp(ctx, id, ty);
                    ctx.inner.pop_type_body();
                }
                _ => {} // visit_comp_type was already called at Enter; no exit for leaf types
            }
            ctx.inner.maybe_exit_scope(*ty);
        }

        VisitEvent::Canon {
            kind, idx, canon, ..
        } => {
            ctx.inner.maybe_enter_scope(*canon);
            let space = canon.index_space_of();
            let id = ctx
                .inner
                .lookup_id_for(&space, &ComponentSection::Canon, *idx);
            visitor.visit_canon(ctx, *kind, id, canon);
            ctx.inner.maybe_exit_scope(*canon);
        }
        VisitEvent::Alias {
            kind, idx, alias, ..
        } => {
            ctx.inner.maybe_enter_scope(*alias);
            let space = alias.index_space_of();
            let id = ctx
                .inner
                .lookup_id_for(&space, &ComponentSection::Alias, *idx);
            visitor.visit_alias(ctx, *kind, id, alias);
            ctx.inner.maybe_exit_scope(*alias);
        }
        VisitEvent::Import {
            kind, idx, imp, ..
        } => {
            ctx.inner.maybe_enter_scope(*imp);
            let space = imp.index_space_of();
            let id = ctx
                .inner
                .lookup_id_for(&space, &ComponentSection::ComponentImport, *idx);
            visitor.visit_comp_import(ctx, *kind, id, imp);
            ctx.inner.maybe_exit_scope(*imp);
        }
        VisitEvent::Export {
            kind, idx, exp, ..
        } => {
            ctx.inner.maybe_enter_scope(*exp);
            let space = exp.index_space_of();
            let id = ctx
                .inner
                .lookup_id_for(&space, &ComponentSection::ComponentExport, *idx);
            visitor.visit_comp_export(ctx, *kind, id, exp);
            ctx.inner.maybe_exit_scope(*exp);
        }
        VisitEvent::EnterCoreRecGroup { count, ty, .. } => {
            visitor.enter_core_rec_group(ctx, *count, ty);
        }
        VisitEvent::CoreSubtype {
            parent_idx,
            subvec_idx,
            subtype,
            ..
        } => {
            ctx.inner.maybe_enter_scope(*subtype);
            let id = ctx.inner.lookup_id_with_subvec_for(
                &Space::CoreType,
                &ComponentSection::CoreType,
                *parent_idx,
                *subvec_idx,
            );
            visitor.visit_core_subtype(ctx, id, subtype);
            ctx.inner.maybe_exit_scope(*subtype);
        }
        VisitEvent::ExitCoreRecGroup { .. } => {
            visitor.exit_core_rec_group(ctx);
        }
        VisitEvent::EnterCoreType { idx, ty, .. } => {
            ctx.inner.maybe_enter_scope(*ty);
            let id = ctx
                .inner
                .lookup_id_for(&Space::CoreType, &ComponentSection::CoreType, *idx);
            if let CoreType::Module(decls) = ty {
                ctx.inner.push_type_body(TypeBodyDecls::Module(decls));
            }
            visitor.enter_core_module_type(ctx, id, ty);
        }
        VisitEvent::ModuleTypeDecl {
            idx, parent, decl, ..
        } => {
            ctx.inner.maybe_enter_scope(*decl);
            let id =
                ctx.inner
                    .lookup_id_for(&decl.index_space_of(), &ComponentSection::CoreType, *idx);
            visitor.visit_module_type_decl(ctx, *idx, id, parent, decl);
            ctx.inner.maybe_exit_scope(*decl);
        }
        VisitEvent::ExitCoreType { idx, ty, .. } => {
            let id = ctx
                .inner
                .lookup_id_for(&Space::CoreType, &ComponentSection::CoreType, *idx);
            visitor.exit_core_module_type(ctx, id, ty);
            ctx.inner.pop_type_body();
            ctx.inner.maybe_exit_scope(*ty);
        }
        VisitEvent::CoreInst { idx, inst, .. } => {
            ctx.inner.maybe_enter_scope(*inst);
            let id =
                ctx.inner
                    .lookup_id_for(&Space::CoreInst, &ComponentSection::CoreInstance, *idx);
            visitor.visit_core_instance(ctx, id, inst);
            ctx.inner.maybe_exit_scope(*inst);
        }
        VisitEvent::CustomSection { sect, .. } => {
            ctx.inner.maybe_enter_scope(*sect);
            visitor.visit_custom_section(ctx, sect);
            ctx.inner.maybe_exit_scope(*sect);
        }
        VisitEvent::StartFunc { func, .. } => {
            ctx.inner.maybe_enter_scope(*func);
            visitor.visit_start_section(ctx, func);
            ctx.inner.maybe_exit_scope(*func);
        }
    }
}

#[derive(Debug)]
pub(crate) enum VisitEvent<'ir> {
    EnterRootComp {
        component: &'ir Component<'ir>,
    },
    ExitRootComp {
        component: &'ir Component<'ir>,
    },
    EnterComp {
        section_idx: usize,
        idx: usize,
        component: &'ir Component<'ir>,
    },
    ExitComp {
        section_idx: usize,
        idx: usize,
        component: &'ir Component<'ir>,
    },
    Module {
        section_idx: usize,
        idx: usize,
        module: &'ir Module<'ir>,
    },

    // ------------------------
    // Component-level items
    // ------------------------
    EnterCompType {
        section_idx: usize,
        idx: usize,
        ty: &'ir ComponentType<'ir>,
    },
    ExitCompType {
        section_idx: usize,
        idx: usize,
        ty: &'ir ComponentType<'ir>,
    },
    // subitems of a component type
    CompTypeDecl {
        /// Section ordinal of the enclosing top-level `ComponentType` section.
        section_idx: usize,
        parent: &'ir ComponentType<'ir>,
        /// index in the decl vector
        idx: usize,
        decl: &'ir ComponentTypeDeclaration<'ir>,
    },
    InstTypeDecl {
        /// Section ordinal of the enclosing top-level `ComponentType` section.
        section_idx: usize,
        parent: &'ir ComponentType<'ir>,
        /// index in the decl vector
        idx: usize,
        decl: &'ir InstanceTypeDeclaration<'ir>,
    },

    CompInst {
        section_idx: usize,
        idx: usize,
        inst: &'ir ComponentInstance<'ir>,
    },

    // ------------------------------------------------
    // Items with multiple possible resolved namespaces
    // ------------------------------------------------
    Canon {
        section_idx: usize,
        kind: ItemKind,
        idx: usize,
        canon: &'ir CanonicalFunction,
    },
    Alias {
        section_idx: usize,
        kind: ItemKind,
        idx: usize,
        alias: &'ir ComponentAlias<'ir>,
    },
    Import {
        section_idx: usize,
        kind: ItemKind,
        idx: usize,
        imp: &'ir ComponentImport<'ir>,
    },
    Export {
        section_idx: usize,
        kind: ItemKind,
        idx: usize,
        exp: &'ir ComponentExport<'ir>,
    },

    // ------------------------
    // Core WebAssembly items
    // ------------------------
    EnterCoreRecGroup {
        /// Section ordinal of the enclosing top-level `CoreType` section.
        section_idx: usize,
        ty: &'ir CoreType<'ir>,
        count: usize,
    },
    CoreSubtype {
        /// Section ordinal of the enclosing top-level `CoreType` section.
        section_idx: usize,
        parent_idx: usize,
        subvec_idx: usize,
        subtype: &'ir SubType,
    },
    ExitCoreRecGroup {
        /// Section ordinal of the enclosing top-level `CoreType` section.
        section_idx: usize,
    },
    EnterCoreType {
        section_idx: usize,
        idx: usize,
        ty: &'ir CoreType<'ir>,
    },
    ModuleTypeDecl {
        /// Section ordinal of the enclosing top-level `CoreType` section.
        section_idx: usize,
        parent: &'ir CoreType<'ir>,
        /// index in the decl vector
        idx: usize,
        decl: &'ir ModuleTypeDeclaration<'ir>,
    },
    ExitCoreType {
        section_idx: usize,
        idx: usize,
        ty: &'ir CoreType<'ir>,
    },
    CoreInst {
        section_idx: usize,
        idx: usize,
        inst: &'ir Instance<'ir>,
    },

    // ------------------------
    // Sections
    // ------------------------
    CustomSection {
        section_idx: usize,
        sect: &'ir CustomSection<'ir>,
    },
    StartFunc {
        section_idx: usize,
        func: &'ir ComponentStartFunction,
    },
}
impl<'ir> VisitEvent<'ir> {
    /// Top-level section ordinal of the enclosing section for this event, relative to the
    /// immediately-containing component. Returns `None` for root-component enter/exit
    /// events, which are not emitted from inside any section.
    pub(crate) fn section_idx(&self) -> Option<usize> {
        match self {
            VisitEvent::EnterRootComp { .. } | VisitEvent::ExitRootComp { .. } => None,
            VisitEvent::EnterComp { section_idx, .. }
            | VisitEvent::ExitComp { section_idx, .. }
            | VisitEvent::Module { section_idx, .. }
            | VisitEvent::EnterCompType { section_idx, .. }
            | VisitEvent::ExitCompType { section_idx, .. }
            | VisitEvent::CompTypeDecl { section_idx, .. }
            | VisitEvent::InstTypeDecl { section_idx, .. }
            | VisitEvent::CompInst { section_idx, .. }
            | VisitEvent::Canon { section_idx, .. }
            | VisitEvent::Alias { section_idx, .. }
            | VisitEvent::Import { section_idx, .. }
            | VisitEvent::Export { section_idx, .. }
            | VisitEvent::EnterCoreRecGroup { section_idx, .. }
            | VisitEvent::CoreSubtype { section_idx, .. }
            | VisitEvent::ExitCoreRecGroup { section_idx, .. }
            | VisitEvent::EnterCoreType { section_idx, .. }
            | VisitEvent::ModuleTypeDecl { section_idx, .. }
            | VisitEvent::ExitCoreType { section_idx, .. }
            | VisitEvent::CoreInst { section_idx, .. }
            | VisitEvent::CustomSection { section_idx, .. }
            | VisitEvent::StartFunc { section_idx, .. } => Some(*section_idx),
        }
    }

    pub fn enter_root_comp(component: &'ir Component<'ir>) -> Self {
        Self::EnterRootComp { component }
    }
    pub fn exit_root_comp(component: &'ir Component<'ir>) -> Self {
        Self::ExitRootComp { component }
    }
    pub fn enter_comp(section_idx: usize, idx: usize, component: &'ir Component<'ir>) -> Self {
        Self::EnterComp {
            section_idx,
            idx,
            component,
        }
    }
    pub fn exit_comp(section_idx: usize, idx: usize, component: &'ir Component<'ir>) -> Self {
        Self::ExitComp {
            section_idx,
            idx,
            component,
        }
    }
    pub fn module(
        section_idx: usize,
        _: ItemKind,
        idx: usize,
        module: &'ir Module<'ir>,
    ) -> Self {
        Self::Module {
            section_idx,
            idx,
            module,
        }
    }
    pub fn enter_comp_type(
        section_idx: usize,
        _: ItemKind,
        idx: usize,
        ty: &'ir ComponentType<'ir>,
    ) -> Self {
        Self::EnterCompType {
            section_idx,
            idx,
            ty,
        }
    }
    pub fn comp_type_decl(
        section_idx: usize,
        parent: &'ir ComponentType<'ir>,
        idx: usize,
        decl: &'ir ComponentTypeDeclaration<'ir>,
    ) -> Self {
        Self::CompTypeDecl {
            section_idx,
            parent,
            idx,
            decl,
        }
    }
    pub fn inst_type_decl(
        section_idx: usize,
        parent: &'ir ComponentType<'ir>,
        idx: usize,
        decl: &'ir InstanceTypeDeclaration<'ir>,
    ) -> Self {
        Self::InstTypeDecl {
            section_idx,
            parent,
            idx,
            decl,
        }
    }
    pub fn exit_comp_type(
        section_idx: usize,
        _: ItemKind,
        idx: usize,
        ty: &'ir ComponentType<'ir>,
    ) -> Self {
        Self::ExitCompType {
            section_idx,
            idx,
            ty,
        }
    }
    pub fn comp_inst(
        section_idx: usize,
        _: ItemKind,
        idx: usize,
        inst: &'ir ComponentInstance<'ir>,
    ) -> Self {
        Self::CompInst {
            section_idx,
            idx,
            inst,
        }
    }
    pub fn canon(
        section_idx: usize,
        kind: ItemKind,
        idx: usize,
        canon: &'ir CanonicalFunction,
    ) -> Self {
        Self::Canon {
            section_idx,
            kind,
            idx,
            canon,
        }
    }
    pub fn alias(
        section_idx: usize,
        kind: ItemKind,
        idx: usize,
        alias: &'ir ComponentAlias<'ir>,
    ) -> Self {
        Self::Alias {
            section_idx,
            kind,
            idx,
            alias,
        }
    }
    pub fn import(
        section_idx: usize,
        kind: ItemKind,
        idx: usize,
        imp: &'ir ComponentImport<'ir>,
    ) -> Self {
        Self::Import {
            section_idx,
            kind,
            idx,
            imp,
        }
    }
    pub fn export(
        section_idx: usize,
        kind: ItemKind,
        idx: usize,
        exp: &'ir ComponentExport<'ir>,
    ) -> Self {
        Self::Export {
            section_idx,
            kind,
            idx,
            exp,
        }
    }
    pub fn enter_rec_group(section_idx: usize, count: usize, ty: &'ir CoreType<'ir>) -> Self {
        Self::EnterCoreRecGroup {
            section_idx,
            count,
            ty,
        }
    }
    pub fn core_subtype(
        section_idx: usize,
        parent_idx: usize,
        subvec_idx: usize,
        subtype: &'ir SubType,
    ) -> Self {
        Self::CoreSubtype {
            section_idx,
            parent_idx,
            subvec_idx,
            subtype,
        }
    }
    pub fn exit_rec_group(section_idx: usize) -> Self {
        Self::ExitCoreRecGroup { section_idx }
    }
    pub fn enter_core_type(
        section_idx: usize,
        _: ItemKind,
        idx: usize,
        ty: &'ir CoreType<'ir>,
    ) -> Self {
        Self::EnterCoreType {
            section_idx,
            idx,
            ty,
        }
    }
    pub fn mod_type_decl(
        section_idx: usize,
        parent: &'ir CoreType<'ir>,
        idx: usize,
        decl: &'ir ModuleTypeDeclaration<'ir>,
    ) -> Self {
        Self::ModuleTypeDecl {
            section_idx,
            parent,
            idx,
            decl,
        }
    }
    pub fn exit_core_type(
        section_idx: usize,
        _: ItemKind,
        idx: usize,
        ty: &'ir CoreType<'ir>,
    ) -> Self {
        Self::ExitCoreType {
            section_idx,
            idx,
            ty,
        }
    }
    pub fn core_inst(
        section_idx: usize,
        _: ItemKind,
        idx: usize,
        inst: &'ir Instance<'ir>,
    ) -> Self {
        Self::CoreInst {
            section_idx,
            idx,
            inst,
        }
    }
    pub fn custom_sect(
        section_idx: usize,
        _: ItemKind,
        _: usize,
        sect: &'ir CustomSection<'ir>,
    ) -> Self {
        Self::CustomSection { section_idx, sect }
    }
    pub fn start_func(
        section_idx: usize,
        _: ItemKind,
        _: usize,
        func: &'ir ComponentStartFunction,
    ) -> Self {
        Self::StartFunc { section_idx, func }
    }
}
