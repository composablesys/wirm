#![allow(clippy::too_many_arguments)]
//! Intermediate Representation of a wasm component.

use crate::encode::component::encode;
use crate::error::Error;
use crate::error::Error::IO;
use crate::ir::component::alias::Aliases;
use crate::ir::component::canons::Canons;
use crate::ir::component::idx_spaces::{
    IndexSpaceOf, IndexStore, ScopeId, Space, SpaceSubtype, StoreHandle,
};
use crate::ir::component::refs::{GetItemRef, GetTypeRefs, IndexedRef};
use crate::ir::component::scopes::{IndexScopeRegistry, RegistryHandle};
use crate::ir::component::section::{
    get_sections_for_comp_ty, get_sections_for_core_ty_and_assign_top_level_ids,
    populate_space_for_comp_ty, populate_space_for_core_ty, ComponentSection,
};
use crate::ir::component::types::ComponentTypes;
use crate::ir::component::visitor::ResolvedItem;
use crate::ir::helpers::{
    print_alias, print_component_export, print_component_import, print_component_type,
    print_core_type,
};
use crate::ir::id::{
    AliasFuncId, AliasId, CanonicalFuncId, ComponentExportId, ComponentId, ComponentTypeFuncId,
    ComponentTypeId, ComponentTypeInstanceId, CoreInstanceId, CustomSectionID, FunctionID,
    GlobalID, ModuleID,
};
use crate::ir::module::module_globals::Global;
use crate::ir::module::Module;
use crate::ir::types::{CustomSection, CustomSections};
use crate::ir::AppendOnlyVec;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentFuncType, ComponentImport,
    ComponentInstance, ComponentStartFunction, ComponentType, CoreType, Encoding, Instance,
    InstanceTypeDeclaration, NameMap, Parser, Payload,
};

mod alias;
mod canons;
pub mod concrete;
pub(crate) mod idx_spaces;
pub mod refs;
pub(crate) mod scopes;
pub(crate) mod section;
#[cfg(test)]
mod tests;
mod types;
pub mod visitor;

