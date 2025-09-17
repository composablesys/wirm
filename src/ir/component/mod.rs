#![allow(clippy::mut_range_bound)] // see https://github.com/rust-lang/rust-clippy/issues/6072
//! Intermediate Representation of a wasm component.

use crate::error::Error;
use crate::ir::component::alias::Aliases;
use crate::ir::component::canons::Canons;
use crate::ir::component::index_spaces::{ExternalItemKind, IdxSpaces, SpaceSubtype};
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
    do_reencode, process_alias,
};
use wasm_encoder::reencode::{Reencode, ReencodeComponent, RoundtripReencoder};
use wasm_encoder::{ComponentAliasSection, ModuleArg, ModuleSection, NestedComponentSection};
use wasmparser::{CanonicalFunction, ComponentAlias, ComponentExport, ComponentExternalKind, ComponentFuncType, ComponentImport, ComponentInstance, ComponentOuterAliasKind, ComponentStartFunction, ComponentType, ComponentTypeDeclaration, ComponentTypeRef, ComponentValType, CoreType, Encoding, ExternalKind, Instance, InstanceTypeDeclaration, Parser, Payload};

mod alias;
mod canons;
mod index_spaces;
pub mod types;

#[derive(Debug, Default)]
/// Intermediate Representation of a wasm component.
pub struct Component<'a> {
    /// Nested Components
    pub components: Vec<Component<'a>>,
    /// Modules
    pub modules: Vec<Module<'a>>,
    /// Component Types
    pub component_types: ComponentTypes<'a>,
    /// Component Instances
    pub component_instance: Vec<ComponentInstance<'a>>,
    /// Canons
    pub canons: Canons,

    /// Alias
    pub alias: Aliases<'a>,
    /// Imports
    pub imports: Vec<ComponentImport<'a>>,
    /// Exports
    pub exports: Vec<ComponentExport<'a>>,

    /// Core Types
    pub core_types: Vec<CoreType<'a>>,
    /// Core Instances
    pub instances: Vec<Instance<'a>>,

    // Tracks the index spaces of this component.
    indices: IdxSpaces,

    /// Custom sections
    pub custom_sections: CustomSections<'a>,
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

impl<'a> Component<'a> {
    /// Creates a new Empty Component
    pub fn new() -> Self {
        Self::default()
    }

    fn add_section(&mut self, outer: ComponentSection, inner: ExternalItemKind, idx: usize) -> usize {
        // get and save off the assumed id
        let assumed_id = self.indices.assign_assumed_id(&outer, &inner, idx);

        // add to section order list
        if self.sections[self.num_sections - 1].1 == outer {
            self.sections[self.num_sections - 1].0 += 1;
        } else {
            self.sections.push((1, outer));
        }

        println!("assumed: {:?}", assumed_id);
        assumed_id.unwrap_or_else(|| { idx })
    }

    /// Add a Module to this Component.
    pub fn add_module(&mut self, module: Module<'a>) -> ModuleID {
        let idx = self.modules.len();
        self.modules.push(module);
        let id = self.add_section(ComponentSection::Module, ExternalItemKind::NA, idx);

        ModuleID(id as u32)
    }

    /// Add a Global to this Component.
    pub fn add_globals(&mut self, global: Global, module_idx: ModuleID) -> GlobalID {
        self.modules[*module_idx as usize].globals.add(global)
    }

    pub fn add_import(&mut self, import: ComponentImport<'a>) -> u32 {
        let idx = self.imports.len();
        self.imports.push(import);
        let kind = ExternalItemKind::from(&import.ty);

        self.add_section(ComponentSection::ComponentImport, kind, idx) as u32
    }

    pub fn add_alias_func(&mut self, alias: ComponentAlias<'a>) -> (AliasFuncId, AliasId) {
        let kind = ExternalItemKind::from(&alias);
        print!("[add_alias_func] '{}', from instance {}, curr-len: {}, ",
                 if let ComponentAlias::InstanceExport {name, ..} | ComponentAlias::CoreInstanceExport {name, ..} = &alias {
                    name
                } else {
                    "no-name"
                },
                 if let ComponentAlias::InstanceExport {instance_index, ..} | ComponentAlias::CoreInstanceExport {instance_index, ..} = &alias {
                     &format!("{instance_index}")
                 } else {
                     "NA"
                 },
                self.canons.items.len()
        );
        let (item_id, alias_id) = self.alias.add(alias);
        let id = self.add_section(ComponentSection::Alias, kind, *alias_id as usize);
        println!("   --> @{}", id);

        (AliasFuncId(id as u32), alias_id)
    }

