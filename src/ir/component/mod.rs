#![allow(clippy::too_many_arguments)]
//! Intermediate Representation of a wasm component.

use crate::encode::component::encode;
use crate::error::Error;
use crate::error::Error::IO;
use crate::ir::component::idx_spaces::{
    IndexSpaceOf, IndexStore, ScopeId, Space, SpaceSubtype, StoreHandle,
};
use crate::ir::component::nodes::{Aliases, Canons, ComponentTypes};
use crate::ir::component::refs::{GetItemRef, GetTypeRefs, IndexedRef};
use crate::ir::component::scopes::{IndexScopeRegistry, RegistryHandle};
use crate::ir::component::section::{
    get_sections_for_comp_ty, get_sections_for_core_ty_and_assign_top_level_ids,
    populate_space_for_comp_ty, populate_space_for_core_ty, ComponentSection,
};
use crate::ir::component::visitor::ResolvedItem;
use crate::ir::helpers::{
    print_alias, print_component_export, print_component_import, print_component_type,
    print_core_type,
};
use crate::ir::id::{
    AliasId, CanonFuncId, CompUniqueId, ComponentFunctionId, ComponentId, ComponentInstanceId,
    ComponentTypeId, CoreInstanceId, CoreTypeId, CustomSectionID, FunctionID, ModuleID, ValueID,
};
use crate::ir::module::Module;
use crate::ir::types::{CustomSection, CustomSections};
use crate::ir::AppendOnlyVec;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentExportName, ComponentExternalKind,
    ComponentImport, ComponentImportName, ComponentInstance, ComponentOuterAliasKind,
    ComponentStartFunction, ComponentType, ComponentTypeRef, ComponentValType, CoreType, Encoding,
    ExternalKind, Instance, NameMap, Parser, Payload, TypeBounds,
};

pub mod concrete;
pub mod idx_spaces;
mod nodes;
pub mod refs;
pub(crate) mod scopes;
pub(crate) mod section;
#[cfg(test)]
mod tests;
pub mod visitor;

#[derive(Debug)]
/// Intermediate Representation of a wasm component.
pub struct Component<'a> {
    /// Wirm-internal globally-unique identifier for this component, used as
    /// the HashMap key into the scope registry. Distinct from the
    /// component-index-space position (a `ComponentId`), which is
    /// parent-relative and only meaningful as a ref into a parent's
    /// `components` vector.
    pub(crate) uid: CompUniqueId,
    /// Nested Components
    // These have scopes, but the scopes are looked up by CompUniqueId
    pub components: AppendOnlyVec<Component<'a>>,
    /// Modules
    // These have scopes, but they aren't handled by component encoding logic
    pub modules: AppendOnlyVec<Module<'a>>,
    /// Component Types
    // These can have scopes and need to be looked up by a pointer to the IR node --> Box the value!
    pub component_types: ComponentTypes<'a>,
    /// Component Instances
    pub component_instance: AppendOnlyVec<ComponentInstance<'a>>,
    /// Canons
    pub canons: Canons,

    /// Alias
    pub alias: Aliases<'a>,
    /// Imports
    pub imports: AppendOnlyVec<ComponentImport<'a>>,
    /// Exports
    pub exports: AppendOnlyVec<ComponentExport<'a>>,

    /// Core Types
    // These can have scopes and need to be looked up by a pointer to the IR node --> Box the value!
    pub core_types: AppendOnlyVec<Box<CoreType<'a>>>,
    /// Core Instances
    pub instances: AppendOnlyVec<Instance<'a>>,

    // Tracks the index spaces of this component.
    pub(crate) space_id: ScopeId, // cached for quick lookup!
    pub(crate) scope_registry: RegistryHandle,
    pub(crate) index_store: StoreHandle,

    /// Custom sections
    pub custom_sections: CustomSections<'a>,
    /// Component Start Section
    pub start_section: AppendOnlyVec<ComponentStartFunction>,
    /// Sections of the Component. Represented as (#num of occurrences of a section, type of section)
    pub sections: Vec<(u32, ComponentSection)>,
    num_sections: usize,

    // Names
    pub(crate) component_name: Option<String>,
    pub(crate) core_func_names: Names,
    pub(crate) global_names: Names,
    pub(crate) memory_names: Names,
    pub(crate) tag_names: Names,
    pub(crate) table_names: Names,
    pub(crate) module_names: Names,
    pub(crate) core_instances_names: Names,
    pub(crate) core_type_names: Names,
    pub(crate) type_names: Names,
    pub(crate) instance_names: Names,
    pub(crate) components_names: Names,
    pub(crate) func_names: Names,
    pub(crate) value_names: Names,
}

impl<'a> Component<'a> {
    /// Emit the Component into a wasm binary file.
    pub fn emit_wasm(&self, file_name: &str) -> crate::ir::types::Result<()> {
        let wasm = self.encode()?;
        std::fs::write(file_name, wasm).map_err(IO)?;
        Ok(())
    }

    fn add_section_and_get_id(
        &mut self,
        space: Space,
        sect: ComponentSection,
        idx: usize,
    ) -> usize {
        // get and save off the assumed id
        let assumed_id =
            self.index_store
                .borrow_mut()
                .assign_assumed_id(&self.space_id, &space, &sect, idx);

        self.add_section(sect);

        assumed_id.unwrap_or(idx)
    }

    fn add_section(&mut self, sect: ComponentSection) {
        // add to section order list
        if self.num_sections > 0 && self.sections[self.num_sections - 1].1 == sect {
            self.sections[self.num_sections - 1].0 += 1;
        } else {
            self.sections.push((1, sect));
            self.num_sections += 1;
        }
    }

    /// Add a Module to this Component.
    pub fn add_module(&mut self, module: Module<'a>) -> ModuleID {
        let idx = self.modules.len();
        let id =
            self.add_section_and_get_id(module.index_space_of(), ComponentSection::Module, idx);
        self.modules.push(module);

        ModuleID(id as u32)
    }

