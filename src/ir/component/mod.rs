#![allow(clippy::mut_range_bound)] // see https://github.com/rust-lang/rust-clippy/issues/6072
//! Intermediate Representation of a wasm component.

use crate::error::Error;
use crate::ir::component::alias::Aliases;
use crate::ir::component::canons::Canons;
use crate::ir::component::index_spaces::{IdxSpace, IdxSpaces};
use crate::ir::component::types::ComponentTypes;
use crate::ir::helpers::{
    print_alias, print_component_export, print_component_import, print_component_type,
    print_core_type,
};
use crate::ir::id::{
    AliasFuncId, AliasId, CanonicalFuncId, ComponentExportId, ComponentTypeFuncId, ComponentTypeId,
    ComponentTypeInstanceId, CoreInstanceId, CustomSectionID, FunctionID, GlobalID, ModuleID,
};
use crate::ir::module::module_functions::FuncKind;
use crate::ir::module::module_globals::Global;
use crate::ir::module::Module;
use crate::ir::section::ComponentSection;
use crate::ir::types::CustomSections;
use crate::ir::wrappers::{
    add_to_namemap, convert_component_type, convert_instance_type, convert_module_type_declaration,
    convert_results, do_reencode, process_alias,
};
use wasm_encoder::reencode::{Reencode, ReencodeComponent, RoundtripReencoder};
use wasm_encoder::{ComponentAliasSection, ModuleArg, ModuleSection, NestedComponentSection};
use wasmparser::{CanonicalFunction, ComponentAlias, ComponentExport, ComponentExternalKind, ComponentFuncType, ComponentImport, ComponentInstance, ComponentOuterAliasKind, ComponentStartFunction, ComponentType, ComponentTypeDeclaration, ComponentTypeRef, ComponentValType, CoreType, Encoding, ExternalKind, Instance, InstanceTypeDeclaration, Parser, Payload};

mod alias;
mod canons;
mod index_spaces;
pub mod types;

#[derive(Debug)]
/// Intermediate Representation of a wasm component.
pub struct Component<'a> {
    /// Modules
    pub modules: Vec<Module<'a>>,
    ///Alias
    pub alias: Aliases<'a>,
    /// Core Types
    pub core_types: Vec<CoreType<'a>>,
    /// Component Types
    pub component_types: ComponentTypes<'a>,
    /// Imports
    pub imports: Vec<ComponentImport<'a>>,
    /// Exports
    pub exports: Vec<ComponentExport<'a>>,
    /// Core Instances
    pub instances: Vec<Instance<'a>>,
    /// Component Instances
    pub component_instance: Vec<ComponentInstance<'a>>,
    /// Canons
    pub canons: Canons,
    /// Custom sections
    pub custom_sections: CustomSections<'a>,
    /// Nested Components
    pub components: Vec<Component<'a>>,
    /// Component Start Section
    pub start_section: Vec<ComponentStartFunction>,
    /// Sections of the Component. Represented as (#num of occurrences of a section, type of section)
    pub sections: Vec<(u32, ComponentSection)>,
    num_sections: usize,

    // Names
    pub(crate) component_name: Option<String>,
    pub(crate) core_func_names: wasm_encoder::NameMap,
    pub(crate) global_names: wasm_encoder::NameMap,
    pub(crate) memory_names: wasm_encoder::NameMap,
    pub(crate) tag_names: wasm_encoder::NameMap,
    pub(crate) table_names: wasm_encoder::NameMap,
    pub(crate) module_names: wasm_encoder::NameMap,
    pub(crate) core_instances_names: wasm_encoder::NameMap,
    pub(crate) core_type_names: wasm_encoder::NameMap,
    pub(crate) type_names: wasm_encoder::NameMap,
    pub(crate) instance_names: wasm_encoder::NameMap,
    pub(crate) components_names: wasm_encoder::NameMap,
    pub(crate) func_names: wasm_encoder::NameMap,
    pub(crate) value_names: wasm_encoder::NameMap,
}

impl Default for Component<'_> {
    fn default() -> Self {
        Component::new()
    }
}

impl<'a> Component<'a> {
    /// Creates a new Empty Component
    pub fn new() -> Self {
        Component {
            modules: vec![],
            alias: Aliases::default(),
            core_types: vec![],
            component_types: ComponentTypes::default(),
            imports: vec![],
            exports: vec![],
            instances: vec![],
            component_instance: vec![],
            canons: Canons::default(),
            custom_sections: CustomSections::new(vec![]),
            start_section: vec![],
            sections: vec![],
            num_sections: 0,
            components: vec![],
            component_name: None,
            core_func_names: wasm_encoder::NameMap::new(),
            global_names: wasm_encoder::NameMap::new(),
            memory_names: wasm_encoder::NameMap::new(),
            tag_names: wasm_encoder::NameMap::new(),
            table_names: wasm_encoder::NameMap::new(),
            module_names: wasm_encoder::NameMap::new(),
            core_instances_names: wasm_encoder::NameMap::new(),
            core_type_names: wasm_encoder::NameMap::new(),
            type_names: wasm_encoder::NameMap::new(),
            instance_names: wasm_encoder::NameMap::new(),
            components_names: wasm_encoder::NameMap::new(),
            func_names: wasm_encoder::NameMap::new(),
            value_names: wasm_encoder::NameMap::new(),
        }
    }

    fn add_section(&mut self, section: ComponentSection) {
        if self.sections[self.num_sections - 1].1 == section {
            self.sections[self.num_sections - 1].0 += 1;
        } else {
            self.sections.push((1, section));
        }
    }

    /// Add a Module to this Component.
    pub fn add_module(&mut self, module: Module<'a>) -> ModuleID {
        let id = self.modules.len();
        self.modules.push(module);
        self.add_section(ComponentSection::Module);

        ModuleID(id as u32)
    }

    /// Add a Global to this Component.
    pub fn add_globals(&mut self, global: Global, module_idx: ModuleID) -> GlobalID {
        self.modules[*module_idx as usize].globals.add(global)
    }

    pub fn add_import(&mut self, import: ComponentImport<'a>) -> u32 {
        let id = self.imports.len();
        self.imports.push(import);
        self.add_section(ComponentSection::ComponentImport);

        id as u32
    }

