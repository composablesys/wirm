#![allow(clippy::mut_range_bound)] // see https://github.com/rust-lang/rust-clippy/issues/6072
//! Intermediate Representation of a wasm component.

use wasm_encoder::reencode::{Reencode, ReencodeComponent, RoundtripReencoder};
use wasm_encoder::{ComponentAliasSection, ModuleArg, ModuleSection, NestedComponentSection};
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance,
    ComponentStartFunction, ComponentType, ComponentTypeDeclaration, CoreType, Encoding, Instance,
    Parser, Payload,
};

use crate::error::Error;
use crate::ir::helpers::{
    print_alias, print_component_export, print_component_import, print_component_type,
    print_core_type,
};
use crate::ir::id::{CustomSectionID, FunctionID, GlobalID, ModuleID};
use crate::ir::module::module_functions::FuncKind;
use crate::ir::module::module_globals::Global;
use crate::ir::module::Module;
use crate::ir::section::ComponentSection;
use crate::ir::types::CustomSections;
use crate::ir::wrappers::{
    add_to_namemap, convert_component_type, convert_instance_type, convert_module_type_declaration,
    convert_results, do_reencode, process_alias,
};

#[derive(Debug)]
/// Intermediate Representation of a wasm component.
pub struct Component<'a> {
    /// Modules
    pub modules: Vec<Module<'a>>,
    ///Alias
    pub alias: Vec<ComponentAlias<'a>>,
    /// Core Types
    pub core_types: Vec<CoreType<'a>>,
    /// Component Types
    pub component_types: Vec<ComponentType<'a>>,
    /// Imports
    pub imports: Vec<ComponentImport<'a>>,
    /// Exports
    pub exports: Vec<ComponentExport<'a>>,
    /// Core Instances
    pub instances: Vec<Instance<'a>>,
    /// Component Instances
    pub component_instance: Vec<ComponentInstance<'a>>,
    /// Canons
    pub canons: Vec<CanonicalFunction>,
    /// Custom sections
    pub custom_sections: CustomSections<'a>,
    /// Nested Components
    pub components: Vec<Component<'a>>,
    /// Number of modules
    pub num_modules: usize,
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
            alias: vec![],
            core_types: vec![],
            component_types: vec![],
            imports: vec![],
            exports: vec![],
            instances: vec![],
            component_instance: vec![],
            canons: vec![],
            custom_sections: CustomSections::new(vec![]),
            num_modules: 0,
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

    fn add_to_own_section(&mut self, section: ComponentSection) {
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
        self.add_to_own_section(ComponentSection::Module);
        self.num_modules += 1;

        ModuleID(id as u32)
    }

    /// Add a Global to this Component.
    pub fn add_globals(&mut self, global: Global, module_idx: ModuleID) -> GlobalID {
        self.modules[*module_idx as usize].globals.add(global)
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
        Component::parse_comp(wasm, enable_multi_memory, parser, 0, &mut vec![])
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
        let num_modules = modules.len();
        Ok(Component {
            modules,
            alias,
            core_types,
            component_types,
            imports,
            exports,
            instances,
            component_instance,
            canons,
            custom_sections: CustomSections::new(custom_sections),
            num_modules,
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

    /// Encode a `Component` to bytes..
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
        let mut reencode = wasm_encoder::reencode::RoundtripReencoder;
        // NOTE: All of these are 1-indexed and not 0-indexed
        let mut last_processed_module = 0;
        let mut last_processed_core_ty = 0;
        let mut last_processed_comp_ty = 0;
        let mut last_processed_imp = 0;
        let mut last_processed_exp = 0;
        let mut last_processed_comp_inst = 0;
        let mut last_processed_core_inst = 0;
        let mut last_processed_alias = 0;
        let mut last_processed_canon = 0;
        let mut last_processed_custom_section = 0;
        let mut last_processed_component = 0;

        for (num, section) in self.sections.iter() {
            match section {
                ComponentSection::Component => {
                    assert!(
                        *num as usize + last_processed_component as usize <= self.components.len()
                    );
                    for comp_idx in last_processed_component..last_processed_component + num {
                        component.section(&NestedComponentSection(
                            &self.components[comp_idx as usize].encode_comp(),
                        ));
                        last_processed_component += 1;
                    }
                }
                ComponentSection::Module => {
                    assert!(*num as usize + last_processed_module as usize <= self.modules.len());
                    for mod_idx in last_processed_module..last_processed_module + num {
                        component.section(&ModuleSection(
                            &self.modules[mod_idx as usize].encode_internal(false).0,
                        ));
                        last_processed_module += 1;
                    }
                }
                ComponentSection::CoreType => {
                    assert!(
                        *num as usize + last_processed_core_ty as usize <= self.core_types.len()
                    );
                    let mut type_section = wasm_encoder::CoreTypeSection::new();
                    for cty_idx in last_processed_core_ty..last_processed_core_ty + num {
                        match &self.core_types[cty_idx as usize] {
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
                                let enc = type_section.ty();
                                convert_module_type_declaration(module, enc, &mut reencode);
                            }
                        }
                        last_processed_core_ty += 1;
                    }
                    component.section(&type_section);
                }
                ComponentSection::ComponentType => {
                    assert!(
                        *num as usize + last_processed_comp_ty as usize
                            <= self.component_types.len()
                    );
                    let mut component_ty_section = wasm_encoder::ComponentTypeSection::new();
                    for comp_ty_idx in last_processed_comp_ty..last_processed_comp_ty + num {
                        match &self.component_types[comp_ty_idx as usize] {
                            ComponentType::Defined(comp_ty) => {
                                let enc = component_ty_section.defined_type();
                                match comp_ty {
                                    wasmparser::ComponentDefinedType::Primitive(p) => {
                                        enc.primitive(wasm_encoder::PrimitiveValType::from(*p))
                                    }
                                    wasmparser::ComponentDefinedType::Record(records) => {
                                        enc.record(records.iter().map(|record| {
                                            (record.0, reencode.component_val_type(record.1))
                                        }));
                                    }
                                    wasmparser::ComponentDefinedType::Variant(variants) => enc
                                        .variant(variants.iter().map(|variant| {
                                            (
                                                variant.name,
                                                variant
                                                    .ty
                                                    .map(|ty| reencode.component_val_type(ty)),
                                                variant.refines,
                                            )
                                        })),
                                    wasmparser::ComponentDefinedType::List(l) => {
                                        enc.list(reencode.component_val_type(*l))
                                    }
                                    wasmparser::ComponentDefinedType::Tuple(tup) => enc
                                        .tuple(tup.iter().map(|val_type| {
                                            reencode.component_val_type(*val_type)
                                        })),
                                    wasmparser::ComponentDefinedType::Flags(flags) => {
                                        enc.flags(flags.clone().into_vec().into_iter())
                                    }
                                    wasmparser::ComponentDefinedType::Enum(en) => {
                                        enc.enum_type(en.clone().into_vec().into_iter())
                                    }
                                    wasmparser::ComponentDefinedType::Option(opt) => {
                                        enc.option(reencode.component_val_type(*opt))
                                    }
                                    wasmparser::ComponentDefinedType::Result { ok, err } => enc
                                        .result(
                                            ok.map(|val_type| {
                                                reencode.component_val_type(val_type)
                                            }),
                                            err.map(|val_type| {
                                                reencode.component_val_type(val_type)
                                            }),
                                        ),
                                    wasmparser::ComponentDefinedType::Own(u) => enc.own(*u),
                                    wasmparser::ComponentDefinedType::Borrow(u) => enc.borrow(*u),
                                    wasmparser::ComponentDefinedType::Future(opt) => match opt {
                                        Some(u) => {
                                            enc.future(Some(reencode.component_val_type(*u)))
                                        }
                                        None => enc.future(None),
                                    },
                                    wasmparser::ComponentDefinedType::Stream(opt) => match opt {
                                        Some(u) => {
                                            enc.stream(Some(reencode.component_val_type(*u)))
                                        }
                                        None => enc.stream(None),
                                    },
                                    wasmparser::ComponentDefinedType::FixedSizeList(ty, i) => {
                                        enc.fixed_size_list(reencode.component_val_type(*ty), *i)
                                    }
                                }
                            }
                            ComponentType::Func(func_ty) => {
                                let mut enc = component_ty_section.function();
                                enc.params(func_ty.params.iter().map(
                                    |p: &(&str, wasmparser::ComponentValType)| {
                                        (p.0, reencode.component_val_type(p.1))
                                    },
                                ));
                                convert_results(func_ty.result, enc, &mut reencode);
                            }
                            ComponentType::Component(comp) => {
                                let mut new_comp = wasm_encoder::ComponentType::new();
                                for c in comp.iter() {
                                    match c {
                                        ComponentTypeDeclaration::CoreType(core) => match core {
                                            CoreType::Rec(recgroup) => {
                                                let types = recgroup
                                                    .types()
                                                    .map(|ty| {
                                                        reencode
                                                            .sub_type(ty.to_owned())
                                                            .unwrap_or_else(|_| panic!("Could not encode type as subtype: {:?}", ty))
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
                                                let enc = new_comp.core_type();
                                                convert_module_type_declaration(
                                                    module,
                                                    enc,
                                                    &mut reencode,
                                                );
                                            }
                                        },
                                        ComponentTypeDeclaration::Type(typ) => {
                                            let enc = new_comp.ty();
                                            convert_component_type(
                                                &(*typ).clone(),
                                                enc,
                                                &mut reencode,
                                            );
                                        }
                                        ComponentTypeDeclaration::Alias(a) => {
                                            new_comp.alias(process_alias(a, &mut reencode));
                                        }
                                        ComponentTypeDeclaration::Export { name, ty } => {
                                            let ty = do_reencode(
                                                *ty,
                                                RoundtripReencoder::component_type_ref,
                                                &mut reencode,
                                                "component type",
                                            );
                                            new_comp.export(name.0, ty);
                                        }
                                        ComponentTypeDeclaration::Import(imp) => {
                                            let ty = do_reencode(
                                                imp.ty,
                                                RoundtripReencoder::component_type_ref,
                                                &mut reencode,
                                                "component type",
                                            );
                                            new_comp.import(imp.name.0, ty);
                                        }
                                    }
                                }
                                component_ty_section.component(&new_comp);
                            }
                            ComponentType::Instance(inst) => {
                                component_ty_section
                                    .instance(&convert_instance_type(inst, &mut reencode));
                            }
                            ComponentType::Resource { rep, dtor } => {
                                component_ty_section
                                    .resource(reencode.val_type(*rep).unwrap(), *dtor);
                            }
                        }
                        last_processed_comp_ty += 1;
                    }
                    component.section(&component_ty_section);
                }
                ComponentSection::ComponentImport => {
                    assert!(*num as usize + last_processed_imp as usize <= self.imports.len());
                    let mut imports = wasm_encoder::ComponentImportSection::new();
                    for imp_idx in last_processed_imp..last_processed_imp + num {
                        let imp = &self.imports[imp_idx as usize];
                        let ty = do_reencode(
                            imp.ty,
                            RoundtripReencoder::component_type_ref,
                            &mut reencode,
                            "component type",
                        );
                        imports.import(imp.name.0, ty);
                        last_processed_imp += 1;
                    }
                    component.section(&imports);
                }
                ComponentSection::ComponentExport => {
                    assert!(*num as usize + last_processed_exp as usize <= self.exports.len());
                    let mut exports = wasm_encoder::ComponentExportSection::new();
                    for exp_idx in last_processed_exp..last_processed_exp + num {
                        let exp = &self.exports[exp_idx as usize];
                        exports.export(
                            exp.name.0,
                            reencode.component_export_kind(exp.kind),
                            exp.index,
                            exp.ty.map(|ty| {
                                do_reencode(
                                    ty,
                                    RoundtripReencoder::component_type_ref,
                                    &mut reencode,
                                    "component type",
                                )
                            }),
                        );
                        last_processed_exp += 1;
                    }
                    component.section(&exports);
                }
                ComponentSection::ComponentInstance => {
                    assert!(
                        *num as usize + last_processed_comp_inst as usize
                            <= self.component_instance.len()
                    );
                    let mut instances = wasm_encoder::ComponentInstanceSection::new();
                    for instance_idx in last_processed_comp_inst..last_processed_comp_inst + num {
                        let instance = &self.component_instance[instance_idx as usize];
                        match instance {
                            ComponentInstance::Instantiate {
                                component_index,
                                args,
                            } => {
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
                                    (
                                        value.name.0,
                                        reencode.component_export_kind(value.kind),
                                        value.index,
                                    )
                                }));
                            }
                        }
                        last_processed_comp_inst += 1;
                    }
                    component.section(&instances);
                }
                ComponentSection::CoreInstance => {
                    assert!(
                        *num as usize + last_processed_core_inst as usize <= self.instances.len()
                    );
                    let mut instances = wasm_encoder::InstanceSection::new();
                    for instance_idx in last_processed_core_inst..last_processed_core_inst + num {
                        let instance = &self.instances[instance_idx as usize];
                        match instance {
                            Instance::Instantiate { module_index, args } => {
                                instances.instantiate(
                                    *module_index,
                                    args.iter()
                                        .map(|arg| (arg.name, ModuleArg::Instance(arg.index))),
                                );
                            }
                            Instance::FromExports(exports) => {
                                instances.export_items(exports.iter().map(|export| {
                                    (
                                        export.name,
                                        wasm_encoder::ExportKind::from(export.kind),
                                        export.index,
                                    )
                                }));
                            }
                        }
                        last_processed_core_inst += 1;
                    }
                    component.section(&instances);
                }
                ComponentSection::Alias => {
                    assert!(*num as usize + last_processed_alias as usize <= self.alias.len());
                    let mut alias = ComponentAliasSection::new();
                    for a_idx in last_processed_alias..last_processed_alias + num {
                        let a = &self.alias[a_idx as usize];
                        alias.alias(process_alias(a, &mut reencode));
                        last_processed_alias += 1;
                    }
                    component.section(&alias);
                }
                ComponentSection::Canon => {
                    assert!(*num as usize + last_processed_canon as usize <= self.canons.len());
                    let mut canon_sec = wasm_encoder::CanonicalFunctionSection::new();
                    for canon_idx in last_processed_canon..last_processed_canon + num {
                        let canon = &self.canons[canon_idx as usize];
                        match canon {
                            CanonicalFunction::Lift {
                                core_func_index,
                                type_index,
                                options,
                            } => {
                                canon_sec.lift(
                                    *core_func_index,
                                    *type_index,
                                    options.iter().map(|canon| {
                                        do_reencode(
                                            *canon,
                                            RoundtripReencoder::canonical_option,
                                            &mut reencode,
                                            "canonical option",
                                        )
                                    }),
                                );
                            }
                            CanonicalFunction::Lower {
                                func_index,
                                options,
                            } => {
                                canon_sec.lower(
                                    *func_index,
                                    options.iter().map(|canon| {
                                        do_reencode(
                                            *canon,
                                            RoundtripReencoder::canonical_option,
                                            &mut reencode,
                                            "canonical option",
                                        )
                                    }),
                                );
                            }
                            CanonicalFunction::ResourceNew { resource } => {
                                canon_sec.resource_new(*resource);
                            }
                            CanonicalFunction::ResourceDrop { resource } => {
                                canon_sec.resource_drop(*resource);
                            }
                            CanonicalFunction::ResourceRep { resource } => {
                                canon_sec.resource_rep(*resource);
                            }
                            CanonicalFunction::ResourceDropAsync { resource } => {
                                canon_sec.resource_drop_async(*resource);
                            }
                            CanonicalFunction::ThreadAvailableParallelism => {
                                canon_sec.thread_available_parallelism();
                            }
                            CanonicalFunction::BackpressureSet => {
                                canon_sec.backpressure_set();
                            }
                            CanonicalFunction::TaskReturn { result, options } => {
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
                                canon_sec.stream_new(*ty);
                            }
                            CanonicalFunction::StreamRead { ty, options } => {
                                canon_sec.stream_read(
                                    *ty,
                                    options
                                        .into_iter()
                                        .map(|t| {
                                            do_reencode(
                                                *t,
                                                RoundtripReencoder::canonical_option,
                                                &mut reencode,
                                                "canonical option",
                                            )
                                        })
                                        .collect::<Vec<wasm_encoder::CanonicalOption>>(),
                                );
                            }
                            CanonicalFunction::StreamWrite { ty, options } => {
                                canon_sec.stream_write(
                                    *ty,
                                    options
                                        .into_iter()
                                        .map(|t| {
                                            do_reencode(
                                                *t,
                                                RoundtripReencoder::canonical_option,
                                                &mut reencode,
                                                "canonical option",
                                            )
                                        })
                                        .collect::<Vec<wasm_encoder::CanonicalOption>>(),
                                );
                            }
                            CanonicalFunction::StreamCancelRead { ty, async_ } => {
                                canon_sec.stream_cancel_read(*ty, *async_);
                            }
                            CanonicalFunction::StreamCancelWrite { ty, async_ } => {
                                canon_sec.stream_cancel_write(*ty, *async_);
                            }
                            CanonicalFunction::FutureNew { ty } => {
                                canon_sec.future_new(*ty);
                            }
                            CanonicalFunction::FutureRead { ty, options } => {
                                canon_sec.future_read(
                                    *ty,
                                    options
                                        .into_iter()
                                        .map(|t| {
                                            do_reencode(
                                                *t,
                                                RoundtripReencoder::canonical_option,
                                                &mut reencode,
                                                "canonical option",
                                            )
                                        })
                                        .collect::<Vec<wasm_encoder::CanonicalOption>>(),
                                );
                            }
                            CanonicalFunction::FutureWrite { ty, options } => {
                                canon_sec.future_write(
                                    *ty,
                                    options
                                        .into_iter()
                                        .map(|t| {
                                            do_reencode(
                                                *t,
                                                RoundtripReencoder::canonical_option,
                                                &mut reencode,
                                                "canonical option",
                                            )
                                        })
                                        .collect::<Vec<wasm_encoder::CanonicalOption>>(),
                                );
                            }
                            CanonicalFunction::FutureCancelRead { ty, async_ } => {
                                canon_sec.future_cancel_read(*ty, *async_);
                            }
                            CanonicalFunction::FutureCancelWrite { ty, async_ } => {
                                canon_sec.future_cancel_write(*ty, *async_);
                            }
                            CanonicalFunction::ErrorContextNew { options } => {
                                canon_sec.error_context_new(
                                    options
                                        .into_iter()
                                        .map(|t| {
                                            do_reencode(
                                                *t,
                                                RoundtripReencoder::canonical_option,
                                                &mut reencode,
                                                "canonical option",
                                            )
                                        })
                                        .collect::<Vec<wasm_encoder::CanonicalOption>>(),
                                );
                            }
                            CanonicalFunction::ErrorContextDebugMessage { options } => {
                                canon_sec.error_context_debug_message(
                                    options
                                        .into_iter()
                                        .map(|t| {
                                            do_reencode(
                                                *t,
                                                RoundtripReencoder::canonical_option,
                                                &mut reencode,
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
                                canon_sec.thread_spawn_ref(*func_ty_index);
                            }
                            CanonicalFunction::ThreadSpawnIndirect {
                                func_ty_index,
                                table_index,
                            } => {
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
                                canon_sec.stream_drop_readable(*ty);
                            }
                            CanonicalFunction::StreamDropWritable { ty } => {
                                canon_sec.stream_drop_writable(*ty);
                            }
                            CanonicalFunction::FutureDropReadable { ty } => {
                                canon_sec.future_drop_readable(*ty);
                            }
                            CanonicalFunction::FutureDropWritable { ty } => {
                                canon_sec.future_drop_writable(*ty);
                            }
                        }
                        last_processed_canon += 1;
                    }
                    component.section(&canon_sec);
                }
                ComponentSection::ComponentStartSection => {
                    // Should only be 1 start section
                    assert_eq!(self.start_section.len(), 1);
                    let start_fn = &self.start_section[0];
                    let start_sec = wasm_encoder::ComponentStartSection {
                        function_index: start_fn.func_index,
                        args: start_fn.arguments.iter(),
                        results: start_fn.results,
                    };
                    component.section(&start_sec);
                }
                ComponentSection::CustomSection => {
                    assert!(
                        *num as usize + last_processed_custom_section as usize
                            <= self.custom_sections.len()
                    );
                    for custom_sec_idx in
                        last_processed_custom_section..last_processed_custom_section + num
                    {
                        let section = &self
                            .custom_sections
                            .get_by_id(CustomSectionID(custom_sec_idx));
                        component.section(&wasm_encoder::CustomSection {
                            name: std::borrow::Cow::Borrowed(section.name),
                            data: section.data.clone(),
                        });
                        last_processed_custom_section += 1;
                    }
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

    /// Print a rudimentary textual representation of a `Component`
    pub fn print(&self) {
        // Print Alias
        if !self.alias.is_empty() {
            eprintln!("Alias Section:");
            for alias in self.alias.iter() {
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
        if !self.component_types.is_empty() {
            eprintln!("Component Type Section:");
            for cty in self.component_types.iter() {
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