    /// Allocate a sub-component scope: a fresh `ScopeId` plus a fresh
    /// `CompUniqueId`, both threaded into a returned tuple alongside cloned
    /// `Rc` handles to `self`'s scope registry and index store. Used by
    /// `add_component` and `add_component_from_bytes` to set up a sub-scope
    /// the same way the parser does for nested component sections.
    fn alloc_sub_scope(&mut self) -> (ScopeId, CompUniqueId, RegistryHandle, StoreHandle) {
        let sub_space_id = self.index_store.borrow_mut().new_scope();
        let sub_uid = self.scope_registry.borrow_mut().alloc_uid();
        (
            sub_space_id,
            sub_uid,
            Rc::clone(&self.scope_registry),
            Rc::clone(&self.index_store),
        )
    }

    /// Register a freshly-built sub-component: assigns its index in this
    /// component's component-index space, pushes it, and adds the section
    /// marker.
    fn register_sub_component(&mut self, sub: Component<'a>) -> ComponentId {
        let idx = self.components.len();
        let id =
            self.add_section_and_get_id(sub.index_space_of(), ComponentSection::Component, idx);
        self.components.push(sub);

        ComponentId(id as u32)
    }

    /// Parse `bytes` as a complete component binary and add the result as a
    /// sub-component of `self`. The new sub-component shares `self`'s
    /// scope registry and index store, so refs inside it (including
    /// `Outer`-scope aliases pointing back into `self`) resolve correctly
    /// against `self`'s items.
    pub fn add_component_from_bytes(&mut self, bytes: &'a [u8]) -> Result<ComponentId, Error> {
        let (sub_space_id, sub_uid, registry, store) = self.alloc_sub_scope();
        let sub = Self::parse_comp(
            bytes,
            false,
            false,
            Parser::new(0),
            0,
            sub_space_id,
            sub_uid,
            registry,
            store,
        )?;
        Ok(self.register_sub_component(sub))
    }