    pub fn add_alias_func(&mut self, alias: ComponentAlias<'a>) -> (AliasFuncId, AliasId) {
        let (item_id, alias_id) = self.alias.add(alias);
        self.add_section(ComponentSection::Alias);

        (AliasFuncId(item_id), alias_id)
    }

    pub fn add_canon_func(&mut self, canon: CanonicalFunction) -> CanonicalFuncId {
        let id = self.canons.add(canon).1;
        self.add_section(ComponentSection::Canon);

        id
    }

    pub(crate) fn add_component_type(
        &mut self,
        component_ty: ComponentType<'a>,
    ) -> (u32, ComponentTypeId) {
        let ids = self.component_types.add(component_ty);
        self.add_section(ComponentSection::ComponentType);

        ids
    }

    pub fn add_type_instance(
        &mut self,
        decls: Vec<InstanceTypeDeclaration<'a>>,
    ) -> (ComponentTypeInstanceId, ComponentTypeId) {
        let (ty_inst_id, ty_id) =
            self.add_component_type(ComponentType::Instance(decls.into_boxed_slice()));

        // almost account for aliased types!
        (
            ComponentTypeInstanceId(ty_inst_id + self.alias.num_types as u32),
            ty_id,
        )
    }

    pub fn add_type_func(
        &mut self,
        ty: ComponentFuncType<'a>,
    ) -> (ComponentTypeFuncId, ComponentTypeId) {
        let (ty_inst_id, ty_id) = self.add_component_type(ComponentType::Func(ty));

        // almost account for aliased types!
        (
            ComponentTypeFuncId(ty_inst_id + self.alias.num_types as u32),
            ty_id,
        )
    }

    pub fn add_core_instance(&mut self, instance: Instance<'a>) -> CoreInstanceId {
        let inst_id = self.instances.len() as u32;
        self.instances.push(instance);
        self.add_section(ComponentSection::CoreInstance);

        CoreInstanceId(inst_id)
    }

    pub fn get_type_of_exported_func(
        &self,
        export_id: ComponentExportId,
    ) -> Option<&ComponentType<'a>> {
        // TODO: cache this somehow
        let mut canon_funcs_before = 0;
        while !matches!(
            self.canons.items.get(canon_funcs_before),
            Some(CanonicalFunction::Lift { .. }) | Some(CanonicalFunction::Lower { .. })
        ) {
            // Handle non-lift/lower canonical functions
            canon_funcs_before += 1;
        }

        // TODO: cache this somehow
        let mut exported_funcs_before = 0;
        let mut e = self.exports.get(*export_id as usize);
        while exported_funcs_before < *export_id
            && e.is_some()
            && !matches!(e.unwrap().kind, ComponentExternalKind::Func)
        {
            exported_funcs_before += 1;
            e = self.exports.get(*export_id as usize);
        }