#[derive(Debug)]
/// Intermediate Representation of a wasm component.
pub struct Component<'a> {
    pub id: ComponentId,
    /// Nested Components
    // These have scopes, but the scopes are looked up by ComponentId
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
        if !self.sections.is_empty() && self.sections[self.num_sections - 1].1 == sect {
            self.sections[self.num_sections - 1].0 += 1;
        } else {
            self.sections.push((1, sect));
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

    /// Add a Global to this Component.
    pub fn add_globals(&mut self, global: Global, module_idx: ModuleID) -> GlobalID {
        self.modules[*module_idx as usize].globals.add(global)
    }

    pub fn add_custom_section(&mut self, section: CustomSection<'a>) -> CustomSectionID {
        let id = self.custom_sections.add(section);
        self.add_section(ComponentSection::CustomSection);

        id
    }

    /// Add an Import to this Component.
    pub fn add_import(&mut self, import: ComponentImport<'a>) -> u32 {
        let idx = self.imports.len();
        let id = self.add_section_and_get_id(
            import.index_space_of(),
            ComponentSection::ComponentImport,
            idx,
        );
        self.imports.push(import);

        id as u32
    }

    /// Add an Aliased function to this Component.
    pub fn add_alias_func(&mut self, alias: ComponentAlias<'a>) -> (AliasFuncId, AliasId) {
        let space = alias.index_space_of();
        let (_item_id, alias_id) = self.alias.add(alias);
        let id = self.add_section_and_get_id(space, ComponentSection::Alias, *alias_id as usize);

        (AliasFuncId(id as u32), alias_id)
    }

    /// Add a Canonical Function to this Component.
    pub fn add_canon_func(&mut self, canon: CanonicalFunction) -> CanonicalFuncId {
        let space = canon.index_space_of();
        let idx = self.canons.add(canon).1;
        let id = self.add_section_and_get_id(space, ComponentSection::Canon, *idx as usize);

        CanonicalFuncId(id as u32)
    }

    /// Add a Component Type to this Component.
    pub(crate) fn add_component_type(
        &mut self,
        component_ty: ComponentType<'a>,
    ) -> (u32, ComponentTypeId) {
        let space = component_ty.index_space_of();
        let ids = self.component_types.add(component_ty);
        let id =
            self.add_section_and_get_id(space, ComponentSection::ComponentType, *ids.1 as usize);

        // Handle the index space of this node
        populate_space_for_comp_ty(
            self.component_types.items.last().unwrap(),
            self.scope_registry.clone(),
            self.index_store.clone(),
        );

        (id as u32, ids.1)
    }

    /// Add a Component Type that is an Instance to this component.
    pub fn add_type_instance(
        &mut self,
        decls: Vec<InstanceTypeDeclaration<'a>>,
    ) -> (ComponentTypeInstanceId, ComponentTypeId) {
        let (ty_inst_id, ty_id) =
            self.add_component_type(ComponentType::Instance(decls.into_boxed_slice()));

        // almost account for aliased types!
        (ComponentTypeInstanceId(ty_inst_id), ty_id)
    }

    /// Add a Component Type that is a Function to this component.
    pub fn add_type_func(
        &mut self,
        ty: ComponentFuncType<'a>,
    ) -> (ComponentTypeFuncId, ComponentTypeId) {
        let (ty_inst_id, ty_id) = self.add_component_type(ComponentType::Func(ty));

        // almost account for aliased types!
        (ComponentTypeFuncId(ty_inst_id), ty_id)
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

    fn add_to_sections(
        has_subscope: bool,
        sections: &mut Vec<(u32, ComponentSection)>,
        new_sections: &[ComponentSection],
        num_sections: &mut usize,
        sections_added: u32,
    ) {
        // We can only collapse sections if the new sections don't have
        // inner index spaces associated with them.
        let can_collapse = !has_subscope;

        if can_collapse
            && *num_sections > 0
            && sections[*num_sections - 1].1 == *new_sections.last().unwrap()
        {
            sections[*num_sections - 1].0 += sections_added;
            return;
        }
        // Cannot collapse these, add one at a time!
        for sect in new_sections.iter() {
            sections.push((1, sect.clone()));
            *num_sections += 1;
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

        let registry = IndexScopeRegistry::default();
        let mut store = IndexStore::default();
        let space_id = store.new_scope();
        let mut next_comp_id = 0;
        let res = Component::parse_comp(
            wasm,
            enable_multi_memory,
            with_offsets,
            parser,
            0,
            &mut vec![],
            space_id,
            Rc::new(RefCell::new(registry)),
            Rc::new(RefCell::new(store)),
            &mut next_comp_id,
        );
        res
    }

    fn parse_comp(
        wasm: &'a [u8],
        enable_multi_memory: bool,
        with_offsets: bool,
        parser: Parser,
        start: usize,
        parent_stack: &mut Vec<Encoding>,
        space_id: ScopeId,
        registry_handle: RegistryHandle,
        store_handle: StoreHandle,
        next_comp_id: &mut u32,
    ) -> Result<Component<'a>, Error> {
        let my_comp_id = ComponentId(*next_comp_id);
        *next_comp_id += 1;

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
            if let Payload::End(..) = payload {
                if !stack.is_empty() {
                    stack.pop();
                }
            }
            if !stack.is_empty() {
                continue;
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
                        false,
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        l as u32,
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
                        false,
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        l as u32,
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
                        false,
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::CoreTypeSection(core_type_reader) => {
                    let mut temp: Vec<Box<CoreType>> = core_type_reader
                        .into_iter()
                        .map(|res| res.map(Box::new))
                        .collect::<Result<_, _>>()?;

                    let old_len = core_types.len();
                    let l = temp.len();
                    core_types.append(&mut temp);

                    let mut new_sects = vec![];
                    let mut has_subscope = false;
                    for (idx, ty) in core_types[old_len..].iter().enumerate() {
                        let (new_sect, sect_has_subscope) =
                            get_sections_for_core_ty_and_assign_top_level_ids(
                                ty,
                                old_len + idx,
                                &space_id,
                                store_handle.clone(),
                            );
                        has_subscope |= sect_has_subscope;
                        new_sects.push(new_sect);
                    }

                    Self::add_to_sections(
                        has_subscope,
                        &mut sections,
                        &new_sects,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::ComponentTypeSection(component_type_reader) => {
                    let mut temp: Vec<Box<ComponentType>> = component_type_reader
                        .into_iter()
                        .map(|res| res.map(Box::new))
                        .collect::<Result<_, _>>()?;

                    let old_len = component_types.len();
                    let l = temp.len();
                    component_types.append(&mut temp);

                    let mut new_sects = vec![];
                    let mut has_subscope = false;
                    for ty in &component_types[old_len..] {
                        let (new_sect, sect_has_subscope) = get_sections_for_comp_ty(ty);
                        has_subscope |= sect_has_subscope;
                        new_sects.push(new_sect);
                    }

                    store_handle.borrow_mut().assign_assumed_id_for_boxed(
                        &space_id,
                        &component_types[old_len..],
                        old_len,
                        &new_sects,
                    );
                    Self::add_to_sections(
                        has_subscope,
                        &mut sections,
                        &new_sects,
                        &mut num_sections,
                        l as u32,
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
                        false,
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        l as u32,
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
                        false,
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        l as u32,
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
                        false,
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::ModuleSection {
                    parser,
                    unchecked_range,
                } => {
                    // Indicating the start of a new module
                    parent_stack.push(Encoding::Module);
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
                        false,
                        &mut sections,
                        &[ComponentSection::Module],
                        &mut num_sections,
                        1,
                    );
                }
                Payload::ComponentSection {
                    parser,
                    unchecked_range,
                } => {
                    let sub_space_id = store_handle.borrow_mut().new_scope();
                    let sect = ComponentSection::Component;

                    parent_stack.push(Encoding::Component);
                    stack.push(Encoding::Component);
                    let cmp = Component::parse_comp(
                        &wasm[unchecked_range.start - start..unchecked_range.end - start],
                        enable_multi_memory,
                        with_offsets,
                        parser,
                        unchecked_range.start,
                        &mut stack,
                        sub_space_id,
                        Rc::clone(&registry_handle),
                        Rc::clone(&store_handle),
                        next_comp_id,
                    )?;
                    store_handle.borrow_mut().assign_assumed_id(
                        &space_id,
                        &cmp.index_space_of(),
                        &sect,
                        components.len(),
                    );
                    components.push(cmp);

                    Self::add_to_sections(true, &mut sections, &[sect], &mut num_sections, 1);
                }
                Payload::ComponentStartSection { start, range: _ } => {
                    start_section.push(start);
                    Self::add_to_sections(
                        false,
                        &mut sections,
                        &[ComponentSection::ComponentStartSection],
                        &mut num_sections,
                        1,
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
                                false,
                                &mut sections,
                                &[ComponentSection::CustomSection],
                                &mut num_sections,
                                1,
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
                other => println!("TODO: Not sure what to do for: {:?}", other),
            }
        }

        // Scope discovery
        for comp in components.iter() {
            let comp_id = comp.id;
            let sub_space_id = comp.space_id;
            registry_handle
                .borrow_mut()
                .register_comp(comp_id, sub_space_id);
        }
        for ty in core_types.iter() {
            populate_space_for_core_ty(ty, registry_handle.clone(), store_handle.clone());
        }
        for ty in component_types.iter() {
            populate_space_for_comp_ty(ty, registry_handle.clone(), store_handle.clone());
        }

        let comp = Component {
            id: my_comp_id,
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
            .register_comp(my_comp_id, space_id);

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
    /// Because all nested components share the same [`IndexStore`] and scope registry
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

    /// Lookup the type for an exported `lift` canonical function.
    pub fn get_type_of_exported_lift_func(
        &self,
        export_id: ComponentExportId,
    ) -> Option<&ComponentType<'a>> {
        let export = self.exports.get(*export_id as usize)?;
        let type_ref = match self.resolve(&export.get_item_ref().ref_) {
            ResolvedItem::Func(_, canon) => *canon.get_type_refs().first().unwrap(),
            ResolvedItem::Alias(_, alias) => alias.get_item_ref(),
            _ => unreachable!("exported lift func should resolve to a Func or Alias"),
        };
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