    /// Add an empty sub-component to `self`, populate it via the supplied
    /// builder closure, and register it. The sub-component shares `self`'s
    /// scope registry and index store, so the closure can use the full
    /// mutation API to add items that reference outer-scope (parent)
    /// items via `add_alias_outer` etc.
    pub fn add_component<F>(&mut self, f: F) -> ComponentId
    where
        F: FnOnce(&mut Component<'a>),
    {
        let (sub_space_id, sub_uid, registry, store) = self.alloc_sub_scope();
        let mut sub = Component {
            uid: sub_uid,
            components: AppendOnlyVec::default(),
            modules: AppendOnlyVec::default(),
            component_types: ComponentTypes::new(AppendOnlyVec::default()),
            component_instance: AppendOnlyVec::default(),
            canons: Canons::new(AppendOnlyVec::default()),
            alias: Aliases::new(AppendOnlyVec::default()),
            imports: AppendOnlyVec::default(),
            exports: AppendOnlyVec::default(),
            core_types: AppendOnlyVec::default(),
            instances: AppendOnlyVec::default(),
            space_id: sub_space_id,
            scope_registry: registry,
            index_store: store,
            custom_sections: CustomSections::new(vec![]),
            start_section: AppendOnlyVec::default(),
            sections: vec![],
            num_sections: 0,
            component_name: None,
            core_func_names: Names::default(),
            global_names: Names::default(),
            memory_names: Names::default(),
            tag_names: Names::default(),
            table_names: Names::default(),
            module_names: Names::default(),
            core_instances_names: Names::default(),
            core_type_names: Names::default(),
            type_names: Names::default(),
            instance_names: Names::default(),
            components_names: Names::default(),
            func_names: Names::default(),
            value_names: Names::default(),
        };
        sub.scope_registry
            .borrow_mut()
            .register_comp(sub_uid, sub_space_id);

        f(&mut sub);

        self.register_sub_component(sub)
    }

    pub fn add_custom_section(&mut self, section: CustomSection<'a>) -> CustomSectionID {
        let id = self.custom_sections.add(section);
        self.add_section(ComponentSection::CustomSection);

        id
    }

    // ── Imports ───────────────────────────────────────────────────────────
    //
    // Each per-kind helper constructs the right `ComponentTypeRef` variant
    // and returns the typed ID for whichever index space the imported item
    // lands in. The caller doesn't have to remember the kind/typed-ID
    // pairing — the method name encodes it.

    /// Internal: push a fully-constructed `ComponentImport` and return the
    /// raw index assigned in its target index space.
    fn add_import_internal(&mut self, import: ComponentImport<'a>) -> u32 {
        let idx = self.imports.len();
        let id = self.add_section_and_get_id(
            import.index_space_of(),
            ComponentSection::ComponentImport,
            idx,
        );
        self.imports.push(import);

        id as u32
    }

    /// Import a core module typed by the component type at `type_idx`.
    pub fn add_import_core_module(
        &mut self,
        name: ComponentImportName<'a>,
        type_idx: u32,
    ) -> ModuleID {
        let id = self.add_import_internal(ComponentImport {
            name,
            ty: ComponentTypeRef::Module(type_idx),
        });
        ModuleID(id)
    }

    /// Import a sub-component typed by the component type at `type_idx`.
    pub fn add_import_component(
        &mut self,
        name: ComponentImportName<'a>,
        type_idx: u32,
    ) -> ComponentId {
        let id = self.add_import_internal(ComponentImport {
            name,
            ty: ComponentTypeRef::Component(type_idx),
        });
        ComponentId(id)
    }

    /// Import a component instance typed by the component type at `type_idx`.
    pub fn add_import_component_instance(
        &mut self,
        name: ComponentImportName<'a>,
        type_idx: u32,
    ) -> ComponentInstanceId {
        let id = self.add_import_internal(ComponentImport {
            name,
            ty: ComponentTypeRef::Instance(type_idx),
        });
        ComponentInstanceId(id)
    }

    /// Import a component-level function typed by the component type at
    /// `type_idx`.
    pub fn add_import_component_func(
        &mut self,
        name: ComponentImportName<'a>,
        type_idx: u32,
    ) -> ComponentFunctionId {
        let id = self.add_import_internal(ComponentImport {
            name,
            ty: ComponentTypeRef::Func(type_idx),
        });
        ComponentFunctionId(id)
    }

    /// Import a component value of type `val_ty`.
    pub fn add_import_component_value(
        &mut self,
        name: ComponentImportName<'a>,
        val_ty: ComponentValType,
    ) -> ValueID {
        let id = self.add_import_internal(ComponentImport {
            name,
            ty: ComponentTypeRef::Value(val_ty),
        });
        ValueID(id)
    }

    /// Import a component type subject to `bounds`.
    pub fn add_import_component_type(
        &mut self,
        name: ComponentImportName<'a>,
        bounds: TypeBounds,
    ) -> ComponentTypeId {
        let id = self.add_import_internal(ComponentImport {
            name,
            ty: ComponentTypeRef::Type(bounds),
        });
        ComponentTypeId(id)
    }

    // ── Exports ───────────────────────────────────────────────────────────
    //
    // Each per-kind helper exports an item from its respective index space.
    // The returned typed ID is the *new* index assigned in that same space —
    // exporting re-binds the item under a public name and produces a fresh
    // entry in the index space.

    /// Internal: push a fully-constructed `ComponentExport` and return the
    /// raw index assigned in its target index space.
    fn add_export_internal(&mut self, export: ComponentExport<'a>) -> u32 {
        let idx = self.exports.len();
        let id = self.add_section_and_get_id(
            export.index_space_of(),
            ComponentSection::ComponentExport,
            idx,
        );
        self.exports.push(export);

        id as u32
    }

    /// Export the core module at `idx` under `name`.
    pub fn add_export_core_module(
        &mut self,
        name: ComponentExportName<'a>,
        idx: ModuleID,
        ty: Option<ComponentTypeRef>,
    ) -> ModuleID {
        let id = self.add_export_internal(ComponentExport {
            name,
            kind: ComponentExternalKind::Module,
            index: *idx,
            ty,
        });
        ModuleID(id)
    }

    /// Export the sub-component at `idx` under `name`.
    pub fn add_export_component(
        &mut self,
        name: ComponentExportName<'a>,
        idx: ComponentId,
        ty: Option<ComponentTypeRef>,
    ) -> ComponentId {
        let id = self.add_export_internal(ComponentExport {
            name,
            kind: ComponentExternalKind::Component,
            index: *idx,
            ty,
        });
        ComponentId(id)
    }

    /// Export the component instance at `idx` under `name`.
    pub fn add_export_component_instance(
        &mut self,
        name: ComponentExportName<'a>,
        idx: ComponentInstanceId,
        ty: Option<ComponentTypeRef>,
    ) -> ComponentInstanceId {
        let id = self.add_export_internal(ComponentExport {
            name,
            kind: ComponentExternalKind::Instance,
            index: *idx,
            ty,
        });
        ComponentInstanceId(id)
    }

    /// Export the component-level function at `idx` under `name`.
    pub fn add_export_component_func(
        &mut self,
        name: ComponentExportName<'a>,
        idx: ComponentFunctionId,
        ty: Option<ComponentTypeRef>,
    ) -> ComponentFunctionId {
        let id = self.add_export_internal(ComponentExport {
            name,
            kind: ComponentExternalKind::Func,
            index: *idx,
            ty,
        });
        ComponentFunctionId(id)
    }

    /// Export the component value at `idx` under `name`.
    pub fn add_export_component_value(
        &mut self,
        name: ComponentExportName<'a>,
        idx: ValueID,
        ty: Option<ComponentTypeRef>,
    ) -> ValueID {
        let id = self.add_export_internal(ComponentExport {
            name,
            kind: ComponentExternalKind::Value,
            index: *idx,
            ty,
        });
        ValueID(id)
    }

    /// Export the component type at `idx` under `name`.
    pub fn add_export_component_type(
        &mut self,
        name: ComponentExportName<'a>,
        idx: ComponentTypeId,
        ty: Option<ComponentTypeRef>,
    ) -> ComponentTypeId {
        let id = self.add_export_internal(ComponentExport {
            name,
            kind: ComponentExternalKind::Type,
            index: *idx,
            ty,
        });
        ComponentTypeId(id)
    }

    // ── Aliases ───────────────────────────────────────────────────────────
    //
    // One method per `ComponentAlias` variant. Each takes the kind plus the
    // identifying fields and returns an [`AliasId`] sum — the typed ID lands
    // in whichever index space the (variant, kind) pair targets, so the
    // return is genuinely polymorphic; callers use the matching `unwrap_*`
    // helper when they know the kind they passed. The space → variant
    // mapping lives in `AliasId::new`; here we just thread through.

    /// Internal: push a fully-constructed `ComponentAlias` and return the
    /// matching [`AliasId`].
    fn add_alias_internal(&mut self, alias: ComponentAlias<'a>) -> AliasId {
        let space = alias.index_space_of();
        let idx = self.alias.add(alias);
        let id = self.add_section_and_get_id(space, ComponentSection::Alias, idx);

        AliasId::new(space, id as u32)
    }

    /// Add an alias to an export of a component instance.
    pub fn add_alias_instance_export(
        &mut self,
        kind: ComponentExternalKind,
        instance_index: u32,
        name: &'a str,
    ) -> AliasId {
        self.add_alias_internal(ComponentAlias::InstanceExport {
            kind,
            instance_index,
            name,
        })
    }

    /// Add an alias to an export of a core instance.
    pub fn add_alias_core_instance_export(
        &mut self,
        kind: ExternalKind,
        instance_index: u32,
        name: &'a str,
    ) -> AliasId {
        self.add_alias_internal(ComponentAlias::CoreInstanceExport {
            kind,
            instance_index,
            name,
        })
    }

    /// Add an alias to an outer-scope item.
    pub fn add_alias_outer(
        &mut self,
        kind: ComponentOuterAliasKind,
        count: u32,
        index: u32,
    ) -> AliasId {
        self.add_alias_internal(ComponentAlias::Outer { kind, count, index })
    }

    /// Add a Canonical Function to this Component. Returns a [`CanonFuncId`]
    /// because the index space depends on the canon variant — `Lift` and
    /// thread-spawn variants land in the component function index space;
    /// `Lower`, resource ops, and most async/streams variants land in the
    /// core function index space. Use [`CanonFuncId::unwrap_component`] /
    /// [`CanonFuncId::unwrap_core`] when you know which variant you passed.
    pub fn add_canon_func(&mut self, canon: CanonicalFunction) -> CanonFuncId {
        let space = canon.index_space_of();
        let idx = self.canons.add(canon);
        let id = self.add_section_and_get_id(space, ComponentSection::Canon, idx);

        CanonFuncId::new(space, id as u32)
    }

    /// Add a component type to this Component. Returns the index assigned in
    /// the component-type index space. Caller chooses the variant
    /// (`Instance`, `Func`, `Defined`, `Component`, `Resource`) — the index
    /// space and return type are the same regardless.
    pub fn add_component_type(&mut self, component_ty: ComponentType<'a>) -> ComponentTypeId {
        let space = component_ty.index_space_of();
        let idx = self.component_types.add(component_ty);
        let id = self.add_section_and_get_id(space, ComponentSection::ComponentType, idx);

        // Handle the index space of this node
        populate_space_for_comp_ty(
            self.component_types.items.last().unwrap(),
            self.scope_registry.clone(),
            self.index_store.clone(),
        );

        ComponentTypeId(id as u32)
    }

    /// Add a core type to this Component. Returns the index assigned in the
    /// core-type index space. Caller chooses the variant (`Rec`, `Module`).
    pub fn add_core_type(&mut self, ty: CoreType<'a>) -> CoreTypeId {
        let space = ty.index_space_of();
        let idx = self.core_types.len();
        self.core_types.push(Box::new(ty));
        let id = self.add_section_and_get_id(space, ComponentSection::CoreType, idx);

        // Handle the index space of this node (e.g. ModuleType decls).
        populate_space_for_core_ty(
            self.core_types.last().unwrap(),
            self.scope_registry.clone(),
            self.index_store.clone(),
        );

        CoreTypeId(id as u32)
    }

    /// Add a new core instance to this component.
    pub fn add_core_instance(&mut self, instance: Instance<'a>) -> CoreInstanceId {
        let idx = self.instances.len();
        let id = self.add_section_and_get_id(
            instance.index_space_of(),
            ComponentSection::CoreInstance,
            idx,
        );
        self.instances.push(instance);

        CoreInstanceId(id as u32)
    }

    /// Add a new component instance to this component.
    pub fn add_component_instance(
        &mut self,
        instance: ComponentInstance<'a>,
    ) -> ComponentInstanceId {
        let idx = self.component_instance.len();
        let id = self.add_section_and_get_id(
            instance.index_space_of(),
            ComponentSection::ComponentInstance,
            idx,
        );
        self.component_instance.push(instance);

        ComponentInstanceId(id as u32)
    }

    // Deferred until the parser-side start-result tracking bug is fixed (see
    // the TODO in `parse_comp`'s `Payload::ComponentStartSection` arm). Once
    // that lands, this should return `Vec<ValueID>` so callers can reference
    // the produced values; shipping it before the fix would either ignore
    // results (breaking calls that need them) or require a breaking signature
    // change later.
    //
    // /// Add a start section to this component. `results` declares the number
    // /// of values the start function produces — each becomes a new entry in
    // /// the component value index space.
    // pub fn add_start_section(
    //     &mut self,
    //     func: ComponentFunctionId,
    //     arguments: Vec<ValueID>,
    //     results: u32,
    // ) -> Vec<ValueID> {
    //     let arguments = arguments.iter().map(|v| **v).collect::<Vec<u32>>();
    //     self.start_section.push(ComponentStartFunction {
    //         func_index: *func,
    //         arguments: arguments.into_boxed_slice(),
    //         results,
    //     });
    //     self.add_section(ComponentSection::ComponentStartSection);
    //     // ... allocate `results` ValueIDs through the index store and return
    //     // them.
    //     todo!()
    // }

    fn add_to_sections(
        sections: &mut Vec<(u32, ComponentSection)>,
        new_sections: &[ComponentSection],
        num_sections: &mut usize,
        payload_kind: ComponentSection,
    ) {
        // Empty payload (e.g. an import section with count=0): still push a
        // `(0, kind)` entry so `section_idx` stays 1:1 with the binary's
        // section positions. Skipping would desync wasmparser-based consumers
        // on inputs that contain empty sections.
        if new_sections.is_empty() {
            sections.push((0, payload_kind));
            *num_sections += 1;
            return;
        }
        debug_assert!(
            new_sections.iter().all(|k| *k == payload_kind),
            "non-empty new_sections must all equal payload_kind"
        );
        // Group consecutive same-kind items within this call into one
        // `(count, kind)` entry. Do NOT fold across calls: one call == one
        // binary section, and collapsing would desync section ordinals from
        // wasmparser-based consumers.
        let mut i = 0usize;
        while i < new_sections.len() {
            let kind = new_sections[i].clone();
            let mut count = 1u32;
            while i + (count as usize) < new_sections.len()
                && new_sections[i + count as usize] == kind
            {
                count += 1;
            }
            sections.push((count, kind));
            *num_sections += 1;
            i += count as usize;
        }
    }

    /// Parse a `Component` from a wasm binary.
    ///
    /// Set enable_multi_memory to `true` to support parsing modules using multiple memories.
    /// Set with_offsets to `true` to save opcode pc offset metadata during parsing
    /// (can be used to determine the static pc offset inside a function body of the start of any opcode).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use wirm::Component;
    ///
    /// let file = "path_to_file";
    /// let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    /// let comp = Component::parse(&buff, false, false).unwrap();
    /// ```
    pub fn parse(
        wasm: &'_ [u8],
        enable_multi_memory: bool,
        with_offsets: bool,
    ) -> Result<Component<'_>, Error> {
        let parser = Parser::new(0);

        let mut registry = IndexScopeRegistry::default();
        let mut store = IndexStore::default();
        let space_id = store.new_scope();
        let root_uid = registry.alloc_uid();
        let res = Component::parse_comp(
            wasm,
            enable_multi_memory,
            with_offsets,
            parser,
            0,
            space_id,
            root_uid,
            Rc::new(RefCell::new(registry)),
            Rc::new(RefCell::new(store)),
        );
        res
    }