    pub fn add_canon_func(&mut self, canon: CanonicalFunction) -> CanonicalFuncId {
        print!("[add_canon_func] {:?}", canon);
        let idx = self.canons.add(canon).1;
        let id = self.add_section(ComponentSection::Canon, ExternalItemKind::NA, *idx as usize);
        println!("   --> @{}", id);

        CanonicalFuncId(id as u32)
    }

    pub(crate) fn add_component_type(
        &mut self,
        component_ty: ComponentType<'a>,
    ) -> (u32, ComponentTypeId) {
        let ids = self.component_types.add(component_ty);
        let id = self.add_section(ComponentSection::ComponentType, ExternalItemKind::NA, *ids.1 as usize);

        (id as u32, ids.1)
    }

    pub fn add_type_instance(
        &mut self,
        decls: Vec<InstanceTypeDeclaration<'a>>,
    ) -> (ComponentTypeInstanceId, ComponentTypeId) {
        let (ty_inst_id, ty_id) =
            self.add_component_type(ComponentType::Instance(decls.into_boxed_slice()));

        // almost account for aliased types!
        (
            ComponentTypeInstanceId(ty_inst_id),
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
            ComponentTypeFuncId(ty_inst_id),
            ty_id,
        )
    }

    pub fn add_core_instance(&mut self, instance: Instance<'a>) -> CoreInstanceId {
        let idx = self.instances.len();
        self.instances.push(instance);
        let id  = self.add_section(ComponentSection::CoreInstance, ExternalItemKind::NA, idx);
        println!("[add_core_instance] id: {id}");

        CoreInstanceId(id as u32)
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
        sections_added: u32
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

        // To track the index spaces
        let mut indices = IdxSpaces::new();

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
                    let num_imps = imports.len();
                    for (i, imp) in temp.iter().enumerate() {
                        let curr_idx = num_imps + i;
                        indices.assign_assumed_id(&ComponentSection::ComponentImport, &ExternalItemKind::from(&imp.ty), curr_idx);
                    }
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
                    let num_exps = exports.len();
                    for (i, exp) in temp.iter().enumerate() {
                        let curr_idx = num_exps + i;
                        indices.assign_assumed_id(&ComponentSection::ComponentExport, &ExternalItemKind::from(&exp.kind), curr_idx);
                    }
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
                    let num_insts = instances.len();
                    for i in 0..temp.len() {
                        let curr_idx = num_insts + i;
                        indices.assign_assumed_id(&ComponentSection::CoreInstance, &ExternalItemKind::NA, curr_idx);
                    }
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
                    let num_tys = core_types.len();
                    for i in 0..temp.len() {
                        let curr_idx = num_tys + i;
                        indices.assign_assumed_id(&ComponentSection::CoreType, &ExternalItemKind::NA, curr_idx);
                    }
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
                    let num_tys = component_types.len();
                    for i in 0..temp.len() {
                        let curr_idx = num_tys + i;
                        indices.assign_assumed_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, curr_idx);
                    }
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
                    let num_insts = component_instance.len();
                    for i in 0..temp.len() {
                        let curr_idx = num_insts + i;
                        indices.assign_assumed_id(&ComponentSection::ComponentInstance, &ExternalItemKind::NA, curr_idx);
                    }
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
                    let num_aliases = alias.len();
                    for (i, alias) in temp.iter().enumerate() {
                        let curr_idx = num_aliases + i;
                        indices.assign_assumed_id(&ComponentSection::Alias, &ExternalItemKind::from(alias), curr_idx);
                    }
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
                    let num_canons = canons.len();
                    for i in 0..temp.len() {
                        let curr_idx = num_canons + i;
                        indices.assign_assumed_id(&ComponentSection::Canon, &ExternalItemKind::NA, curr_idx);
                    }
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
                    indices.assign_assumed_id(&ComponentSection::Module, &ExternalItemKind::NA, modules.len());
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
                    indices.assign_assumed_id(&ComponentSection::Component, &ExternalItemKind::NA, components.len());
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

        println!("Number of core instances: {}", instances.len());
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
            indices,
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
        println!("\n\n==========================\n==== ENCODE COMPONENT ====\n==========================");
        let mut component = wasm_encoder::Component::new();
        let mut reencode = RoundtripReencoder;

        // TODO: Can I avoid this clone?
        let mut indices = self.indices.clone();
        indices.reset_ids();