        if let Some(export) = self.exports.get(*export_id as usize) {
            if let Some(CanonicalFunction::Lift {
                type_index: ty_id, ..
            }) = self
                .canons
                .items
                .get(export.index as usize + canon_funcs_before - exported_funcs_before as usize)
            {
                // TODO: cache this somehow
                let mut num_non_func_tys = 0;
                while !matches!(
                    self.component_types.items.get(num_non_func_tys),
                    Some(ComponentType::Func(..))
                ) {
                    // Skip non-function types
                    num_non_func_tys += 1;
                }
                let mut i = 0;
                let mut num_aliased_types = 0;
                while num_aliased_types < (*ty_id as usize - num_non_func_tys)
                    && i < self.alias.items.len()
                {
                    if matches!(
                        self.alias.items.get(i),
                        Some(ComponentAlias::InstanceExport {
                            kind: ComponentExternalKind::Type,
                            ..
                        })
                    ) {
                        // aliased types
                        num_aliased_types += 1;
                    }
                    i += 1;
                }
                self.component_types
                    .items
                    .get(*ty_id as usize - num_aliased_types)
            } else {
                None
            }
        } else {
            None
        }
    }

    fn add_to_sections(
        sections: &mut Vec<(u32, ComponentSection)>,
        section: ComponentSection,
        num_sections: &mut usize,
        sections_added: u32,
    ) {
        if *num_sections > 0 && sections[*num_sections - 1].1 == section {
            sections[*num_sections - 1].0 += sections_added;
        } else {
            sections.push((sections_added, section));
            *num_sections += 1;
        }
    }

    /// Parse a `Component` from a wasm binary.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use wirm::Component;
    ///
    /// let file = "path_to_file";
    /// let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    /// let comp = Component::parse(&buff, false).unwrap();
    /// ```
    pub fn parse(wasm: &'a [u8], enable_multi_memory: bool) -> Result<Self, Error> {
        let parser = Parser::new(0);
        Self::parse_comp(wasm, enable_multi_memory, parser, 0, &mut vec![])
    }

    fn parse_comp(
        wasm: &'a [u8],
        enable_multi_memory: bool,
        parser: Parser,
        start: usize,
        parent_stack: &mut Vec<Encoding>,
    ) -> Result<Self, Error> {
        let mut modules = vec![];
        let mut core_types = vec![];
        let mut component_types = vec![];
        let mut imports = vec![];
        let mut exports = vec![];
        let mut instances = vec![];
        let mut canons = vec![];
        let mut alias = vec![];
        let mut component_instance = vec![];
        let mut custom_sections = vec![];
        let mut sections = vec![];
        let mut num_sections: usize = 0;
        let mut components: Vec<Component> = vec![];
        let mut start_section = vec![];
        let mut stack = vec![];

        // Names
        let mut component_name: Option<String> = None;
        let mut core_func_names = wasm_encoder::NameMap::new();
        let mut global_names = wasm_encoder::NameMap::new();
        let mut tag_names = wasm_encoder::NameMap::new();
        let mut memory_names = wasm_encoder::NameMap::new();
        let mut table_names = wasm_encoder::NameMap::new();
        let mut module_names = wasm_encoder::NameMap::new();
        let mut core_instance_names = wasm_encoder::NameMap::new();
        let mut instance_names = wasm_encoder::NameMap::new();
        let mut components_names = wasm_encoder::NameMap::new();
        let mut func_names = wasm_encoder::NameMap::new();
        let mut value_names = wasm_encoder::NameMap::new();
        let mut core_type_names = wasm_encoder::NameMap::new();
        let mut type_names = wasm_encoder::NameMap::new();

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
                    let temp: &mut Vec<ComponentImport> = &mut import_section_reader
                        .into_iter()
                        .collect::<Result<_, _>>()?;
                    let l = temp.len();
                    imports.append(temp);
                    Self::add_to_sections(
                        &mut sections,
                        ComponentSection::ComponentImport,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::ComponentExportSection(export_section_reader) => {
                    let temp: &mut Vec<ComponentExport> = &mut export_section_reader
                        .into_iter()
                        .collect::<Result<_, _>>()?;
                    let l = temp.len();
                    exports.append(temp);
                    Self::add_to_sections(
                        &mut sections,
                        ComponentSection::ComponentExport,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::InstanceSection(instance_section_reader) => {
                    let temp: &mut Vec<Instance> = &mut instance_section_reader
                        .into_iter()
                        .collect::<Result<_, _>>()?;
                    let l = temp.len();
                    instances.append(temp);
                    Self::add_to_sections(
                        &mut sections,
                        ComponentSection::CoreInstance,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::CoreTypeSection(core_type_reader) => {
                    let temp: &mut Vec<CoreType> =
                        &mut core_type_reader.into_iter().collect::<Result<_, _>>()?;
                    let l = temp.len();
                    core_types.append(temp);
                    Self::add_to_sections(
                        &mut sections,
                        ComponentSection::CoreType,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::ComponentTypeSection(component_type_reader) => {
                    let temp: &mut Vec<ComponentType> = &mut component_type_reader
                        .into_iter()
                        .collect::<Result<_, _>>()?;
                    let l = temp.len();
                    component_types.append(temp);
                    Self::add_to_sections(
                        &mut sections,
                        ComponentSection::ComponentType,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::ComponentInstanceSection(component_instances) => {
                    let temp: &mut Vec<ComponentInstance> =
                        &mut component_instances.into_iter().collect::<Result<_, _>>()?;
                    let l = temp.len();
                    component_instance.append(temp);
                    Self::add_to_sections(
                        &mut sections,
                        ComponentSection::ComponentInstance,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::ComponentAliasSection(alias_reader) => {
                    let temp: &mut Vec<ComponentAlias> =
                        &mut alias_reader.into_iter().collect::<Result<_, _>>()?;
                    let l = temp.len();
                    alias.append(temp);
                    Self::add_to_sections(
                        &mut sections,
                        ComponentSection::Alias,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::ComponentCanonicalSection(canon_reader) => {
                    let temp: &mut Vec<CanonicalFunction> =
                        &mut canon_reader.into_iter().collect::<Result<_, _>>()?;
                    let l = temp.len();
                    canons.append(temp);
                    Self::add_to_sections(
                        &mut sections,
                        ComponentSection::Canon,
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
                    modules.push(Module::parse_internal(
                        &wasm[unchecked_range.start - start..unchecked_range.end - start],
                        enable_multi_memory,
                        parser,
                    )?);
                    Self::add_to_sections(
                        &mut sections,
                        ComponentSection::Module,
                        &mut num_sections,
                        1,
                    );
                }
                Payload::ComponentSection {
                    parser,
                    unchecked_range,
                } => {
                    // Indicating the start of a new component
                    parent_stack.push(Encoding::Component);
                    stack.push(Encoding::Component);
                    let cmp = Component::parse_comp(
                        &wasm[unchecked_range.start - start..unchecked_range.end - start],
                        enable_multi_memory,
                        parser,
                        unchecked_range.start,
                        &mut stack,
                    )?;
                    components.push(cmp);
                    Self::add_to_sections(
                        &mut sections,
                        ComponentSection::Component,
                        &mut num_sections,
                        1,
                    );
                }
                Payload::ComponentStartSection { start, range: _ } => {
                    start_section.push(start);
                    Self::add_to_sections(
                        &mut sections,
                        ComponentSection::ComponentStartSection,
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
                                        add_to_namemap(&mut core_func_names, names);
                                    }
                                    wasmparser::ComponentName::CoreGlobals(names) => {
                                        add_to_namemap(&mut global_names, names);
                                    }
                                    wasmparser::ComponentName::CoreTables(names) => {
                                        add_to_namemap(&mut table_names, names);
                                    }
                                    wasmparser::ComponentName::CoreModules(names) => {
                                        add_to_namemap(&mut module_names, names);
                                    }
                                    wasmparser::ComponentName::CoreInstances(names) => {
                                        add_to_namemap(&mut core_instance_names, names);
                                    }
                                    wasmparser::ComponentName::CoreTypes(names) => {
                                        add_to_namemap(&mut core_type_names, names);
                                    }
                                    wasmparser::ComponentName::Types(names) => {
                                        add_to_namemap(&mut type_names, names);
                                    }
                                    wasmparser::ComponentName::Instances(names) => {
                                        add_to_namemap(&mut instance_names, names);
                                    }
                                    wasmparser::ComponentName::Components(names) => {
                                        add_to_namemap(&mut components_names, names);
                                    }
                                    wasmparser::ComponentName::Funcs(names) => {
                                        add_to_namemap(&mut func_names, names);
                                    }
                                    wasmparser::ComponentName::Values(names) => {
                                        add_to_namemap(&mut value_names, names);
                                    }
                                    wasmparser::ComponentName::CoreMemories(names) => {
                                        add_to_namemap(&mut memory_names, names);
                                    }
                                    wasmparser::ComponentName::CoreTags(names) => {
                                        add_to_namemap(&mut tag_names, names);
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
                                ComponentSection::CustomSection,
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
                _ => {}
            }
        }
        Ok(Component {
            modules,
            alias: Aliases::new(alias),
            core_types,
            component_types: ComponentTypes::new(component_types),
            imports,
            exports,
            instances,
            component_instance,
            canons: Canons::new(canons),
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
            core_instances_names: core_instance_names,
            core_type_names,
            type_names,
            instance_names,
            components_names,
            func_names,
            components,
            value_names,
        })
    }

    /// Encode a `Component` into bytes.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use wirm::Component;
    ///
    /// let file = "path_to_file";
    /// let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    /// let mut comp = Component::parse(&buff, false).unwrap();
    /// let result = comp.encode();
    /// ```
    pub fn encode(&mut self) -> Vec<u8> {
        self.encode_comp().finish()
    }

    fn encode_comp(&mut self) -> wasm_encoder::Component {
        let mut component = wasm_encoder::Component::new();
        let mut idx = IdxSpaces::new();
        let mut reencode = RoundtripReencoder;

        // Create a clone of the original sections to allow iterating over them (borrow issues)
        // TODO: Can I avoid this clone?
        let orig_sections = self.sections.clone();
        for (num, section) in orig_sections.iter() {
            match section {
                ComponentSection::Component => {
                    self.internal_encode_component(idx.comp.curr(), *num as usize, &mut component, &mut idx);
                }
                ComponentSection::Module => {
                    self.internal_encode_module(idx.module.curr(), *num as usize, &mut component, &mut idx);
                }
                ComponentSection::CoreType => {
                    self.internal_encode_core_type(
                        idx.core_type.curr(),
                        *num as usize,
                        &mut component,
                        &mut idx,
                        &mut reencode,
                    );
                }
                ComponentSection::ComponentType => {
                    self.internal_encode_component_type(
                        idx.comp_type.curr(),
                        *num as usize,
                        &mut component,
                        &mut idx,
                        &mut reencode,
                    );
                }
                ComponentSection::ComponentImport => {
                    self.internal_encode_component_import(
                        idx.last_processed_imp,
                        *num as usize,
                        &mut component,
                        &mut idx,
                        &mut reencode,
                    );
                    idx.last_processed_imp += *num as usize;
                }
                ComponentSection::ComponentExport => {
                    self.internal_encode_component_export(
                        idx.last_processed_exp,
                        *num as usize,
                        &mut component,
                        &mut idx,
                        &mut reencode,
                    );
                    idx.last_processed_exp += *num as usize;
                }
                ComponentSection::ComponentInstance => {
                    self.internal_encode_component_instance(
                        idx.comp_inst.curr(),
                        *num as usize,
                        &mut component,
                        &mut idx,
                        &mut reencode,
                    );
                }
                ComponentSection::CoreInstance => {
                    self.internal_encode_core_instance(
                        idx.core_inst.curr(),
                        *num as usize,
                        &mut component,
                        &mut idx,
                    );
                }
                ComponentSection::Alias => {
                    self.internal_encode_alias(
                        idx.last_processed_alias,
                        *num as usize,
                        &mut component,
                        &mut idx,
                        &mut reencode,
                    );
                    idx.last_processed_alias += *num as usize;
                }
                ComponentSection::Canon => {
                    self.internal_encode_canon(
                        idx.core_func.curr(),
                        *num as usize,
                        &mut component,
                        &mut idx,
                        &mut reencode,
                    );
                }
                ComponentSection::ComponentStartSection => {
                    self.internal_encode_start(&mut component);
                }
                ComponentSection::CustomSection => {
                    self.internal_encode_custom(
                        idx.last_processed_custom,
                        *num as usize,
                        &mut component,
                    );
                    idx.last_processed_custom += *num as usize;
                }
            }
        }

        // Name section
        let mut name_sec = wasm_encoder::ComponentNameSection::new();

        if let Some(comp_name) = &self.component_name {
            name_sec.component(comp_name);
        }

        name_sec.core_funcs(&self.core_func_names);
        name_sec.core_tables(&self.table_names);
        name_sec.core_memories(&self.memory_names);
        name_sec.core_tags(&self.tag_names);
        name_sec.core_globals(&self.global_names);
        name_sec.core_types(&self.core_type_names);
        name_sec.core_modules(&self.module_names);
        name_sec.core_instances(&self.core_instances_names);
        name_sec.funcs(&self.func_names);
        name_sec.values(&self.value_names);
        name_sec.types(&self.type_names);
        name_sec.components(&self.components_names);
        name_sec.instances(&self.instance_names);

        // Add the name section back to the component
        component.section(&name_sec);

        component
    }

    fn internal_encode_component(
        &mut self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        idx: &mut IdxSpaces,
    ) {
        assert!(start + num - idx.comp.curr_external() <= self.components.len());
        for i in 0..num {
            let id = idx.comp.id_for(start + i);
            component.section(&NestedComponentSection(
                &self.components[id].encode_comp(),
            ));
            idx.comp.assign(start + i, id);
        }
    }

    fn internal_encode_module(
        &mut self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        idx: &mut IdxSpaces,
    ) {
        assert!(start + num - idx.module.curr_external() <= self.modules.len());
        for i in 0..num {
            let id = idx.module.id_for(start + i);
            component.section(&ModuleSection(
                &self.modules[id].encode_internal(false).0,
            ));
            idx.module.assign(start + i, id);
        }
    }

    fn internal_encode_core_type(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        idx: &mut IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) {
        assert!(num + start <= self.core_types.len());
        let mut type_section = wasm_encoder::CoreTypeSection::new();

        for i in 0..num {
            let id = idx.core_type.id_for(start + i);

            match &self.core_types[id] {
                CoreType::Rec(recgroup) => {
                    let types = recgroup
                        .types()
                        .map(|ty| {
                            reencode.sub_type(ty.to_owned()).unwrap_or_else(|_| {
                                panic!("Could not encode type as subtype: {:?}", ty)
                            })
                        })
                        .collect::<Vec<_>>();

                    if recgroup.is_explicit_rec_group() {
                        type_section.ty().core().rec(types);
                    } else {
                        // it's implicit!
                        for subty in types {
                            type_section.ty().core().subtype(&subty);
                        }
                    }
                }
                CoreType::Module(module) => {
                    // TODO: This needs to be fixed
                    let enc = type_section.ty();
                    convert_module_type_declaration(module, enc, reencode);
                }
            }
            idx.core_type.assign(start + i, id);
        }
        component.section(&type_section);
    }

    fn internal_encode_component_type(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        idx: &mut IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) {
        assert!(num + start - idx.comp_type.curr_external() <= self.component_types.items.len());
        let mut component_ty_section = wasm_encoder::ComponentTypeSection::new();
        for i in 0..num {
            let id = idx.comp_type.id_for(start + i);

            match &self.component_types.items[id] {
                ComponentType::Defined(comp_ty) => {
                    let enc = component_ty_section.defined_type();
                    match comp_ty {
                        wasmparser::ComponentDefinedType::Primitive(p) => {
                            enc.primitive(wasm_encoder::PrimitiveValType::from(*p))
                        }
                        wasmparser::ComponentDefinedType::Record(records) => {
                            enc.record(
                                records.iter().map(|(n, ty)| {
                                    let fixed_ty = self.lookup_component_val_type(
                                        *ty, idx, component, reencode
                                    );
                                    (*n, reencode.component_val_type(fixed_ty))
                                }),
                            );
                        }
                        wasmparser::ComponentDefinedType::Variant(variants) => {
                            enc.variant(variants.iter().map(|variant| {
                                (
                                    variant.name,
                                    variant.ty.map(|ty| {
                                        let fixed_ty = self.lookup_component_val_type(
                                            ty, idx, component, reencode
                                        );
                                        reencode.component_val_type(fixed_ty)
                                    }),
                                    variant.refines,
                                )
                            }))
                        }
                        wasmparser::ComponentDefinedType::List(l) => {
                            let fixed_ty = self.lookup_component_val_type(
                                *l, idx, component, reencode
                            );
                            enc.list(reencode.component_val_type(fixed_ty))
                        }
                        wasmparser::ComponentDefinedType::Tuple(tup) => enc.tuple(
                            tup.iter()
                                .map(|val_type| {
                                    let fixed_ty = self.lookup_component_val_type(
                                        *val_type, idx, component, reencode
                                    );
                                    reencode.component_val_type(fixed_ty)
                                }),
                        ),
                        wasmparser::ComponentDefinedType::Flags(flags) => {
                            enc.flags(flags.clone().into_vec().into_iter())
                        }
                        wasmparser::ComponentDefinedType::Enum(en) => {
                            enc.enum_type(en.clone().into_vec().into_iter())
                        }
                        wasmparser::ComponentDefinedType::Option(opt) => {
                            let fixed_ty = self.lookup_component_val_type(
                                *opt, idx, component, reencode
                            );
                            enc.option(reencode.component_val_type(fixed_ty))
                        }
                        wasmparser::ComponentDefinedType::Result { ok, err } => enc.result(
                            ok.map(|val_type| {
                                let fixed_ty = self.lookup_component_val_type(
                                    val_type, idx, component, reencode
                                );
                                reencode.component_val_type(fixed_ty)
                            }),
                            err.map(|val_type| {
                                let fixed_ty = self.lookup_component_val_type(
                                    val_type, idx, component, reencode
                                );
                                reencode.component_val_type(fixed_ty)
                            }),
                        ),
                        wasmparser::ComponentDefinedType::Own(u) => {
                            // TODO: This needs to be fixed
                            enc.own(*u)
                        },
                        wasmparser::ComponentDefinedType::Borrow(u) => {
                            // TODO: This needs to be fixed
                            enc.borrow(*u)
                        },
                        wasmparser::ComponentDefinedType::Future(opt) => match opt {
                            Some(u) => {
                                let fixed_ty = self.lookup_component_val_type(
                                    *u, idx, component, reencode
                                );
                                enc.future(Some(reencode.component_val_type(fixed_ty)))
                            },
                            None => enc.future(None),
                        },
                        wasmparser::ComponentDefinedType::Stream(opt) => match opt {
                            Some(u) => {
                                let fixed_ty = self.lookup_component_val_type(
                                    *u, idx, component, reencode
                                );
                                enc.stream(Some(reencode.component_val_type(fixed_ty)))
                            },
                            None => enc.stream(None),
                        },
                        wasmparser::ComponentDefinedType::FixedSizeList(ty, i) => {
                            let fixed_ty = self.lookup_component_val_type(
                                *ty, idx, component, reencode
                            );
                            enc.fixed_size_list(reencode.component_val_type(fixed_ty), *i)
                        }
                    }
                }
                ComponentType::Func(func_ty) => {
                    let mut enc = component_ty_section.function();
                    enc.params(func_ty.params.iter().map(
                        |p: &(&str, wasmparser::ComponentValType)| {
                            let fixed_ty = self.lookup_component_val_type(
                                p.1, idx, component, reencode
                            );
                            (p.0, reencode.component_val_type(fixed_ty))
                        },
                    ));
                    enc.result(func_ty.result.map(|v| {
                        let fixed_ty = self.lookup_component_val_type(
                            v, idx, component, reencode
                        );
                        reencode.component_val_type(fixed_ty)
                    }));
                }
                ComponentType::Component(comp) => {
                    // TODO: Check if we need to lookup IDs here
                    let mut new_comp = wasm_encoder::ComponentType::new();
                    for c in comp.iter() {
                        match c {
                            ComponentTypeDeclaration::CoreType(core) => match core {
                                CoreType::Rec(recgroup) => {
                                    let types = recgroup
                                        .types()
                                        .map(|ty| {
                                            reencode.sub_type(ty.to_owned()).unwrap_or_else(|_| {
                                                panic!("Could not encode type as subtype: {:?}", ty)
                                            })
                                        })
                                        .collect::<Vec<_>>();

                                    if recgroup.is_explicit_rec_group() {
                                        new_comp.core_type().core().rec(types);
                                    } else {
                                        // it's implicit!
                                        for subty in types {
                                            new_comp.core_type().core().subtype(&subty);
                                        }
                                    }
                                }
                                CoreType::Module(module) => {
                                    // TODO: This needs to be fixed
                                    let enc = new_comp.core_type();
                                    convert_module_type_declaration(module, enc, reencode);
                                }
                            },
                            ComponentTypeDeclaration::Type(typ) => {
                                // TODO: This needs to be fixed
                                let enc = new_comp.ty();
                                convert_component_type(&(*typ).clone(), enc, reencode);
                            }
                            ComponentTypeDeclaration::Alias(a) => {
                                // TODO: This needs to be fixed
                                new_comp.alias(process_alias(a, reencode));
                            }
                            ComponentTypeDeclaration::Export { name, ty } => {
                                // TODO: This needs to be fixed
                                let ty = do_reencode(
                                    *ty,
                                    RoundtripReencoder::component_type_ref,
                                    reencode,
                                    "component type",
                                );
                                new_comp.export(name.0, ty);
                            }
                            ComponentTypeDeclaration::Import(imp) => {
                                // TODO: This needs to be fixed
                                let ty = do_reencode(
                                    imp.ty,
                                    RoundtripReencoder::component_type_ref,
                                    reencode,
                                    "component type",
                                );
                                new_comp.import(imp.name.0, ty);
                            }
                        }
                    }
                    component_ty_section.component(&new_comp);
                }
                ComponentType::Instance(inst) => {
                    // TODO: This needs to be fixed
                    component_ty_section.instance(&convert_instance_type(inst, reencode));
                }
                ComponentType::Resource { rep, dtor } => {
                    // TODO: This needs to be fixed (the dtor likely points to a function)
                    component_ty_section.resource(reencode.val_type(*rep).unwrap(), *dtor);
                }
            }
            idx.comp_type.assign(start + i, id);
        }
        component.section(&component_ty_section);
    }

    fn internal_encode_component_import(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        idx: &mut IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) {
        assert!(start + num <= self.imports.len());
        let mut imports = wasm_encoder::ComponentImportSection::new();
        for i in 0..num {
            let id = start + i;
            let imp = &self.imports[id];

            // TODO: Verify this is correct!
            let space = match imp.ty {
                ComponentTypeRef::Component(_) => &mut idx.comp,
                ComponentTypeRef::Module(_) => &mut idx.module,
                ComponentTypeRef::Type(_) => &mut idx.comp_type,
                ComponentTypeRef::Instance(_) => &mut idx.comp_inst,
                ComponentTypeRef::Func(_) => &mut idx.comp_func,
                ComponentTypeRef::Value(_) => &mut idx.comp_val
            };
            let ref_id = space.next();
            space.assign(ref_id, ref_id);
            space.was_external();

            // TODO: This needs to be fixed (imp.ty)
            let ty = do_reencode(
                imp.ty,
                RoundtripReencoder::component_type_ref,
                reencode,
                "component import",
            );
            imports.import(imp.name.0, ty);
        }
        component.section(&imports);
    }

    fn internal_encode_component_export(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        idx: &mut IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) {
        assert!(start + num <= self.exports.len());
        let mut exports = wasm_encoder::ComponentExportSection::new();
        for i in 0..num {
            let id = start + i;
            let exp = &self.exports[id];

            // TODO: Verify this is correct!
            let space = match exp.kind {
                ComponentExternalKind::Component => &mut idx.comp,
                ComponentExternalKind::Module => &mut idx.module,
                ComponentExternalKind::Type =>  &mut idx.comp_type,
                ComponentExternalKind::Instance =>  &mut idx.comp_inst,
                ComponentExternalKind::Func => &mut idx.comp_func,
                ComponentExternalKind::Value => &mut idx.comp_val
            };
            let ref_id = space.next();
            space.assign(ref_id, ref_id);
            space.was_external();
            exports.export(
                exp.name.0,
                reencode.component_export_kind(exp.kind),
                exp.index,
                exp.ty.map(|ty| {
                    // TODO: This needs to be fixed
                    do_reencode(
                        ty,
                        RoundtripReencoder::component_type_ref,
                        reencode,
                        "component export",
                    )
                }),
            );
        }
        component.section(&exports);
    }

    fn internal_encode_component_instance(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        idx: &mut IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) {
        println!("internal_encode_component_instance");
        assert!(start + num - idx.comp_inst.curr_external() <= self.component_instance.len());
        let mut instances = wasm_encoder::ComponentInstanceSection::new();
        for i in 0..num {
            let id = idx.comp_inst.id_for(start + i);
            let instance = &self.component_instance[id];
            match instance {
                ComponentInstance::Instantiate {
                    component_index,
                    args,
                } => {
                    // TODO: This needs to be fixed
                    instances.instantiate(
                        *component_index,
                        args.iter().map(|arg| {
                            (
                                arg.name,
                                reencode.component_export_kind(arg.kind),
                                arg.index,
                            )
                        }),
                    );
                }
                ComponentInstance::FromExports(export) => {
                    instances.export_items(export.iter().map(|value| {
                        // TODO: This needs to be fixed (value.kind)
                        (
                            value.name.0,
                            reencode.component_export_kind(value.kind),
                            value.index,
                        )
                    }));
                }
            }
            idx.comp_inst.assign(start + i, id);
        }
        component.section(&instances);
    }

    fn internal_encode_core_instance(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        idx: &mut IdxSpaces,
    ) {
        println!("internal_encode_core_instance");
        assert!(start + num <= self.instances.len());
        let mut instances = wasm_encoder::InstanceSection::new();
        for i in 0..num {
            let id = idx.core_inst.id_for(start + i);
            let instance = &self.instances[id];
            match instance {
                Instance::Instantiate { module_index, args } => {
                    // TODO: This needs to be fixed
                    instances.instantiate(
                        *module_index,
                        args.iter()
                            .map(|arg| (arg.name, ModuleArg::Instance(arg.index))),
                    );
                }
                Instance::FromExports(exports) => {
                    instances.export_items(exports.iter().map(|export| {
                        // TODO: This needs to be fixed (export.kind)
                        (
                            export.name,
                            wasm_encoder::ExportKind::from(export.kind),
                            export.index,
                        )
                    }));
                }
            }
            idx.core_inst.assign(start + i, id);
        }
        component.section(&instances);
    }

    fn internal_encode_alias(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        idx: &mut IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) {
        assert!(
            start + num <= self.alias.items.len(),
            "{start} + {num} <= {}",
            self.alias.items.len()
        );
        let mut alias = ComponentAliasSection::new();
        for i in 0..num {
            let a = &self.alias.items[start + i];
            // TODO: This needs to be fixed (what does the alias point to?)
            let (space, fixed_alias) = match a {
                ComponentAlias::InstanceExport { kind, .. } => match kind {
                    ComponentExternalKind::Module => (Some(&mut idx.module), a.clone()),
                    ComponentExternalKind::Func => (Some(&mut idx.comp_func), a.clone()),
                    ComponentExternalKind::Value => (Some(&mut idx.comp_val), a.clone()),
                    ComponentExternalKind::Type => (Some(&mut idx.comp_type), a.clone()),
                    ComponentExternalKind::Instance => (Some(&mut idx.comp_inst), a.clone()),
                    ComponentExternalKind::Component => (Some(&mut idx.comp), a.clone()),
                },
                ComponentAlias::CoreInstanceExport { kind, .. } => match kind {
                    ExternalKind::Func |
                    ExternalKind::Memory |
                    ExternalKind::Global |
                    ExternalKind::Tag |
                    ExternalKind::Table => (None, a.clone()),
                }
                ComponentAlias::Outer { kind, .. } => match kind {
                    ComponentOuterAliasKind::CoreModule => (Some(&mut idx.module), a.clone()),
                    ComponentOuterAliasKind::CoreType => (Some(&mut idx.core_type), a.clone()),
                    ComponentOuterAliasKind::Type => (Some(&mut idx.comp_type), a.clone()),
                    ComponentOuterAliasKind::Component => (Some(&mut idx.comp), a.clone()),
                }
            };
            if let Some(space) = space {
                let ref_id = space.next();
                space.assign(ref_id, ref_id);
                space.was_external();
            }

            alias.alias(process_alias(&fixed_alias, reencode));
        }
        component.section(&alias);
    }

    fn internal_encode_canon(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        idx: &mut IdxSpaces,
        reencode: &mut RoundtripReencoder,
    ) {
        assert!(start + num <= self.canons.items.len());
        let mut canon_sec = wasm_encoder::CanonicalFunctionSection::new();
        for i in 0..num {
            let id = idx.core_func.id_for(start + i);
            let canon = &self.canons.items[id];
            match canon {
                CanonicalFunction::Lift {
                    core_func_index,
                    type_index,
                    options,
                } => {
                    // TODO: This needs to be fixed
                    canon_sec.lift(
                        *core_func_index,
                        *type_index,
                        options.iter().map(|canon| {
                            do_reencode(
                                *canon,
                                RoundtripReencoder::canonical_option,
                                reencode,
                                "canonical option",
                            )
                        }),
                    );
                }
                CanonicalFunction::Lower {
                    func_index,
                    options,
                } => {
                    // TODO: This needs to be fixed
                    canon_sec.lower(
                        *func_index,
                        options.iter().map(|canon| {
                            do_reencode(
                                *canon,
                                RoundtripReencoder::canonical_option,
                                reencode,
                                "canonical option",
                            )
                        }),
                    );
                }
                CanonicalFunction::ResourceNew { resource } => {
                    // TODO: This needs to be fixed
                    canon_sec.resource_new(*resource);
                }
                CanonicalFunction::ResourceDrop { resource } => {
                    // TODO: This needs to be fixed
                    canon_sec.resource_drop(*resource);
                }
                CanonicalFunction::ResourceRep { resource } => {
                    // TODO: This needs to be fixed
                    canon_sec.resource_rep(*resource);
                }
                CanonicalFunction::ResourceDropAsync { resource } => {
                    // TODO: This needs to be fixed
                    canon_sec.resource_drop_async(*resource);
                }
                CanonicalFunction::ThreadAvailableParallelism => {
                    canon_sec.thread_available_parallelism();
                }
                CanonicalFunction::BackpressureSet => {
                    canon_sec.backpressure_set();
                }
                CanonicalFunction::TaskReturn { result, options } => {
                    // TODO: This needs to be fixed
                    let options = options
                        .iter()
                        .cloned()
                        .map(|v| v.into())
                        .collect::<Vec<_>>();
                    let result = result.map(|v| v.into());
                    canon_sec.task_return(result, options);
                }
                CanonicalFunction::Yield { async_ } => {
                    canon_sec.yield_(*async_);
                }
                CanonicalFunction::WaitableSetNew => {
                    canon_sec.waitable_set_new();
                }
                CanonicalFunction::WaitableSetWait { async_, memory } => {
                    canon_sec.waitable_set_wait(*async_, *memory);
                }
                CanonicalFunction::WaitableSetPoll { async_, memory } => {
                    canon_sec.waitable_set_poll(*async_, *memory);
                }
                CanonicalFunction::WaitableSetDrop => {
                    canon_sec.waitable_set_drop();
                }
                CanonicalFunction::WaitableJoin => {
                    canon_sec.waitable_join();
                }
                CanonicalFunction::SubtaskDrop => {
                    canon_sec.subtask_drop();
                }
                CanonicalFunction::StreamNew { ty } => {
                    // TODO: This needs to be fixed
                    canon_sec.stream_new(*ty);
                }
                CanonicalFunction::StreamRead { ty, options } => {
                    // TODO: This needs to be fixed
                    canon_sec.stream_read(
                        *ty,
                        options
                            .into_iter()
                            .map(|t| {
                                do_reencode(
                                    *t,
                                    RoundtripReencoder::canonical_option,
                                    reencode,
                                    "canonical option",
                                )
                            })
                            .collect::<Vec<wasm_encoder::CanonicalOption>>(),
                    );
                }
                CanonicalFunction::StreamWrite { ty, options } => {
                    // TODO: This needs to be fixed
                    canon_sec.stream_write(
                        *ty,
                        options
                            .into_iter()
                            .map(|t| {
                                do_reencode(
                                    *t,
                                    RoundtripReencoder::canonical_option,
                                    reencode,
                                    "canonical option",
                                )
                            })
                            .collect::<Vec<wasm_encoder::CanonicalOption>>(),
                    );
                }
                CanonicalFunction::StreamCancelRead { ty, async_ } => {
                    // TODO: This needs to be fixed
                    canon_sec.stream_cancel_read(*ty, *async_);
                }
                CanonicalFunction::StreamCancelWrite { ty, async_ } => {
                    // TODO: This needs to be fixed
                    canon_sec.stream_cancel_write(*ty, *async_);
                }
                CanonicalFunction::FutureNew { ty } => {
                    // TODO: This needs to be fixed
                    canon_sec.future_new(*ty);
                }
                CanonicalFunction::FutureRead { ty, options } => {
                    // TODO: This needs to be fixed
                    canon_sec.future_read(
                        *ty,
                        options
                            .into_iter()
                            .map(|t| {
                                do_reencode(
                                    *t,
                                    RoundtripReencoder::canonical_option,
                                    reencode,
                                    "canonical option",
                                )
                            })
                            .collect::<Vec<wasm_encoder::CanonicalOption>>(),
                    );
                }
                CanonicalFunction::FutureWrite { ty, options } => {
                    // TODO: This needs to be fixed
                    canon_sec.future_write(
                        *ty,
                        options
                            .into_iter()
                            .map(|t| {
                                do_reencode(
                                    *t,
                                    RoundtripReencoder::canonical_option,
                                    reencode,
                                    "canonical option",
                                )
                            })
                            .collect::<Vec<wasm_encoder::CanonicalOption>>(),
                    );
                }
                CanonicalFunction::FutureCancelRead { ty, async_ } => {
                    // TODO: This needs to be fixed
                    canon_sec.future_cancel_read(*ty, *async_);
                }
                CanonicalFunction::FutureCancelWrite { ty, async_ } => {
                    // TODO: This needs to be fixed
                    canon_sec.future_cancel_write(*ty, *async_);
                }
                CanonicalFunction::ErrorContextNew { options } => {
                    // TODO: This needs to be fixed
                    canon_sec.error_context_new(
                        options
                            .into_iter()
                            .map(|t| {
                                do_reencode(
                                    *t,
                                    RoundtripReencoder::canonical_option,
                                    reencode,
                                    "canonical option",
                                )
                            })
                            .collect::<Vec<wasm_encoder::CanonicalOption>>(),
                    );
                }
                CanonicalFunction::ErrorContextDebugMessage { options } => {
                    // TODO: This needs to be fixed
                    canon_sec.error_context_debug_message(
                        options
                            .into_iter()
                            .map(|t| {
                                do_reencode(
                                    *t,
                                    RoundtripReencoder::canonical_option,
                                    reencode,
                                    "canonical option",
                                )
                            })
                            .collect::<Vec<wasm_encoder::CanonicalOption>>(),
                    );
                }
                CanonicalFunction::ErrorContextDrop => {
                    canon_sec.error_context_drop();
                }
                CanonicalFunction::ThreadSpawnRef { func_ty_index } => {
                    // TODO: This needs to be fixed
                    canon_sec.thread_spawn_ref(*func_ty_index);
                }
                CanonicalFunction::ThreadSpawnIndirect {
                    func_ty_index,
                    table_index,
                } => {
                    // TODO: This needs to be fixed
                    canon_sec.thread_spawn_indirect(*func_ty_index, *table_index);
                }
                CanonicalFunction::TaskCancel => {
                    canon_sec.task_cancel();
                }
                CanonicalFunction::ContextGet(i) => {
                    canon_sec.context_get(*i);
                }
                CanonicalFunction::ContextSet(i) => {
                    canon_sec.context_set(*i);
                }
                CanonicalFunction::SubtaskCancel { async_ } => {
                    canon_sec.subtask_cancel(*async_);
                }
                CanonicalFunction::StreamDropReadable { ty } => {
                    // TODO: This needs to be fixed
                    canon_sec.stream_drop_readable(*ty);
                }
                CanonicalFunction::StreamDropWritable { ty } => {
                    // TODO: This needs to be fixed
                    canon_sec.stream_drop_writable(*ty);
                }
                CanonicalFunction::FutureDropReadable { ty } => {
                    // TODO: This needs to be fixed
                    canon_sec.future_drop_readable(*ty);
                }
                CanonicalFunction::FutureDropWritable { ty } => {
                    // TODO: This needs to be fixed
                    canon_sec.future_drop_writable(*ty);
                }
            }
            idx.core_func.assign(start + i, id);
        }
        component.section(&canon_sec);
    }

    fn internal_encode_start(&self, component: &mut wasm_encoder::Component) {
        // Should only be 1 start section
        assert_eq!(self.start_section.len(), 1);
        // TODO: This needs to be fixed (func_index)
        let start_fn = &self.start_section[0];
        let start_sec = wasm_encoder::ComponentStartSection {
            function_index: start_fn.func_index,
            args: start_fn.arguments.iter(),
            results: start_fn.results,
        };
        component.section(&start_sec);
    }

    fn lookup_component_val_type(&self, ty: ComponentValType, idx: &mut IdxSpaces,
                                 component: &mut wasm_encoder::Component,
                                 reencode: &mut RoundtripReencoder,) -> ComponentValType{
        if let ComponentValType::Type(ty_id) = ty {
            if idx.comp_type.ooo(ty_id as usize) {
                // we need to skip around and encode this type first!
                self.internal_encode_component_type(ty_id as usize, 1, component, idx, reencode);
            }
            ComponentValType::Type(idx.comp_type.lookup(ty_id as usize) as u32)
        } else {
            ty
        }
    }

    fn internal_encode_custom(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
    ) {
        assert!(start + num <= self.custom_sections.len());
        for i in 0..num {
            let section = &self
                .custom_sections
                .get_by_id(CustomSectionID((start + i) as u32));
            component.section(&wasm_encoder::CustomSection {
                name: std::borrow::Cow::Borrowed(section.name),
                data: section.data.clone(),
            });
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

    /// Emit the Component into a wasm binary file.
    pub fn emit_wasm(&mut self, file_name: &str) -> Result<(), std::io::Error> {
        let comp = self.encode_comp();
        let wasm = comp.finish();
        std::fs::write(file_name, wasm)?;
        Ok(())
    }

    /// Get Local Function ID by name
    // Note: returned absolute id here
    pub fn get_fid_by_name(&self, name: &str, module_idx: ModuleID) -> Option<FunctionID> {
        for (idx, func) in self.modules[*module_idx as usize]
            .functions
            .iter()
            .enumerate()
        {
            if let FuncKind::Local(l) = &func.kind {
                if let Some(n) = &l.body.name {
                    if n == name {
                        return Some(FunctionID(idx as u32));
                    }
                }
            }
        }
        None
    }
}