    fn parse_comp(
        wasm: &'a [u8],
        enable_multi_memory: bool,
        with_offsets: bool,
        parser: Parser,
        start: usize,
        space_id: ScopeId,
        my_uid: CompUniqueId,
        registry_handle: RegistryHandle,
        store_handle: StoreHandle,
    ) -> Result<Component<'a>, Error> {
        let mut modules = AppendOnlyVec::default();
        let mut core_types = AppendOnlyVec::default();
        let mut component_types = AppendOnlyVec::default();
        let mut imports = AppendOnlyVec::default();
        let mut exports = AppendOnlyVec::default();
        let mut instances = AppendOnlyVec::default();
        let mut canons = AppendOnlyVec::default();
        let mut alias = AppendOnlyVec::default();
        let mut component_instance = AppendOnlyVec::default();
        let mut components = AppendOnlyVec::default();
        let mut start_section = AppendOnlyVec::default();
        let mut custom_sections = vec![];

        let mut sections = vec![];
        let mut num_sections: usize = 0;
        let mut stack = vec![];

        // Names
        let mut component_name: Option<String> = None;
        let mut core_func_names = Names::default();
        let mut global_names = Names::default();
        let mut tag_names = Names::default();
        let mut memory_names = Names::default();
        let mut table_names = Names::default();
        let mut module_names = Names::default();
        let mut core_instances_names = Names::default();
        let mut instance_names = Names::default();
        let mut components_names = Names::default();
        let mut func_names = Names::default();
        let mut value_names = Names::default();
        let mut core_type_names = Names::default();
        let mut type_names = Names::default();