        // Create a clone of the original sections to allow iterating over them (borrow issues)
        // TODO: Can I avoid this clone?
        let orig_sections = self.sections.clone();
        for (num, section) in orig_sections.iter() {
            let start_idx = indices.visit_section(section, *num as usize);
            match section {
                ComponentSection::Component => {
                    self.internal_encode_component(start_idx, *num as usize, &mut component, &mut indices);
                }
                ComponentSection::Module => {
                    self.internal_encode_module(start_idx, *num as usize, &mut component, &mut indices);
                }
                ComponentSection::CoreType => {
                    self.internal_encode_core_type(
                        start_idx,
                        *num as usize,
                        &mut component,
                        &mut reencode,
                        &mut indices
                    );
                }
                ComponentSection::ComponentType => {
                    println!("[ComponentType] called in main, target idx: {}", start_idx);
                    self.internal_encode_component_type(
                        start_idx,
                        *num as usize,
                        &mut component,
                        &mut reencode,
                        &mut indices
                    );
                }
                ComponentSection::ComponentImport => {
                    self.internal_encode_component_import(
                        start_idx,
                        *num as usize,
                        &mut component,
                        &mut reencode,
                        &mut indices
                    );
                }
                ComponentSection::ComponentExport => {
                    self.internal_encode_component_export(
                        start_idx,
                        *num as usize,
                        &mut component,
                        &mut reencode,
                        &mut indices
                    );
                }
                ComponentSection::ComponentInstance => {
                    self.internal_encode_component_instance(
                        start_idx,
                        *num as usize,
                        &mut component,
                        &mut reencode,
                        &mut indices
                    );
                }
                ComponentSection::CoreInstance => {
                    self.internal_encode_core_instance(
                        start_idx,
                        *num as usize,
                        &mut component,
                        &mut reencode,
                        &mut indices
                    );
                }
                ComponentSection::Alias => {
                    self.internal_encode_alias(
                        start_idx,
                        *num as usize,
                        &mut component,
                        &mut reencode,
                        &mut indices
                    );
                }
                ComponentSection::Canon => {
                    self.internal_encode_canon(
                        start_idx,
                        *num as usize,
                        &mut component,
                        &mut reencode,
                        &mut indices
                    );
                }
                ComponentSection::ComponentStartSection => {
                    self.internal_encode_start(&mut component);
                }
                ComponentSection::CustomSection => {
                    self.internal_encode_custom(
                        start_idx,
                        *num as usize,
                        &mut component,
                    );
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


        println!("\n\n====================\n==== END ENCODE ====\n====================");
        component
    }

    fn internal_encode_component(
        &mut self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        indices: &mut IdxSpaces
    ) {
        println!("\ninternal_encode_component[{start}]x{num}");
        assert!(start + num <= self.components.len());

        let section = ComponentSection::Component;
        for i in 0..num {
            let idx = start + i;
            let kind = ExternalItemKind::NA;
            if indices.is_encoded(&section, &kind, idx) { continue; }
            component.section(&NestedComponentSection(
                &self.components[idx].encode_comp(),
            ));
            // let id = idx.comp.id_for(comp_idx);
            println!("here: internal_encode_component");
            indices.assign_actual_id(&section, &kind, idx);
        }
    }

    fn internal_encode_module(
        &mut self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        indices: &mut IdxSpaces,
    ) {
        assert!(start + num <= self.modules.len());

        let section = ComponentSection::Module;
        for i in 0..num {
            let idx = start + i;
            let kind = ExternalItemKind::NA;
            if indices.is_encoded(&section, &kind, idx) { continue; }
            component.section(&ModuleSection(
                &self.modules[idx].encode_internal(false).0,
            ));
            // let id = idx.module.id_for(mod_idx);
            println!("here: internal_encode_module");
            indices.assign_actual_id(&section, &kind, idx);
        }
    }

    fn internal_encode_core_type(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        reencode: &mut RoundtripReencoder,
        indices: &mut IdxSpaces
    ) {
        assert!(num + start <= self.core_types.len());
        let mut type_section = wasm_encoder::CoreTypeSection::new();

        let section = ComponentSection::CoreType;
        for i in 0..num {
            let idx = start + i;
            let kind = ExternalItemKind::NA;
            if indices.is_encoded(&section, &kind, idx) { continue; }

            match &self.core_types[idx] {
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
                    // TODO: This *might* need to be fixed, but I'm unsure
                    let enc = type_section.ty();
                    convert_module_type_declaration(module, enc, reencode);
                }
            }
            // let id = idx.core_type.id_for(ty_idx);
            println!("here: internal_encode_core_type");
            indices.assign_actual_id(&section, &kind, idx);
        }
        component.section(&type_section);
    }

    fn internal_encode_component_type(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        reencode: &mut RoundtripReencoder,
        indices: &mut IdxSpaces
    ) {
        println!("[internal_encode_component_type] {start} + {num} = {} <= {}", num + start, self.component_types.items.len());
        assert!(num + start <= self.component_types.items.len(), "{num} + {start} = {} <= {}", num + start, self.component_types.items.len());

        let section = ComponentSection::ComponentType;
        let mut component_ty_section = wasm_encoder::ComponentTypeSection::new();
        for i in 0..num {
            let idx = start + i;
            let kind = ExternalItemKind::NA;
            if indices.is_encoded(&section, &kind, idx) { continue; }

            match &self.component_types.items[idx] {
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
                                        *ty, component, reencode, indices
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
                                            ty, component, reencode, indices
                                        );
                                        reencode.component_val_type(fixed_ty)
                                    }),
                                    variant.refines,
                                )
                            }))
                        }
                        wasmparser::ComponentDefinedType::List(l) => {
                            let fixed_ty = self.lookup_component_val_type(
                                *l, component, reencode, indices
                            );
                            enc.list(reencode.component_val_type(fixed_ty))
                        }
                        wasmparser::ComponentDefinedType::Tuple(tup) => enc.tuple(
                            tup.iter()
                                .map(|val_type| {
                                    let fixed_ty = self.lookup_component_val_type(
                                        *val_type, component, reencode, indices
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
                                *opt, component, reencode, indices
                            );
                            enc.option(reencode.component_val_type(fixed_ty))
                        }
                        wasmparser::ComponentDefinedType::Result { ok, err } => enc.result(
                            ok.map(|val_type| {
                                let fixed_ty = self.lookup_component_val_type(
                                    val_type, component, reencode, indices
                                );
                                reencode.component_val_type(fixed_ty)
                            }),
                            err.map(|val_type| {
                                let fixed_ty = self.lookup_component_val_type(
                                    val_type, component, reencode, indices
                                );
                                reencode.component_val_type(fixed_ty)
                            }),
                        ),
                        wasmparser::ComponentDefinedType::Own(u) => {
                            let id = if let Some(id) = indices.lookup_actual_id(&section, &kind, *u as usize) {
                                // has already been encoded
                                *id
                            } else {
                                // we need to skip around and encode this type first!
                                self.internal_encode_component_type(*u as usize, 1, component, reencode, indices);
                                indices.lookup_actual_id_or_panic(&section, &kind, *u as usize)
                            };
                            enc.own(id as u32)
                        },
                        wasmparser::ComponentDefinedType::Borrow(u) => {
                            let id = if let Some(id) = indices.lookup_actual_id(&section, &kind, *u as usize) {
                                // has already been encoded
                                *id
                            } else {
                                // we need to skip around and encode this type first!
                                self.internal_encode_component_type(*u as usize, 1, component, reencode, indices);
                                indices.lookup_actual_id_or_panic(&section, &kind, *u as usize)
                            };
                            enc.borrow(id as u32)
                        },
                        wasmparser::ComponentDefinedType::Future(opt) => match opt {
                            Some(u) => {
                                let fixed_ty = self.lookup_component_val_type(
                                    *u, component, reencode, indices
                                );
                                enc.future(Some(reencode.component_val_type(fixed_ty)))
                            },
                            None => enc.future(None),
                        },
                        wasmparser::ComponentDefinedType::Stream(opt) => match opt {
                            Some(u) => {
                                let fixed_ty = self.lookup_component_val_type(
                                    *u, component, reencode, indices
                                );
                                enc.stream(Some(reencode.component_val_type(fixed_ty)))
                            },
                            None => enc.stream(None),
                        },
                        wasmparser::ComponentDefinedType::FixedSizeList(ty, i) => {
                            let fixed_ty = self.lookup_component_val_type(
                                *ty, component, reencode, indices
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
                                p.1, component, reencode, indices
                            );
                            (p.0, reencode.component_val_type(fixed_ty))
                        },
                    ));
                    enc.result(func_ty.result.map(|v| {
                        let fixed_ty = self.lookup_component_val_type(
                            v, component, reencode, indices
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
                                let fixed_ty = self.fix_component_type_ref(*ty, component, reencode, indices);

                                let ty = do_reencode(
                                    fixed_ty,
                                    RoundtripReencoder::component_type_ref,
                                    reencode,
                                    "component type",
                                );
                                new_comp.export(name.0, ty);
                            }
                            ComponentTypeDeclaration::Import(imp) => {
                                let fixed_ty = self.fix_component_type_ref(imp.ty, component, reencode, indices);

                                let ty = do_reencode(
                                    fixed_ty,
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
            println!("here: internal_encode_component_type");
            indices.assign_actual_id(&section, &kind, idx);
        }
        component.section(&component_ty_section);
    }

    fn internal_encode_component_import(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        reencode: &mut RoundtripReencoder,
        indices: &mut IdxSpaces
    ) {
        println!("\ninternal_encode_component_import[{start}]x{num}");
        assert!(start + num <= self.imports.len());
        let mut imports = wasm_encoder::ComponentImportSection::new();

        let section = ComponentSection::ComponentImport;
        for i in 0..num {
            let idx = start + i;
            let imp = &self.imports[idx];
            let kind = ExternalItemKind::from(&imp.ty);
            if indices.is_encoded(&section, &kind, idx) { continue; }

            // TODO: Verify this is correct!
            indices.assign_actual_id(
                &section,
                &kind,
                idx
            );

            let fixed_ty = self.fix_component_type_ref(imp.ty, component, reencode, indices);
            let ty = do_reencode(
                fixed_ty,
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
        reencode: &mut RoundtripReencoder,
        indices: &mut IdxSpaces
    ) {
        println!("\ninternal_encode_component_export[{start}]x{num}");
        assert!(start + num <= self.exports.len());
        let mut exports = wasm_encoder::ComponentExportSection::new();

        let section = ComponentSection::ComponentExport;
        for i in 0..num {
            let idx = start + i;
            let exp = &self.exports[idx];
            let kind = ExternalItemKind::from(&exp.kind);
            println!("internal_encode_component_export: {:?}::{:?}", section, kind);
            if indices.is_encoded(&section, &kind, idx) { continue; }
            println!("here: internal_encode_component_export");

            let res = exp.ty.map(|ty| {
                let fixed_ty = self.fix_component_type_ref(ty, component, reencode, indices);
                do_reencode(
                    fixed_ty,
                    RoundtripReencoder::component_type_ref,
                    reencode,
                    "component export",
                )
            });

            // TODO: Verify this is correct!
            indices.assign_actual_id(
                &section,
                &kind,
                idx
            );

            exports.export(
                exp.name.0,
                reencode.component_export_kind(exp.kind),
                exp.index,
                res,
            );
        }
        component.section(&exports);
    }

    fn internal_encode_component_instance(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        reencode: &mut RoundtripReencoder,
        indices: &mut IdxSpaces
    ) {
        println!("\ninternal_encode_component_instance[{start}]x{num}");
        assert!(start + num <= self.component_instance.len());
        let mut instances = wasm_encoder::ComponentInstanceSection::new();

        let section = ComponentSection::ComponentInstance;
        for i in 0..num {
            let idx = start + i;
            let kind = ExternalItemKind::NA;
            if indices.is_encoded(&section, &kind, idx) { continue; }
            let instance = &self.component_instance[idx];
            match instance {
                ComponentInstance::Instantiate {
                    component_index,
                    args,
                } => {
                    let section = ComponentSection::Component;
                    let kind = ExternalItemKind::NA;
                    let id = if let Some(id) = indices.lookup_actual_id(&section, &kind, *component_index as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        // self.internal_encode_component(*component_index as usize, 1, component, indices);
                        // indices.lookup_actual_id_or_panic(&section, &kind, *component_index as usize)
                        panic!("Issue with borrowing mutable self...")
                    };
                    instances.instantiate(
                        id as u32,
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
            // let id = idx.comp_inst.id_for(inst_idx);
            println!("here: internal_encode_component_instance");
            indices.assign_actual_id(&section, &kind, idx);
        }
        component.section(&instances);
    }

    fn internal_encode_core_instance(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        reencode: &mut RoundtripReencoder,
        indices: &mut IdxSpaces
    ) {
        println!("\ninternal_encode_core_instance[{start}]x{num}");
        // assert!(start + num <= self.instances.len(), "{start} + {num} <= {}", self.instances.len());
        let mut instances = wasm_encoder::InstanceSection::new();

        let section = ComponentSection::CoreInstance;
        for i in 0..num {
            let idx = start + i;
            let kind = ExternalItemKind::NA;
            if indices.is_encoded(&section, &kind, idx) {
                println!("[internal_encode_core_instance] SKIPPED: {:?}@{}", kind, idx);
                continue;
            }
            let instance = &self.instances[idx];
            match instance {
                Instance::Instantiate { module_index, args } => {
                    instances.instantiate(
                        *module_index,
                        args.iter()
                            .map(|arg| {
                                let id = if let Some(id) = indices.lookup_actual_id(&section, &kind, arg.index as usize) {
                                    // has already been encoded
                                    println!("[internal_encode_core_instance::{:?}] instantiating module #{}, already encoded dependency: {}-->{}", kind, module_index, arg.index, id);
                                    *id
                                } else {
                                    // we need to skip around and encode this type first!
                                    println!("[internal_encode_core_instance::{:?}] instantiating module #{}, must encode dependency: @{}", kind, module_index, arg.index);
                                    // TODO -- the assumed_id seems wrong! should be 17 i think (check instrument funcs)
                                    let (_, idx) = indices.index_from_assumed_id(&section, &kind, arg.index as usize);
                                    self.internal_encode_core_instance(idx, 1, component, reencode, indices);
                                    indices.lookup_actual_id_or_panic(&section, &kind, arg.index as usize)
                                };
                                (arg.name, ModuleArg::Instance(id as u32))
                            }),
                    );
                }
                Instance::FromExports(exports) => {
                    instances.export_items(exports.iter().map(|export| {
                        // TODO: This needs to be fixed (export.kind)
                        let section = ComponentSection::ComponentExport;
                        let kind = ExternalItemKind::from(&export.kind);
                        println!("[{:?}:{:?}] handling: {}", section, kind, export.name);
                        let id = if let Some(id) = indices.lookup_actual_id(&section, &kind, export.index as usize) {
                            // has already been encoded
                            println!("[internal_encode_core_instance::{:?}] already encoded dependency: {}-->{}", kind, export.index, id);
                            *id
                        } else {
                            // we need to skip around and encode this type first!
                            println!("[internal_encode_core_instance::{:?}] must encode dependency: @{}", kind, export.index);
                            let (idx_sect, idx_kind) = match &export.kind {
                                ExternalKind::Func => (ComponentSection::Canon, ExternalItemKind::NA),
                                _ => (section.clone(), kind)
                            };
                            let (_, idx) = indices.index_from_assumed_id(&idx_sect, &idx_kind, export.index as usize);
                            println!("    ==> using idx: {idx}");
                            self.internal_encode_canon(idx, 1, component, reencode, indices);
                            indices.lookup_actual_id_or_panic(&section, &kind, export.index as usize)
                        };
                        (
                            export.name,
                            wasm_encoder::ExportKind::from(export.kind),
                            id as u32,
                        )
                    }));
                }
            }
            // let id = idx.core_inst.id_for(instance_idx);
            println!("here: internal_encode_core_instance");
            indices.assign_actual_id(&section, &kind, idx);
        }
        component.section(&instances);
    }

    fn internal_encode_alias(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        reencode: &mut RoundtripReencoder,
        indices: &mut IdxSpaces
    ) {
        println!("\ninternal_encode_alias[{start}]x{num}");
        assert!(
            start + num <= self.alias.items.len(),
            "{start} + {num} <= {}",
            self.alias.items.len()
        );
        let mut alias = ComponentAliasSection::new();

        let section = ComponentSection::Alias;
        for i in 0..num {
            let idx = start + i;
            let a = &self.alias.items[idx];
            let kind = ExternalItemKind::from(a);
            println!("here: internal_encode_alias");
            if indices.is_encoded(&section, &kind, idx) {
                println!("[internal_encode_alias] SKIPPED: {:?}@{}", kind, idx);
                continue;
            }
            // TODO: This needs to be fixed (what does the alias point to?)
            indices.assign_actual_id(
                &section,
                &kind,
                idx
            );

            alias.alias(process_alias(&a, reencode));
        }
        component.section(&alias);
    }

    fn internal_encode_canon(
        &self,
        start: usize,
        num: usize,
        component: &mut wasm_encoder::Component,
        reencode: &mut RoundtripReencoder,
        indices: &mut IdxSpaces
    ) {
        println!("\ninternal_encode_canon[{start}]x{num}");
        assert!(start + num <= self.canons.items.len());
        let mut canon_sec = wasm_encoder::CanonicalFunctionSection::new();

        let section = ComponentSection::Canon;
        for i in 0..num {
            // let id = idx.core_func.id_for(start + i);
            let idx = start + i;
            let kind = ExternalItemKind::NA;
            if indices.is_encoded(&section, &kind, idx) { continue; }
            let canon = &self.canons.items[idx];
            match canon {
                CanonicalFunction::Lift {
                    core_func_index,
                    type_index,
                    options,
                } => {
                    let func_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::Canon, &ExternalItemKind::NA, *core_func_index as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.comp_func.index_from_assumed_id(&section, *core_func_index as usize).unwrap();
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *core_func_index as usize)
                    };
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *type_index as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *type_index as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *type_index as usize)
                    };
                    canon_sec.lift(
                        func_id as u32,
                        ty_id as u32,
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
                    // TODO -- this assumes that we're needing to lookup as an alias!
                    let func_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::Alias, &ExternalItemKind::CompFunc, *func_index as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (ty, idx) = indices.index_from_assumed_id(&ComponentSection::Alias, &ExternalItemKind::CompFunc, *func_index as usize);
                        println!("    ==> using idx: {idx}");
                        match ty {
                            SpaceSubtype::Export => self.internal_encode_component_export(idx, 1, component, reencode, indices),
                            SpaceSubtype::Import => self.internal_encode_component_import(idx, 1, component, reencode, indices),
                            SpaceSubtype::Alias => self.internal_encode_alias(idx, 1, component, reencode, indices),
                            SpaceSubtype::Main => self.internal_encode_canon(idx, 1, component, reencode, indices)
                        }
                        indices.lookup_actual_id_or_panic(&ComponentSection::Alias, &ExternalItemKind::CompFunc, *func_index as usize)
                    };
                    canon_sec.lower(
                        func_id as u32,
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
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *resource as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *resource as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *resource as usize)
                    };
                    canon_sec.resource_new(ty_id as u32);
                }
                CanonicalFunction::ResourceDrop { resource } => {
                    println!("[CanonicalFunction::ResourceDrop] here@{resource}");
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *resource as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *resource as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_core_type(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&ComponentSection::ComponentType, &kind, *resource as usize)
                        // self.internal_encode_canon(*resource as usize, 1, component, reencode, indices);
                        // indices.lookup_actual_id_or_panic(&section, &kind, *resource as usize)
                    };
                    canon_sec.resource_drop(ty_id as u32);
                }
                CanonicalFunction::ResourceRep { resource } => {
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *resource as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *resource as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *resource as usize)
                    };
                    canon_sec.resource_rep(ty_id as u32);
                }
                CanonicalFunction::ResourceDropAsync { resource } => {
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *resource as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *resource as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *resource as usize)
                    };
                    canon_sec.resource_drop_async(ty_id as u32);
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
                    let result = result.map(|v| {
                        let fixed_ty = self.lookup_component_val_type(
                            v, component, reencode, indices
                        );
                        fixed_ty.into()
                    });
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
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
                    };
                    canon_sec.stream_new(ty_id as u32);
                }
                CanonicalFunction::StreamRead { ty, options } => {
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
                    };
                    canon_sec.stream_read(
                        ty_id as u32,
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
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
                    };
                    canon_sec.stream_write(
                        ty_id as u32,
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
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
                    };
                    canon_sec.stream_cancel_read(ty_id as u32, *async_);
                }
                CanonicalFunction::StreamCancelWrite { ty, async_ } => {
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
                    };
                    canon_sec.stream_cancel_write(ty_id as u32, *async_);
                }
                CanonicalFunction::FutureNew { ty } => {
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
                    };
                    canon_sec.future_new(ty_id as u32);
                }
                CanonicalFunction::FutureRead { ty, options } => {
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
                    };
                    canon_sec.future_read(
                        ty_id as u32,
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
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
                    };
                    canon_sec.future_write(
                        ty_id as u32,
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
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
                    };
                    canon_sec.future_cancel_read(ty_id as u32, *async_);
                }
                CanonicalFunction::FutureCancelWrite { ty, async_ } => {
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
                    };
                    canon_sec.future_cancel_write(ty_id as u32, *async_);
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
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *func_ty_index as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *func_ty_index as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *func_ty_index as usize)
                    };
                    canon_sec.thread_spawn_ref(ty_id as u32);
                }
                CanonicalFunction::ThreadSpawnIndirect {
                    func_ty_index,
                    table_index,
                } => {
                    // TODO: This needs to be fixed
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *func_ty_index as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *func_ty_index as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *func_ty_index as usize)
                    };
                    canon_sec.thread_spawn_indirect(ty_id as u32, *table_index);
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
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
                    };
                    canon_sec.stream_drop_readable(ty_id as u32);
                }
                CanonicalFunction::StreamDropWritable { ty } => {
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
                    };
                    canon_sec.stream_drop_writable(ty_id as u32);
                }
                CanonicalFunction::FutureDropReadable { ty } => {
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
                    };
                    canon_sec.future_drop_readable(ty_id as u32);
                }
                CanonicalFunction::FutureDropWritable { ty } => {
                    let ty_id = if let Some(id) = indices.lookup_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *ty as usize) {
                        // has already been encoded
                        *id
                    } else {
                        // we need to skip around and encode this type first!
                        println!("here");
                        let (_, idx) = indices.index_from_assumed_id(&section, &kind, *ty as usize);
                        println!("    ==> using idx: {idx}");
                        self.internal_encode_canon(idx, 1, component, reencode, indices);
                        indices.lookup_actual_id_or_panic(&section, &kind, *ty as usize)
                    };
                    canon_sec.future_drop_writable(ty_id as u32);
                }
            }
            println!("here: internal_encode_canon");
            indices.assign_actual_id(&section, &kind, idx);
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

    fn fix_component_type_ref(&self, ty: ComponentTypeRef,
                              component: &mut wasm_encoder::Component,
                              reencode: &mut RoundtripReencoder,
                              indices: &mut IdxSpaces) -> ComponentTypeRef {
        match ty {
            ComponentTypeRef::Module(id) => {
                let section = ComponentSection::Module;
                let kind = ExternalItemKind::NA;
                let id = if let Some(id) = indices.lookup_actual_id(&section, &kind, id as usize) {
                    // has already been encoded
                    *id
                } else {
                    // we need to skip around and encode this type first!
                    println!("here");
                    // self.internal_encode_module(id as usize, 1, component, indices);
                    // indices.lookup_actual_id_or_panic(&section, &kind, id as usize)
                    panic!("couldn't encode due to borrow issues")
                };
                ComponentTypeRef::Module(id as u32)
            }
            ComponentTypeRef::Value(ty) => ComponentTypeRef::Value(self.lookup_component_val_type(ty, component, reencode, indices)),
            ComponentTypeRef::Type(_) => ty, // nothing to do
            ComponentTypeRef::Func(id) |
            ComponentTypeRef::Instance(id) => {
                // TODO -- no idea if this section is right...
                let section = ComponentSection::ComponentType;
                let kind = ExternalItemKind::NA;
                let id = if let Some(id) = indices.lookup_actual_id(&section, &kind, id as usize) {
                    // has already been encoded
                    *id
                } else {
                    // we need to skip around and encode this type first!
                    println!("from comp-type-ref");
                    let (_, idx) = indices.index_from_assumed_id(&section, &kind, id as usize);
                    println!("    ==> using idx: {idx}");
                    self.internal_encode_component_type(idx, 1, component, reencode, indices);
                    indices.lookup_actual_id_or_panic(&section, &kind, id as usize)
                };
                if matches!(ty, ComponentTypeRef::Func(_)) {
                    ComponentTypeRef::Func(id as u32)
                } else {
                    ComponentTypeRef::Instance(id as u32)
                }
            }
            ComponentTypeRef::Component(id) => {
                // TODO -- no idea if this section is right...
                let section = ComponentSection::ComponentType;
                let kind = ExternalItemKind::NA;
                let id = if let Some(id) = indices.lookup_actual_id(&section, &kind, id as usize) {
                    // has already been encoded
                    *id
                } else {
                    // we need to skip around and encode this type first!
                    println!("here");
                    let (_, idx) = indices.index_from_assumed_id(&section, &kind, id as usize);
                    println!("    ==> using idx: {idx}");
                    self.internal_encode_component_type(idx, 1, component, reencode, indices);
                    indices.lookup_actual_id_or_panic(&section, &kind, id as usize)
                };
                ComponentTypeRef::Func(id as u32)
            }
        }
    }

    fn lookup_component_val_type(&self, ty: ComponentValType,
                                 component: &mut wasm_encoder::Component,
                                 reencode: &mut RoundtripReencoder,
                                 indices: &mut IdxSpaces) -> ComponentValType{
        let section = ComponentSection::ComponentType;
        let kind = ExternalItemKind::NA;

        if let ComponentValType::Type(ty_id) = ty {
            let id = if let Some(id) = indices.lookup_actual_id(&section, &kind, ty_id as usize) {
                // has already been encoded
                *id
            } else {
                // we need to skip around and encode this type first!
                println!("here");
                let (_, idx) = indices.index_from_assumed_id(&section, &kind, ty_id as usize);
                println!("    ==> using idx: {idx}");
                self.internal_encode_component_type(idx, 1, component, reencode, indices);
                indices.lookup_actual_id_or_panic(&section, &kind, ty_id as usize)
            };
            ComponentValType::Type(id as u32)
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