        for payload in parser.parse_all(wasm) {
            let payload = payload?;

            // `wasmparser::Parser::parse_all` yields payloads from every
            // nesting level via its own internal recursion. For subcomponents
            // and submodules we've already handed off to a separate recursive
            // `parse_comp` / `parse_internal` call, so we need to skip those
            // nested payloads here. The `stack` counter tracks how deep we
            // are inside a nested-but-already-processed section: every nested
            // `ComponentSection`/`ModuleSection` bumps it, every `End`
            // unbumps. Top-level `End` (stack empty) and the nested section
            // that we entered ourselves both fall through to the match.
            match &payload {
                Payload::End(_) if !stack.is_empty() => {
                    stack.pop();
                    continue;
                }
                Payload::ComponentSection { .. } | Payload::ModuleSection { .. }
                    if !stack.is_empty() =>
                {
                    stack.push(Encoding::Component); // marker; variant unused
                    continue;
                }
                _ if !stack.is_empty() => continue,
                _ => {}
            }

            match payload {
                Payload::ComponentImportSection(import_section_reader) => {
                    let mut temp: Vec<ComponentImport> = import_section_reader
                        .into_iter()
                        .collect::<Result<_, _>>()?;
                    let l = temp.len();
                    let new_sections = vec![ComponentSection::ComponentImport; l];
                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &temp,
                        imports.len(),
                        &new_sections,
                    );
                    imports.append(&mut temp);
                    Self::add_to_sections(
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        ComponentSection::ComponentImport,
                    );
                }
                Payload::ComponentExportSection(export_section_reader) => {
                    let mut temp: Vec<ComponentExport> = export_section_reader
                        .into_iter()
                        .collect::<Result<_, _>>()?;
                    let l = temp.len();
                    let new_sections = vec![ComponentSection::ComponentExport; l];
                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &temp,
                        exports.len(),
                        &new_sections,
                    );
                    exports.append(&mut temp);
                    Self::add_to_sections(
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        ComponentSection::ComponentExport,
                    );
                }
                Payload::InstanceSection(instance_section_reader) => {
                    let mut temp: Vec<Instance> = instance_section_reader
                        .into_iter()
                        .collect::<Result<_, _>>()?;
                    let l = temp.len();
                    let new_sections = vec![ComponentSection::CoreInstance; l];
                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &temp,
                        instances.len(),
                        &new_sections,
                    );
                    instances.append(&mut temp);
                    Self::add_to_sections(
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        ComponentSection::CoreInstance,
                    );
                }
                Payload::CoreTypeSection(core_type_reader) => {
                    let mut temp: Vec<Box<CoreType>> = core_type_reader
                        .into_iter()
                        .map(|res| res.map(Box::new))
                        .collect::<Result<_, _>>()?;

                    let old_len = core_types.len();
                    core_types.append(&mut temp);

                    let mut new_sects = vec![];
                    for (idx, ty) in core_types[old_len..].iter().enumerate() {
                        let (new_sect, _) = get_sections_for_core_ty_and_assign_top_level_ids(
                            ty,
                            old_len + idx,
                            &space_id,
                            store_handle.clone(),
                        );
                        new_sects.push(new_sect);
                    }

                    Self::add_to_sections(
                        &mut sections,
                        &new_sects,
                        &mut num_sections,
                        ComponentSection::CoreType,
                    );
                }
                Payload::ComponentTypeSection(component_type_reader) => {
                    let mut temp: Vec<Box<ComponentType>> = component_type_reader
                        .into_iter()
                        .map(|res| res.map(Box::new))
                        .collect::<Result<_, _>>()?;

                    let old_len = component_types.len();
                    component_types.append(&mut temp);

                    let mut new_sects = vec![];
                    for ty in &component_types[old_len..] {
                        let (new_sect, _) = get_sections_for_comp_ty(ty);
                        new_sects.push(new_sect);
                    }

                    store_handle.borrow_mut().assign_assumed_id_for_boxed(
                        &space_id,
                        &component_types[old_len..],
                        old_len,
                        &new_sects,
                    );
                    Self::add_to_sections(
                        &mut sections,
                        &new_sects,
                        &mut num_sections,
                        ComponentSection::ComponentType,
                    );
                }
                Payload::ComponentInstanceSection(component_instances) => {
                    let mut temp: Vec<ComponentInstance> =
                        component_instances.into_iter().collect::<Result<_, _>>()?;
                    let l = temp.len();
                    let new_sections = vec![ComponentSection::ComponentInstance; l];
                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &temp,
                        component_instance.len(),
                        &new_sections,
                    );
                    component_instance.append(&mut temp);
                    Self::add_to_sections(
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        ComponentSection::ComponentInstance,
                    );
                }
                Payload::ComponentAliasSection(alias_reader) => {
                    let mut temp: Vec<ComponentAlias> =
                        alias_reader.into_iter().collect::<Result<_, _>>()?;
                    let l = temp.len();
                    let new_sections = vec![ComponentSection::Alias; l];
                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &temp,
                        alias.len(),
                        &new_sections,
                    );
                    alias.append(&mut temp);
                    Self::add_to_sections(
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        ComponentSection::Alias,
                    );
                }
                Payload::ComponentCanonicalSection(canon_reader) => {
                    let mut temp: Vec<CanonicalFunction> =
                        canon_reader.into_iter().collect::<Result<_, _>>()?;
                    let l = temp.len();
                    let new_sections = vec![ComponentSection::Canon; l];
                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &temp,
                        canons.len(),
                        &new_sections,
                    );
                    canons.append(&mut temp);
                    Self::add_to_sections(
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        ComponentSection::Canon,
                    );
                }
                Payload::ModuleSection {
                    parser,
                    unchecked_range,
                } => {
                    // Indicating the start of a new module
                    stack.push(Encoding::Module);
                    let m = Module::parse_internal(
                        &wasm[unchecked_range.start - start..unchecked_range.end - start],
                        enable_multi_memory,
                        with_offsets,
                        parser,
                    )?;
                    store_handle.borrow_mut().assign_assumed_id(
                        &space_id,
                        &m.index_space_of(),
                        &ComponentSection::Module,
                        modules.len(),
                    );
                    modules.push(m);
                    Self::add_to_sections(
                        &mut sections,
                        &[ComponentSection::Module],
                        &mut num_sections,
                        ComponentSection::Module,
                    );
                }
                Payload::ComponentSection {
                    parser,
                    unchecked_range,
                } => {
                    let sub_space_id = store_handle.borrow_mut().new_scope();
                    let sub_uid = registry_handle.borrow_mut().alloc_uid();
                    let sect = ComponentSection::Component;

                    stack.push(Encoding::Component);
                    let cmp = Component::parse_comp(
                        &wasm[unchecked_range.start - start..unchecked_range.end - start],
                        enable_multi_memory,
                        with_offsets,
                        parser,
                        unchecked_range.start,
                        sub_space_id,
                        sub_uid,
                        Rc::clone(&registry_handle),
                        Rc::clone(&store_handle),
                    )?;
                    store_handle.borrow_mut().assign_assumed_id(
                        &space_id,
                        &cmp.index_space_of(),
                        &sect,
                        components.len(),
                    );
                    components.push(cmp);

                    Self::add_to_sections(
                        &mut sections,
                        &[sect],
                        &mut num_sections,
                        ComponentSection::Component,
                    );
                }
                Payload::ComponentStartSection { start, range: _ } => {
                    // TODO: bug — each declared result adds a value to the
                    // component value index space. The counter-bump fix below
                    // is correct in isolation but exposes a downstream gap:
                    // refs to start-result values have no resolution path
                    // through the topological collector / `assign.rs` /
                    // `ResolvedItem`. Re-enabling requires a dedicated
                    // `SpaceSubtype::StartResult` (or analogous) and matching
                    // plumbing in those visitors.
                    //
                    // let start_vec_idx = start_section.len();
                    // for _ in 0..start.results {
                    //     store_handle.borrow_mut().assign_assumed_id(
                    //         &space_id,
                    //         &Space::CompVal,
                    //         &ComponentSection::ComponentStartSection,
                    //         start_vec_idx,
                    //     );
                    // }
                    start_section.push(start);
                    Self::add_to_sections(
                        &mut sections,
                        &[ComponentSection::ComponentStartSection],
                        &mut num_sections,
                        ComponentSection::ComponentStartSection,
                    );
                }
                Payload::CustomSection(custom_section_reader) => {
                    match custom_section_reader.as_known() {
                        wasmparser::KnownCustom::ComponentName(name_section_reader) => {
                            for subsection in name_section_reader {
                                #[allow(clippy::single_match)]
                                match subsection? {
                                    wasmparser::ComponentName::Component { name, .. } => {
                                        component_name = Some(name.parse().unwrap())
                                    }
                                    wasmparser::ComponentName::CoreFuncs(names) => {
                                        core_func_names.add_all(names);
                                    }
                                    wasmparser::ComponentName::CoreGlobals(names) => {
                                        global_names.add_all(names);
                                    }
                                    wasmparser::ComponentName::CoreTables(names) => {
                                        table_names.add_all(names);
                                    }
                                    wasmparser::ComponentName::CoreModules(names) => {
                                        module_names.add_all(names);
                                    }
                                    wasmparser::ComponentName::CoreInstances(names) => {
                                        core_instances_names.add_all(names);
                                    }
                                    wasmparser::ComponentName::CoreTypes(names) => {
                                        core_type_names.add_all(names);
                                    }
                                    wasmparser::ComponentName::Types(names) => {
                                        type_names.add_all(names);
                                    }
                                    wasmparser::ComponentName::Instances(names) => {
                                        instance_names.add_all(names);
                                    }
                                    wasmparser::ComponentName::Components(names) => {
                                        components_names.add_all(names);
                                    }
                                    wasmparser::ComponentName::Funcs(names) => {
                                        func_names.add_all(names);
                                    }
                                    wasmparser::ComponentName::Values(names) => {
                                        value_names.add_all(names);
                                    }
                                    wasmparser::ComponentName::CoreMemories(names) => {
                                        memory_names.add_all(names);
                                    }
                                    wasmparser::ComponentName::CoreTags(names) => {
                                        tag_names.add_all(names);
                                    }
                                    wasmparser::ComponentName::Unknown { .. } => {}
                                }
                            }
                        }
                        _ => {
                            custom_sections
                                .push((custom_section_reader.name(), custom_section_reader.data()));
                            Self::add_to_sections(
                                &mut sections,
                                &[ComponentSection::CustomSection],
                                &mut num_sections,
                                ComponentSection::CustomSection,
                            );
                        }
                    }
                }
                Payload::UnknownSection {
                    id,
                    contents: _,
                    range: _,
                } => return Err(Error::UnknownSection { section_id: id }),
                Payload::Version { .. } | Payload::End { .. } => {} // nothing to do
                other => return Err(Error::UnhandledPayload(format!("{other:?}"))),
            }
        }

        // Scope discovery
        for comp in components.iter() {
            let sub_uid = comp.uid;
            let sub_space_id = comp.space_id;
            registry_handle
                .borrow_mut()
                .register_comp(sub_uid, sub_space_id);
        }
        for ty in core_types.iter() {
            populate_space_for_core_ty(ty, registry_handle.clone(), store_handle.clone());
        }
        for ty in component_types.iter() {
            populate_space_for_comp_ty(ty, registry_handle.clone(), store_handle.clone());
        }

        let comp = Component {
            uid: my_uid,
            modules,
            alias: Aliases::new(alias),
            core_types,
            component_types: ComponentTypes::new(component_types),
            imports,
            exports,
            instances,
            component_instance,
            canons: Canons::new(canons),
            space_id,
            scope_registry: registry_handle,
            index_store: store_handle,
            custom_sections: CustomSections::new(custom_sections),
            sections,
            start_section,
            num_sections,
            component_name,
            core_func_names,
            global_names,
            memory_names,
            tag_names,
            table_names,
            module_names,
            core_instances_names,
            core_type_names,
            type_names,
            instance_names,
            components_names,
            func_names,
            components,
            value_names,
        };

        comp.scope_registry
            .borrow_mut()
            .register_comp(my_uid, space_id);

        Ok(comp)
    }

    /// Encode this component into a WebAssembly binary.
    ///
    /// This method performs encoding in three high-level phases:
    ///
    /// 1. **Collect** – Walks the component IR and records all items that will be
    ///    encoded, along with any index references that need to be rewritten.
    /// 2. **Assign** – Resolves all index references after instrumentation so that
    ///    every reference points to its final concrete index.
    /// 3. **Encode** – Emits the final WebAssembly binary using the resolved indices.
    ///
    /// # Nested Components
    ///
    /// Components may be arbitrarily nested. During traversal, the encoder maintains
    /// a stack of component IDs that tracks which component is currently being
    /// visited. A registry maps component IDs to their corresponding components,
    /// allowing the encoder to correctly resolve cross-component references.
    ///
    /// # Scoped Definitions
    ///
    /// Many IR nodes introduce scopes (such as types, instances, or component-local
    /// definitions). To support correct index resolution in the presence of deep
    /// nesting and instrumentation, the encoder tracks scopes explicitly:
    ///
    /// - A scope stack is maintained during traversal.
    /// - Each scoped IR node is registered by identity in a scope registry.
    /// - Index lookups can recover the correct scope for any IR node in O(1) time.
    ///
    /// This design allows instrumentation to safely insert, reorder, or modify
    /// nodes without breaking encoding invariants.
    ///
    /// # Panics
    ///
    /// Panics if encoding encounters an internal inconsistency, such as:
    ///
    /// - An index reference that cannot be resolved to a registered scope
    /// - A malformed or structurally invalid component
    /// - Violations of encoding invariants introduced by instrumentation
    ///
    /// These panics indicate a bug in the encoder or in transformations applied
    /// to the component prior to encoding, rather than a recoverable runtime error.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use wirm::Component;
    ///
    /// let file = "path/to/file.wasm";
    /// let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    /// let mut comp = Component::parse(&buff, false, false).unwrap();
    /// let result = comp.encode();
    /// ```
    pub fn encode(&self) -> crate::ir::types::Result<Vec<u8>> {
        encode(self)
    }

    /// Map an already-resolved `(subtype, idx)` pair to a [`ResolvedItem`].
    ///
    /// This is the shared core of both [`Component::resolve`] and
    /// [`VisitCtxInner::resolve`].  Both callers look up the `(SpaceSubtype, idx)` from
    /// the appropriate [`IndexStore`] scope, then delegate the actual IR-field lookup here.
    pub(crate) fn item_at<'s>(
        &'s self,
        ref_: &IndexedRef,
        subtype: SpaceSubtype,
        idx: usize,
    ) -> ResolvedItem<'s, 'a> {
        match subtype {
            SpaceSubtype::Main => match ref_.space {
                Space::Comp => ResolvedItem::Component(ref_.index, &self.components[idx]),
                Space::CompType => {
                    ResolvedItem::CompType(ref_.index, &self.component_types.items[idx])
                }
                Space::CompInst => {
                    ResolvedItem::CompInst(ref_.index, &self.component_instance[idx])
                }
                Space::CoreInst => ResolvedItem::CoreInst(ref_.index, &self.instances[idx]),
                Space::CoreModule => ResolvedItem::Module(ref_.index, &self.modules[idx]),
                Space::CoreType => ResolvedItem::CoreType(ref_.index, &self.core_types[idx]),
                Space::CompFunc | Space::CoreFunc => {
                    ResolvedItem::Func(ref_.index, &self.canons.items[idx])
                }
                Space::CompVal
                | Space::CoreMemory
                | Space::CoreTable
                | Space::CoreGlobal
                | Space::CoreTag
                | Space::NA => unreachable!(
                    "space {:?} has no main vector on the component IR",
                    ref_.space
                ),
            },
            SpaceSubtype::Export => ResolvedItem::Export(ref_.index, &self.exports[idx]),
            SpaceSubtype::Import => ResolvedItem::Import(ref_.index, &self.imports[idx]),
            SpaceSubtype::Alias => ResolvedItem::Alias(ref_.index, &self.alias.items[idx]),
        }
    }

    /// Resolve a reference that is local to this component's own index space.
    ///
    /// The `ref_` must have `depth == 0` (i.e. it refers to an item defined directly
    /// within this component, not to an outer scope).  This is the typical case for
    /// refs found in a component's own exports, imports, types, etc.
    ///
    /// This uses the index space that was built at parse time — no walk is required.
    /// Because all nested components share the same index store and scope registry
    /// (via `Rc`), this method works correctly on any component in the tree, not just the root.
    pub fn resolve<'s>(&'s self, ref_: &IndexedRef) -> ResolvedItem<'s, 'a> {
        let store = self.index_store.borrow();
        let (subtype, idx, _subvec_idx) =
            store.index_from_assumed_id_no_cache(&self.space_id, ref_);
        drop(store);
        self.item_at(ref_, subtype, idx)
    }

    /// Resolve the item referred to by the named import, using this component's own index space.
    ///
    /// Finds the first import whose name matches `name`, then resolves the type ref it carries
    /// (e.g. `ComponentTypeRef::Instance(idx)`) into a [`ResolvedItem`].
    ///
    /// Returns `None` if no import with that name exists or if the import carries no type ref.
    pub fn resolve_named_import<'s>(&'s self, name: &str) -> Option<ResolvedItem<'s, 'a>> {
        let import = self.imports.iter().find(|i| i.name.0 == name)?;
        let type_ref = import.get_type_refs().into_iter().next()?;
        Some(self.resolve(&type_ref.ref_))
    }

    /// Resolve the item referred to by the named export, using this component's own index space.
    ///
    /// Finds the first export whose name matches `name`, then resolves the item ref it carries
    /// into a [`ResolvedItem`].
    ///
    /// Returns `None` if no export with that name exists.
    pub fn resolve_named_export<'s>(&'s self, name: &str) -> Option<ResolvedItem<'s, 'a>> {
        let export = self.exports.iter().find(|e| e.name.0 == name)?;
        Some(self.resolve(&export.get_item_ref().ref_))
    }

    /// Lookup the type for an exported `lift` canonical function. Returns
    /// `None` if `export` does not resolve (possibly through an alias /
    /// export chain) to a `CanonicalFunction::Lift`. Imports terminate the
    /// walk with `None` since they are not canonical functions.
    pub fn get_type_of_exported_lift_func(
        &self,
        export: &ComponentExport<'a>,
    ) -> Option<&ComponentType<'a>> {
        // Walk through aliases / nested exports to the underlying canonical
        // function. Imports are terminal — an imported function is declared
        // externally and isn't a lift.
        let mut item_ref = export.get_item_ref();
        let canon = loop {
            match self.resolve(&item_ref.ref_) {
                ResolvedItem::Func(_, canon) => break canon,
                ResolvedItem::Alias(_, alias) => item_ref = alias.get_item_ref(),
                ResolvedItem::Export(_, exp) => item_ref = exp.get_item_ref(),
                _ => return None,
            }
        };
        // Only lift funcs carry a component-level type; lower / resource
        // canons don't.
        if !matches!(canon, CanonicalFunction::Lift { .. }) {
            return None;
        }
        let type_ref = *canon.get_type_refs().first()?;
        match self.resolve(&type_ref.ref_) {
            ResolvedItem::CompType(_, ty) => Some(ty),
            _ => None,
        }
    }

    /// Print a rudimentary textual representation of a `Component`
    pub fn print(&self) {
        // Print Alias
        if !self.alias.items.is_empty() {
            eprintln!("Alias Section:");
            for alias in self.alias.items.iter() {
                print_alias(alias);
            }
            eprintln!();
        }

        // Print CoreType
        if !self.core_types.is_empty() {
            eprintln!("Core Type Section:");
            for cty in self.core_types.iter() {
                print_core_type(cty);
            }
            eprintln!();
        }

        // Print ComponentType
        if !self.component_types.items.is_empty() {
            eprintln!("Component Type Section:");
            for cty in self.component_types.items.iter() {
                print_component_type(cty);
            }
            eprintln!();
        }

        // Print Imports
        if !self.imports.is_empty() {
            eprintln!("Imports Section:");
            for imp in self.imports.iter() {
                print_component_import(imp);
            }
            eprintln!();
        }

        // Print Exports
        if !self.imports.is_empty() {
            eprintln!("Exports Section:");
            for exp in self.exports.iter() {
                print_component_export(exp);
            }
            eprintln!();
        }
    }

    /// Get Local Function ID by name
    // Note: returned absolute id here
    pub fn get_fid_by_name(&self, name: &str, module_idx: ModuleID) -> Option<FunctionID> {
        self.modules[*module_idx as usize]
            .functions
            .get_local_fid_by_name(name)
    }
}

#[derive(Debug, Default)]
pub struct Names {
    // Maintains keys in sorted order (need to encode in order of the index)
    pub(crate) names: BTreeMap<u32, String>,
}
impl Names {
    pub fn get(&self, id: u32) -> Option<&str> {
        self.names.get(&id).map(|s| s.as_str())
    }
    pub(crate) fn add_all(&mut self, names: NameMap) {
        for name in names {
            let naming = name.unwrap();
            self.add(naming.index, naming.name);
        }
    }
    fn add(&mut self, id: u32, name: &str) {
        self.names.insert(id, name.to_string());
    }
}
