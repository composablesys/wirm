//! Intermediate Representation of a wasm module.

use super::types::{CustomSection, DataType, InitExpr, Tag, TagUtils};
use crate::error::Error;
use crate::error::Error::{InstrumentationError, IO};
use crate::ir::id::{
    CustomSectionID, DataSegmentID, FunctionID, GlobalID, ImportsID, MemoryID, TypeID,
};
use crate::ir::module::module_exports::{Export, ModuleExports};
use crate::ir::module::module_functions::{
    FuncKind, Function, Functions, ImportedFunction, LocalFunction,
};
use crate::ir::module::module_globals::{
    Global, GlobalKind, ImportedGlobal, LocalGlobal, ModuleGlobals,
};
use crate::ir::module::module_imports::{expand_imports, Import, ModuleImports};
use crate::ir::module::module_memories::{ImportedMemory, LocalMemory, MemKind, Memories, Memory};
use crate::ir::module::module_tables::{Element, ModuleTables, Table};
use crate::ir::module::module_types::{ModuleTypes, RecGroup, Types};
use crate::ir::module::names::{
    indirect_namemap_parser2encoder, indirect_namemap_parser2vec, namemap_parser2encoder,
    namemap_parser2vec, remap_names,
};
use crate::ir::module::reindex::fix_op_id_mapping;
use crate::ir::module::side_effects::{InjectType, Injection};
use crate::ir::types;
use crate::ir::types::{
    Body, CustomSections, DataSegment, DataSegmentKind, ElementItems, ElementKind, Instructions,
    InstrumentationFlag,
};
use log::warn;
use std::borrow::Cow;
use std::collections::HashMap;
use wasm_encoder::reencode::{Reencode, RoundtripReencoder};
use wasm_encoder::TagSection;
use wasmparser::{
    CompositeInnerType, ExternalKind, GlobalType, MemoryType, Operator, PackedIndex, Parser,
    Payload, TagType, TypeRef,
};

pub mod module_exports;
pub mod module_functions;
pub mod module_globals;
pub mod module_imports;
pub mod module_memories;
pub mod module_tables;
pub mod module_types;
pub(crate) mod names;
pub(crate) mod reindex;
pub mod side_effects;
mod special_instr;
#[cfg(test)]
mod test;

#[derive(Clone, Debug, Default)]
/// Intermediate Representation of a wasm module. See the [WASM Spec] for different sections.
///
/// [WASM Spec]: https://webassembly.github.io/spec/core/binary/modules.html
pub struct Module<'a> {
    /// name of module
    pub module_name: Option<String>,
    /// Types
    pub types: ModuleTypes,
    /// Imports
    pub imports: ModuleImports<'a>,
    /// Mapping from function index to type index.
    /// Note that `|functions| == num_functions + num_imported_functions`
    pub functions: Functions<'a>,
    /// Each table has a type and optional initialization expression.
    pub tables: ModuleTables<'a>,
    /// Memories
    pub memories: Memories,
    /// Globals
    pub globals: ModuleGlobals,
    /// Data Sections
    pub data: Vec<DataSegment>,
    data_count_section_exists: bool,
    /// Exports
    pub exports: ModuleExports,
    /// Index of the start function.
    pub start: Option<FunctionID>,
    /// Elements
    pub elements: Vec<Element>,
    /// Tags
    pub tags: Vec<TagType>,
    /// Custom Sections
    pub custom_sections: CustomSections<'a>,
    /// Number of local functions (not counting imported functions)
    pub(crate) num_local_functions: u32,
    /// Number of local globals (not counting imported globals)
    pub(crate) num_local_globals: u32,
    /// Number of local tables (not counting imported tables)
    #[allow(dead_code)]
    pub(crate) num_local_tables: u32,
    /// Number of local memories (not counting imported memories)
    #[allow(dead_code)]
    pub(crate) num_local_memories: u32,

    // just a placeholder for round-trip
    pub(crate) label_names: Vec<(u32, wasm_encoder::NameMap)>,
    pub(crate) type_names: wasm_encoder::NameMap,
    pub(crate) table_names: wasm_encoder::NameMap,
    pub(crate) memory_names: Vec<(u32, String)>,
    pub(crate) global_names: Vec<(u32, String)>,
    pub(crate) elem_names: wasm_encoder::NameMap,
    pub(crate) data_names: wasm_encoder::NameMap,
    pub(crate) field_names: wasm_encoder::IndirectNameMap,
    pub(crate) tag_names: wasm_encoder::NameMap,
    pub(crate) parameter_names: wasm_encoder::IndirectNameMap,
    pub(crate) tag_parameter_names: wasm_encoder::IndirectNameMap,
}

enum FlagsIter<'a> {
    Flags(std::vec::IntoIter<InstrumentationFlag<'a>>),
    Default,
}

impl<'a> Iterator for FlagsIter<'a> {
    type Item = InstrumentationFlag<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            FlagsIter::Flags(flags) => flags.next(),
            FlagsIter::Default => Some(InstrumentationFlag::default()),
        }
    }
}

impl<'a> Module<'a> {
    /// Parses a `Module` from a wasm binary.
    ///
    /// Set enable_multi_memory to `true` to support parsing modules using multiple memories.
    /// Set with_offsets to `true` to save opcode pc offset metadata during parsing
    /// (can be used to determine the static pc offset inside a function body of the start of any opcode).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use wirm::Module;
    ///
    /// let file = "path_to_file";
    /// let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    /// let module = Module::parse(&buff, false, false).unwrap();
    /// ```
    pub fn parse(
        wasm: &'a [u8],
        enable_multi_memory: bool,
        with_offsets: bool,
    ) -> Result<Self, Error> {
        let parser = Parser::new(0);
        Module::parse_internal(wasm, enable_multi_memory, with_offsets, parser)
    }

    fn parse_body(
        body: wasmparser::FunctionBody,
        enable_multi_memory: bool,
        with_offsets: bool,
    ) -> Result<Body, Error> {
        let locals_orig = body
            .get_locals_reader()?
            .get_binary_reader()
            .original_position();
        let locals = body
            .get_locals_reader()?
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        let mut num_locals = 0;
        let locals: Vec<(u32, DataType)> = locals
            .iter()
            .map(|(count, val_type)| {
                num_locals += count;
                (*count, DataType::from(*val_type))
            })
            .collect();

        let op_reader = body.get_operators_reader()?;
        let ops = op_reader
            .into_iter_with_offsets()
            .collect::<Result<Vec<(Operator, u64)>, _>>()?;

        let instructions = Instructions::new(ops, locals_orig, with_offsets);
        if let Some(last) = instructions.get_ops().last() {
            if let Operator::End = last {
            } else {
                return Err(Error::MissingFunctionEnd {
                    func_range: body.range(),
                });
            }
        }
        if !enable_multi_memory
            && instructions.get_ops().iter().any(|i| match i {
                Operator::MemoryGrow { mem, .. } | Operator::MemorySize { mem, .. } => *mem != 0x00,
                _ => false,
            })
        {
            return Err(Error::InvalidMemoryReservedByte {
                func_range: body.range(),
            });
        }

        Ok(Body {
            locals,
            num_locals,
            num_instructions: instructions.len(),
            instructions,
            name: None,
            local_names: None,
        })
    }

    pub(crate) fn parse_internal(
        wasm: &'a [u8],
        enable_multi_memory: bool,
        with_offsets: bool,
        parser: Parser,
    ) -> Result<Self, Error> {
        #[cfg(feature = "parallel")]
        use rayon::prelude::*;

        let mut imports: ModuleImports = ModuleImports::default();
        let mut types: HashMap<TypeID, Types> = HashMap::new();
        let mut recgroups = vec![];
        let mut data = vec![];
        let mut tables = vec![];
        let mut memories = vec![];
        let mut functions = vec![];
        let mut elements = vec![];
        let mut code_section_count = 0;
        let mut globals = vec![];
        let mut exports = vec![];
        let mut start = None;
        let mut data_section_count = None;
        let mut custom_sections = vec![];
        let mut tags: Vec<TagType> = vec![];

        let mut module_name: Option<String> = None;
        // for the other names, we directly encode it without passing them into the IR
        let mut label_names = vec![];
        let mut type_names = wasm_encoder::NameMap::new();
        let mut table_names = wasm_encoder::NameMap::new();
        let mut memory_names = vec![];
        let mut global_names = vec![];
        let mut elem_names = wasm_encoder::NameMap::new();
        let mut data_names = wasm_encoder::NameMap::new();
        let mut field_names = wasm_encoder::IndirectNameMap::new();
        let mut tag_names = wasm_encoder::NameMap::new();
        let mut parameter_names = wasm_encoder::IndirectNameMap::new();
        let mut tag_parameter_names = wasm_encoder::IndirectNameMap::new();
        let mut bodies_and_names = vec![];
        // k-th function import -> its position in the import list. Used to
        // resolve function names; built lazily, reset when the imports change.
        let mut func_import_pos: Option<Vec<u32>> = None;

        for payload in parser.parse_all(wasm) {
            let payload = payload?;
            match payload {
                Payload::ImportSection(import_section_reader) => {
                    let import_vec = expand_imports(import_section_reader)?;
                    imports = ModuleImports::new(import_vec);
                    func_import_pos = None;
                }
                Payload::TypeSection(type_section_reader) => {
                    for ty in type_section_reader.into_iter() {
                        let mut group_types = vec![];
                        let explicit_rec_group = ty.clone()?.is_explicit_rec_group();
                        for subtype in ty?.types() {
                            match subtype.composite_type.inner.clone() {
                                CompositeInnerType::Func(fty) => {
                                    let params = fty
                                        .params()
                                        .iter()
                                        .map(|x| DataType::from(*x))
                                        .collect::<Vec<_>>()
                                        .into_boxed_slice();
                                    let results = fty
                                        .results()
                                        .iter()
                                        .map(|x| DataType::from(*x))
                                        .collect::<Vec<_>>()
                                        .into_boxed_slice();

                                    group_types.push(Types::FuncType {
                                        params,
                                        results,
                                        super_types: subtype.supertype_idxs.clone(),
                                        is_final: subtype.is_final,
                                        shared: subtype.composite_type.shared,
                                        descriptor: unpack(subtype.composite_type.descriptor_idx),
                                        describes: unpack(subtype.composite_type.describes_idx),
                                        tag: None,
                                    });
                                }
                                CompositeInnerType::Array(aty) => {
                                    group_types.push(Types::ArrayType {
                                        mutable: aty.0.mutable,
                                        fields: DataType::from(aty.0.element_type),
                                        super_types: subtype.supertype_idxs.clone(),
                                        is_final: subtype.is_final,
                                        shared: subtype.composite_type.shared,
                                        descriptor: unpack(subtype.composite_type.descriptor_idx),
                                        describes: unpack(subtype.composite_type.describes_idx),
                                        tag: None,
                                    });
                                }
                                CompositeInnerType::Struct(sty) => {
                                    group_types.push(Types::StructType {
                                        mutable: sty
                                            .fields
                                            .iter()
                                            .map(|field| field.mutable)
                                            .collect::<Vec<_>>(),
                                        fields: sty
                                            .fields
                                            .iter()
                                            .map(|field| DataType::from(field.element_type))
                                            .collect::<Vec<_>>(),
                                        super_types: subtype.supertype_idxs.clone(),
                                        is_final: subtype.is_final,
                                        shared: subtype.composite_type.shared,
                                        descriptor: unpack(subtype.composite_type.descriptor_idx),
                                        describes: unpack(subtype.composite_type.describes_idx),
                                        tag: None,
                                    });
                                }
                                CompositeInnerType::Cont(cty) => {
                                    group_types.push(Types::ContType {
                                        packed_index: cty.0,
                                        super_types: subtype.supertype_idxs.clone(),
                                        is_final: subtype.is_final,
                                        shared: subtype.composite_type.shared,
                                        descriptor: unpack(subtype.composite_type.descriptor_idx),
                                        describes: unpack(subtype.composite_type.describes_idx),
                                        tag: None,
                                    });
                                }
                            }
                        }
                        fn unpack(packed: Option<PackedIndex>) -> Option<u32> {
                            if let Some(i) = packed {
                                let idx = i.unpack().as_module_index();
                                if idx.is_none() {
                                    panic!("Internal error: I just made an assumption on how to unpack this. If I'm wrong, create a GH issue.")
                                }
                                idx
                            } else {
                                None
                            }
                        }
                        let mut ids = vec![];
                        for ty in group_types.iter() {
                            let ty_id = TypeID(types.len() as u32);
                            ids.push(ty_id);
                            types.insert(ty_id, ty.to_owned());
                        }
                        recgroups.push(RecGroup::new(ids, explicit_rec_group));
                    }
                }
                Payload::DataSection(data_section_reader) => {
                    data = data_section_reader
                        .into_iter()
                        .map(|sec| {
                            sec.map_err(Error::from)
                                .and_then(DataSegment::from_wasmparser)
                        })
                        .collect::<Result<_, _>>()?;
                }
                Payload::TableSection(table_section_reader) => {
                    tables = table_section_reader
                        .into_iter()
                        .map(|t| {
                            t.map_err(Error::from).map(|t| match t.init {
                                wasmparser::TableInit::RefNull => Table::new(t.ty, None, None),
                                wasmparser::TableInit::Expr(e) => Table::new(t.ty, Some(e), None),
                            })
                        })
                        .collect::<Result<_, _>>()?;
                }
                Payload::MemorySection(memory_section_reader) => {
                    memories = memory_section_reader
                        .into_iter()
                        .collect::<Result<_, _>>()?;
                }
                Payload::FunctionSection(function_section_reader) => {
                    let temp: Vec<u32> = function_section_reader
                        .into_iter()
                        .collect::<Result<_, _>>()?;
                    functions.extend(temp.iter().map(|id| TypeID(*id)));
                }
                Payload::GlobalSection(global_section_reader) => {
                    globals = global_section_reader
                        .into_iter()
                        .map(|g| Global::from_wasmparser(g?))
                        .collect::<Result<_, _>>()?;
                }
                Payload::ExportSection(export_section_reader) => {
                    for exp in export_section_reader.into_iter() {
                        exports.push(Export::from(exp?));
                    }
                }
                Payload::StartSection { func, range: _ } => {
                    if start.is_some() {
                        return Err(Error::MultipleStartSections);
                    }
                    start = Some(FunctionID(func));
                }
                Payload::ElementSection(element_section_reader) => {
                    for element in element_section_reader.into_iter() {
                        let element = element?;
                        let items = ElementItems::from_wasmparser(element.items.clone())?;
                        elements.push(Element::new(
                            ElementKind::from_wasmparser(element.kind)?,
                            items,
                            None,
                        ));
                    }
                }
                Payload::DataCountSection { count, range: _ } => {
                    data_section_count = Some(count);
                }
                Payload::CodeSectionStart {
                    count,
                    unchecked_range: _,
                    size: _,
                } => {
                    code_section_count = count as usize;
                }
                Payload::CodeSectionEntry(body) => {
                    bodies_and_names.push((body, None, None));
                }
                Payload::TagSection(tag_section_reader) => {
                    for tag in tag_section_reader.into_iter() {
                        tags.push(tag?);
                    }
                }
                Payload::CustomSection(custom_section_reader) => {
                    match custom_section_reader.as_known() {
                        wasmparser::KnownCustom::Name(name_section_reader) => {
                            for subsection in name_section_reader {
                                #[allow(clippy::single_match)]
                                match subsection? {
                                    wasmparser::Name::Function(names) => {
                                        // Built at most once per import list
                                        let func_import_pos =
                                            func_import_pos.get_or_insert_with(|| {
                                                (0_u32..)
                                                    .zip(imports.iter())
                                                    .filter(|(_, import)| import.is_function())
                                                    .map(|(pos, _)| pos)
                                                    .collect()
                                            });
                                        for name in names {
                                            let naming = name?;
                                            let abs_idx = naming.index;
                                            if let Some(&pos) =
                                                func_import_pos.get(abs_idx as usize)
                                            {
                                                imports.set_name(
                                                    naming.name.to_string(),
                                                    ImportsID(pos),
                                                );
                                            } else {
                                                // Skip over invalid name section entries in the input module
                                                let rel_idx =
                                                    (abs_idx - imports.num_funcs) as usize;
                                                if let Some(entry) =
                                                    bodies_and_names.get_mut(rel_idx)
                                                {
                                                    entry.1 = Some(naming.name.to_string());
                                                }
                                            }
                                        }
                                    }
                                    wasmparser::Name::Module { name, .. } => {
                                        module_name = Some(name.to_string());
                                    }
                                    wasmparser::Name::Local(names) => {
                                        // Attach each function's local names to its body,
                                        // keyed by function index, so they follow the
                                        // function through any later reindexing.
                                        for indirect in names {
                                            let indirect = indirect?;
                                            let abs_idx = indirect.index;
                                            if abs_idx < imports.num_funcs {
                                                // imported functions have no locals
                                                continue;
                                            }
                                            let rel_idx = (abs_idx - imports.num_funcs) as usize;
                                            // defend against invalid name section entries for locals
                                            if let Some(entry) = bodies_and_names.get_mut(rel_idx) {
                                                entry.2 =
                                                    Some(namemap_parser2encoder(indirect.names));
                                            }
                                        }
                                    }
                                    wasmparser::Name::Label(names) => {
                                        label_names = indirect_namemap_parser2vec(names);
                                    }
                                    wasmparser::Name::Type(names) => {
                                        type_names = namemap_parser2encoder(names);
                                    }
                                    wasmparser::Name::Table(names) => {
                                        table_names = namemap_parser2encoder(names);
                                    }
                                    wasmparser::Name::Memory(names) => {
                                        memory_names = namemap_parser2vec(names);
                                    }
                                    wasmparser::Name::Global(names) => {
                                        global_names = namemap_parser2vec(names);
                                    }
                                    wasmparser::Name::Element(names) => {
                                        elem_names = namemap_parser2encoder(names);
                                    }
                                    wasmparser::Name::Data(names) => {
                                        data_names = namemap_parser2encoder(names);
                                    }
                                    wasmparser::Name::Field(names) => {
                                        field_names = indirect_namemap_parser2encoder(names);
                                    }
                                    wasmparser::Name::Tag(names) => {
                                        tag_names = namemap_parser2encoder(names);
                                    }
                                    wasmparser::Name::Parameter(names) => {
                                        parameter_names = indirect_namemap_parser2encoder(names);
                                    }
                                    wasmparser::Name::TagParameter(names) => {
                                        tag_parameter_names =
                                            indirect_namemap_parser2encoder(names);
                                    }
                                    wasmparser::Name::Unknown { .. } => {}
                                }
                            }
                        }
                        _ => {
                            custom_sections
                                .push((custom_section_reader.name(), custom_section_reader.data()));
                        }
                    }
                }
                Payload::Version {
                    num,
                    encoding: _,
                    range: _,
                } => {
                    if num != 1 {
                        return Err(Error::UnknownVersion(num as u32));
                    }
                }
                Payload::UnknownSection {
                    id,
                    contents: _,
                    range: _,
                } => return Err(Error::UnknownSection { section_id: id }),
                Payload::ModuleSection {
                    parser: _,
                    unchecked_range: _,
                }
                | Payload::InstanceSection(_)
                | Payload::CoreTypeSection(_)
                | Payload::ComponentSection {
                    parser: _,
                    unchecked_range: _,
                }
                | Payload::ComponentInstanceSection(_)
                | Payload::ComponentAliasSection(_)
                | Payload::ComponentTypeSection(_)
                | Payload::ComponentCanonicalSection(_)
                | Payload::ComponentStartSection { start: _, range: _ }
                | Payload::ComponentImportSection(_)
                | Payload::ComponentExportSection(_)
                | Payload::End(_) => {}
                other => return Err(Error::UnhandledPayload(format!("{:?}", other))),
            }
        }

        #[cfg(feature = "parallel")]
        let code_sections = bodies_and_names
            .into_par_iter()
            .map(|(body, name, local_names)| {
                let mut body = Self::parse_body(body, enable_multi_memory, with_offsets)?;
                if let Some(name) = name {
                    body.name = Some(name);
                }
                body.local_names = local_names;
                Ok::<_, Error>(body)
            })
            .collect::<Result<Vec<_>, _>>()?;

        #[cfg(not(feature = "parallel"))]
        let code_sections = bodies_and_names
            .into_iter()
            .map(|(body, name, local_names)| {
                let mut body = Self::parse_body(body, enable_multi_memory, with_offsets)?;
                if let Some(name) = name {
                    body.name = Some(name);
                }
                body.local_names = local_names;
                Ok::<_, Error>(body)
            })
            .collect::<Result<Vec<_>, _>>()?;

        if code_section_count != code_sections.len() || code_section_count != functions.len() {
            return Err(Error::IncorrectCodeCounts {
                function_section_count: functions.len(),
                code_section_declared_count: code_section_count,
                code_section_actual_count: code_sections.len(),
            });
        }
        if let Some(data_count) = data_section_count {
            if data_count as usize != data.len() {
                return Err(Error::IncorrectDataCount {
                    declared_count: data_count as usize,
                    actual_count: data.len(),
                });
            }
        }

        // Add all the functions. First add all the imported functions as they have the first IDs
        let mut final_funcs = vec![];
        let mut imp_fn_id = 0;
        for (index, imp) in imports.iter().enumerate() {
            if let TypeRef::Func(u) | TypeRef::FuncExact(u) = imp.ty {
                final_funcs.push(Function::new(
                    FuncKind::Import(ImportedFunction::new(
                        ImportsID(index as u32),
                        TypeID(u),
                        FunctionID(imp_fn_id),
                    )),
                    Some(imp.name.parse().unwrap()),
                ));
                imp_fn_id += 1;
            }
        }
        // Local Functions
        let code_sections_length = code_sections.len();
        for (index, code_sec) in code_sections.into_iter().enumerate() {
            let name = code_sec.name.clone();
            let ty = types.get(&functions[index]).ok_or_else(|| {
                Error::UnknownId(format!(
                    "Could not find type with ID: {}",
                    *functions[index]
                ))
            })?;
            final_funcs.push(Function::new(
                FuncKind::Local(Box::new(LocalFunction::new(
                    functions[index],
                    FunctionID(imports.num_funcs + index as u32),
                    code_sec,
                    ty.params()?.len(),
                    None,
                ))),
                name,
            ));
        }

        // Process the imported memories
        let mut final_mems = vec![];
        let mut imp_mem_id = 0;
        for (index, imp) in imports.iter().enumerate() {
            if let TypeRef::Memory(ty) = imp.ty {
                final_mems.push(Memory::new(
                    ty,
                    MemKind::Import(ImportedMemory {
                        import_id: ImportsID(index as u32),
                        import_mem_id: MemoryID(imp_mem_id),
                    }),
                    None,
                ));
                imp_mem_id += 1;
            }
        }
        // Process the Local memories
        for (index, ty) in memories.iter().enumerate() {
            final_mems.push(Memory::new(
                ty.to_owned(),
                MemKind::Local(LocalMemory {
                    mem_id: MemoryID(imports.num_memories + index as u32),
                }),
                None,
            ));
        }

        let num_globals = globals.len() as u32;
        let num_memories = memories.len() as u32;
        let num_tables = tables.len() as u32;
        let module_globals = ModuleGlobals::new(&imports, globals);
        Ok(Module {
            types: ModuleTypes::new(recgroups, types),
            imports,
            functions: Functions::new(final_funcs),
            tables: ModuleTables::new(tables),
            memories: Memories::new(final_mems),
            globals: module_globals,
            exports: ModuleExports::new(exports),
            start,
            elements,
            data_count_section_exists: data_section_count.is_some(),
            // code_sections: code_sections.clone(),
            data,
            tags,
            custom_sections: CustomSections::new(custom_sections),
            num_local_functions: code_sections_length as u32,
            num_local_globals: num_globals,
            num_local_tables: num_tables,
            num_local_memories: num_memories,
            module_name,
            type_names,
            table_names,
            elem_names,
            memory_names,
            global_names,
            data_names,
            field_names,
            tag_names,
            label_names,
            parameter_names,
            tag_parameter_names,
        })
    }

    /// Creates Vec of (Function, Number of Instructions)
    pub fn get_func_metadata(&self) -> Vec<(FunctionID, usize)> {
        let mut metadata = vec![];
        for func in self.functions.iter() {
            match &func.kind {
                FuncKind::Import(_) => {}
                FuncKind::Local(func, ..) => {
                    metadata.push((func.func_id, func.body.num_instructions));
                }
            }
        }
        metadata
    }

    /// Emit the module into a wasm binary file.
    pub fn emit_wasm(&mut self, file_name: &str) -> types::Result<()> {
        let (module, _) = self.encode_internal(false)?;
        let wasm = module.finish();
        std::fs::write(file_name, wasm).map_err(IO)?;
        Ok(())
    }

    /// Encode the module into a wasm binary.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use wirm::Module;
    ///
    /// let file = "path_to_file";
    /// let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    /// let mut module = Module::parse(&buff, false, false).unwrap();
    /// let result = module.encode();
    /// ```
    pub fn encode(&self) -> types::Result<Vec<u8>> {
        Ok(self.encode_internal(false)?.0.finish())
    }

    /// Reorganizes items (both local and imports) in the correct ordering after any potential modifications
    pub(crate) fn reorganize<T: LocalOrImport>(items: &mut Vec<T>) {
        let mut result = vec![];
        let mut new_local = vec![];
        for item in items.drain(..) {
            if item.is_deleted() {
                continue;
            }
            if item.is_import() {
                result.push(item);
            } else {
                new_local.push(item);
            }
        }
        result.extend(new_local);
        *items = result;
    }

    /// Get the mapping of old ID -> new ID in module
    pub(crate) fn get_mapping_generic<T: GetID>(
        slice: std::slice::Iter<'_, T>,
    ) -> HashMap<u32, u32> {
        let mut mapping = HashMap::new();
        for (new_id, item) in slice.enumerate() {
            let old_id = item.get_id();
            mapping.insert(old_id, new_id as u32);
        }
        mapping
    }

    pub(crate) fn recalculate_ids<T: LocalOrImport + GetID, U: AsVec<T>>(
        items: &mut U,
    ) -> HashMap<u32, u32> {
        Self::reorganize(items.as_vec_mut());
        let id_mapping = Self::get_mapping_generic(items.as_vec().iter());
        debug_assert_eq!(items.as_vec().len(), id_mapping.len());
        id_mapping
    }

    fn encode_type(&self, ty: &Types) -> wasm_encoder::SubType {
        match ty {
            Types::FuncType {
                params,
                results,
                super_types,
                is_final,
                shared,
                descriptor,
                describes,
                ..
            } => {
                let params = params
                    .iter()
                    .map(wasm_encoder::ValType::from)
                    .collect::<Vec<_>>();
                let results = results
                    .iter()
                    .map(wasm_encoder::ValType::from)
                    .collect::<Vec<_>>();
                let fty = wasm_encoder::FuncType::new(params, results);
                wasm_encoder::SubType {
                    is_final: *is_final,
                    supertype_idxs: super_types
                        .iter()
                        .filter_map(|idx| idx.as_module_index())
                        .collect(),
                    composite_type: wasm_encoder::CompositeType {
                        inner: wasm_encoder::CompositeInnerType::Func(fty),
                        shared: *shared,
                        descriptor: *descriptor,
                        describes: *describes,
                    },
                }
            }
            Types::ArrayType {
                fields,
                mutable,
                super_types,
                is_final,
                shared,
                descriptor,
                describes,
                ..
            } => wasm_encoder::SubType {
                is_final: *is_final,
                supertype_idxs: super_types
                    .iter()
                    .filter_map(|idx| idx.as_module_index())
                    .collect(),
                composite_type: wasm_encoder::CompositeType {
                    inner: wasm_encoder::CompositeInnerType::Array(wasm_encoder::ArrayType(
                        wasm_encoder::FieldType {
                            element_type: wasm_encoder::StorageType::from(*fields),
                            mutable: *mutable,
                        },
                    )),
                    shared: *shared,
                    descriptor: *descriptor,
                    describes: *describes,
                },
            },
            Types::StructType {
                fields,
                mutable,
                super_types,
                is_final,
                shared,
                descriptor,
                describes,
                ..
            } => {
                let mut encoded_fields: Vec<wasm_encoder::FieldType> = vec![];
                for (idx, sty) in fields.iter().enumerate() {
                    encoded_fields.push(wasm_encoder::FieldType {
                        element_type: wasm_encoder::StorageType::from(*sty),
                        mutable: mutable[idx],
                    });
                }
                wasm_encoder::SubType {
                    is_final: *is_final,
                    supertype_idxs: super_types
                        .iter()
                        .filter_map(|idx| idx.as_module_index())
                        .collect(),
                    composite_type: wasm_encoder::CompositeType {
                        inner: wasm_encoder::CompositeInnerType::Struct(wasm_encoder::StructType {
                            fields: Box::from(encoded_fields),
                        }),
                        shared: *shared,
                        descriptor: *descriptor,
                        describes: *describes,
                    },
                }
            }
            Types::ContType { .. } => {
                todo!()
            }
        }
    }

    fn encode_function(
        func: &mut LocalFunction,
        func_mapping: &HashMap<u32, u32>,
        global_mapping: &HashMap<u32, u32>,
        memory_mapping: &HashMap<u32, u32>,
    ) -> types::Result<wasm_encoder::Function> {
        let mut reencode = RoundtripReencoder;
        let Body {
            instructions,
            locals,
            ..
        } = &mut func.body;
        let mut converted_locals = Vec::with_capacity(locals.len());
        for (c, ty) in locals {
            converted_locals.push((*c, wasm_encoder::ValType::from(&*ty)));
        }
        let mut function = wasm_encoder::Function::new(converted_locals);
        let instr_len = instructions.len() - 1;
        let (ops, mut flags) = instructions.get_ops_flags_mut();
        for (idx, op) in ops.iter_mut().enumerate() {
            fix_op_id_mapping(op, func_mapping, global_mapping, memory_mapping)?;
            if flags.is_none() {
                encode(&op.clone(), &mut function, &mut reencode);
                continue;
            }
            let instrument = &mut flags.as_mut().unwrap()[idx];
            if !instrument.has_instr() {
                encode(&op.clone(), &mut function, &mut reencode);
                continue;
            } else {
                instrument.check_special_is_resolved()?;

                // this instruction has instrumentation, handle it!
                let InstrumentationFlag {
                    current_mode: _current_mode,
                    before,
                    after,
                    alternate,
                    ..
                } = instrument;

                // If we're at the `end` of the function, drop this instrumentation
                let at_end = idx >= instr_len;

                // First encode before instructions
                update_ids_and_encode(
                    &mut before.instrs,
                    func_mapping,
                    global_mapping,
                    memory_mapping,
                    &mut function,
                    &mut reencode,
                )?;

                // If there are any alternate, encode the alternate
                if !at_end && !alternate.is_none() {
                    if let Some(alt) = alternate {
                        update_ids_and_encode(
                            &mut alt.instrs,
                            func_mapping,
                            global_mapping,
                            memory_mapping,
                            &mut function,
                            &mut reencode,
                        )?;
                    }
                } else {
                    encode(&op.clone(), &mut function, &mut reencode);
                }

                // Now encode the after instructions
                if !at_end {
                    update_ids_and_encode(
                        &mut after.instrs,
                        func_mapping,
                        global_mapping,
                        memory_mapping,
                        &mut function,
                        &mut reencode,
                    )?;
                }
            }

            fn update_ids_and_encode(
                instrs: &mut Vec<Operator>,
                func_mapping: &HashMap<u32, u32>,
                global_mapping: &HashMap<u32, u32>,
                memory_mapping: &HashMap<u32, u32>,
                function: &mut wasm_encoder::Function,
                reencode: &mut RoundtripReencoder,
            ) -> types::Result<()> {
                for instr in instrs {
                    fix_op_id_mapping(instr, func_mapping, global_mapping, memory_mapping)?;
                    encode(instr, function, reencode);
                }
                Ok(())
            }
            fn encode(
                instr: &Operator,
                function: &mut wasm_encoder::Function,
                reencode: &mut RoundtripReencoder,
            ) {
                function.instruction(
                    &reencode
                        .instruction(instr.clone())
                        .expect("Unable to convert Instruction"),
                );
            }
        }

        Ok(function)
    }

    /// Encodes an Wirm Module to a wasm_encoder Module.
    /// This requires a mutable reference to self due to the special instrumentation resolution step.
    pub(crate) fn encode_internal(
        &self,
        pull_side_effects: bool,
    ) -> types::Result<(
        wasm_encoder::Module,
        HashMap<InjectType, Vec<Injection<'a>>>,
    )> {
        #[cfg(feature = "parallel")]
        use rayon::prelude::*;

        let mut tmp = self.clone();

        // First fix the ID mappings throughout the module
        let func_mapping = if tmp.functions.recalculate_ids {
            Self::recalculate_ids(&mut tmp.functions)
        } else {
            Self::get_mapping_generic(tmp.functions.as_vec().iter())
        };
        let global_mapping = if tmp.globals.recalculate_ids {
            Self::recalculate_ids(&mut tmp.globals)
        } else {
            Self::get_mapping_generic(tmp.globals.iter())
        };
        let memory_mapping = if tmp.memories.recalculate_ids {
            Self::recalculate_ids(&mut tmp.memories)
        } else {
            Self::get_mapping_generic(tmp.memories.iter())
        };

        // Collect side effects second to make sure you get the right IDs in the injections
        let mut side_effects = HashMap::new();

        // Then resolve any instrumentation that needs to be translated to before/after/alt
        tmp.resolve_special_instrumentation(
            &func_mapping,
            &global_mapping,
            &memory_mapping,
            pull_side_effects,
            &mut side_effects,
        )?;

        let mut module = wasm_encoder::Module::new();
        let mut reencode = RoundtripReencoder;

        let new_start = if let Some(start_fn) = tmp.start {
            // fix the start function mapping
            match func_mapping.get(&*start_fn) {
                Some(new_index) => Some(FunctionID(*new_index)),
                None => {
                    warn!("Deleted the start function!");
                    None
                }
            }
        } else {
            None
        };
        tmp.start = new_start;

        // handle recursion groups
        if !tmp.types.groups.is_empty() {
            let mut type_sect = wasm_encoder::TypeSection::new();
            for RecGroup { types, is_explicit } in tmp.types.groups.iter() {
                let mut subtypes = vec![];
                for ty_id in types.iter() {
                    let ty = tmp.types.types.get(ty_id).unwrap();
                    if pull_side_effects {
                        if let Some(tag) = ty.get_tag() {
                            add_injection(
                                &mut side_effects,
                                InjectType::Type,
                                Injection::Type {
                                    ty: ty.clone(),
                                    tag: tag.clone(),
                                },
                            );
                        }
                    }

                    let subtype = tmp.encode_type(ty);
                    if *is_explicit {
                        subtypes.push(subtype);
                    } else {
                        type_sect.ty().subtype(&subtype);
                    }
                }

                if *is_explicit {
                    type_sect.ty().rec(subtypes);
                }
            }
            module.section(&type_sect);
        }

        // initialize function name section
        let mut function_names = wasm_encoder::NameMap::new();
        let mut local_names = wasm_encoder::IndirectNameMap::new();
        if !tmp.imports.is_empty() {
            let mut imports = wasm_encoder::ImportSection::new();
            let mut import_func_idx = 0;
            for import in tmp.imports.iter() {
                if !import.deleted {
                    if import.is_function() {
                        if let Some(import_name) = &import.custom_name {
                            function_names.append(import_func_idx as u32, import_name);
                        }
                        import_func_idx += 1;
                    }
                    imports.import(
                        &import.module,
                        &import.name,
                        reencode.entity_type(import.ty).unwrap(),
                    );
                }
                if pull_side_effects {
                    if let Some(tag) = import.get_tag() {
                        add_injection(
                            &mut side_effects,
                            InjectType::Import,
                            Injection::Import {
                                module: import.module.to_string(),
                                name: import.name.to_string(),
                                type_ref: import.ty,
                                tag: tag.clone(),
                            },
                        );
                    }
                }
            }
            module.section(&imports);
        }

        if !tmp.functions.is_empty() {
            let mut functions = wasm_encoder::FunctionSection::new();
            for func in tmp.functions.iter() {
                if !func.deleted {
                    if let FuncKind::Local(l) = func.kind() {
                        functions.function(*l.ty_id);
                        if pull_side_effects {
                            if let Some(tag) = l.get_tag() {
                                let sig = tmp.types.get(l.ty_id).unwrap_or_else(|| {
                                    panic!("Could not find type for type ID: {}", *l.ty_id)
                                });
                                add_injection(
                                    &mut side_effects,
                                    InjectType::Func,
                                    Injection::Func {
                                        id: *l.func_id,
                                        fname: l.body.name.clone(),
                                        sig: (sig.params()?, sig.results()?),
                                        locals: l.body.locals_as_vec(),
                                        tag: tag.clone(),
                                        body: l.body.instructions.get_ops().to_vec(),
                                    },
                                );
                            }
                        }
                    }
                }
            }
            module.section(&functions);
        }

        if !tmp.tables.is_empty() {
            let mut tables = wasm_encoder::TableSection::new();
            for table in tmp.tables.iter() {
                let table_ty = wasm_encoder::TableType {
                    element_type: wasm_encoder::RefType {
                        nullable: table.ty.element_type.is_nullable(),
                        heap_type: reencode
                            .heap_type(table.ty.element_type.heap_type())
                            .unwrap(),
                    },
                    table64: table.ty.table64,
                    minimum: table.ty.initial,
                    maximum: table.ty.maximum,
                    shared: table.ty.shared,
                };
                match &table.init_expr {
                    None => tables.table(table_ty),
                    Some(const_expr) => {
                        let mut init = InitExpr::eval(const_expr)?;
                        for i in init.exprs.iter_mut() {
                            i.fix_id_mapping(&func_mapping, &global_mapping)?;
                        }
                        tables.table_with_init(table_ty, &init.to_wasmencoder_type())
                    }
                };

                if pull_side_effects {
                    if let Some(tag) = table.get_tag() {
                        add_injection(
                            &mut side_effects,
                            InjectType::Table,
                            Injection::Table { tag: tag.clone() },
                        );
                    }
                }
            }
            module.section(&tables);
        }

        if !tmp.memories.is_empty() {
            let mut memories = wasm_encoder::MemorySection::new();
            for memory in tmp.memories.iter() {
                if memory.is_local() {
                    memories.memory(wasm_encoder::MemoryType::from(memory.ty));

                    if pull_side_effects {
                        if let Some(tag) = memory.get_tag() {
                            add_injection(
                                &mut side_effects,
                                InjectType::Memory,
                                Injection::Memory {
                                    id: memory.get_id(),
                                    initial: memory.ty.initial,
                                    maximum: memory.ty.maximum,
                                    tag: tag.clone(),
                                },
                            );
                        }
                    }
                }
            }
            module.section(&memories);
        }

        if !tmp.tags.is_empty() {
            let mut tags = TagSection::new();
            for tag in tmp.tags.iter() {
                tags.tag(wasm_encoder::TagType {
                    kind: wasm_encoder::TagKind::from(tag.kind),
                    func_type_idx: tag.func_type_idx,
                });
            }
            module.section(&tags);
        }

        if !tmp.globals.is_empty() {
            let mut globals = wasm_encoder::GlobalSection::new();
            for global in tmp.globals.iter_mut() {
                if !global.deleted {
                    // save these off for the side effect processing before matching on global.kind (due to rust borrow issues)
                    let id = global.get_id();
                    let tag = global.get_tag().clone();
                    if let GlobalKind::Local(LocalGlobal { ty, init_expr, .. }) = &mut global.kind {
                        for expr in init_expr.exprs.iter_mut() {
                            expr.fix_id_mapping(&func_mapping, &global_mapping)?;
                        }
                        globals.global(
                            wasm_encoder::GlobalType {
                                val_type: reencode.val_type(ty.content_type).unwrap(),
                                mutable: ty.mutable,
                                shared: ty.shared,
                            },
                            &init_expr.to_wasmencoder_type(),
                        );

                        if pull_side_effects {
                            if let Some(tag) = tag {
                                add_injection(
                                    &mut side_effects,
                                    InjectType::Global,
                                    Injection::Global {
                                        id,
                                        ty: DataType::from(ty.content_type),
                                        shared: ty.shared,
                                        mutable: ty.mutable,
                                        tag,
                                        init_expr: init_expr.clone(),
                                    },
                                );
                            }
                        }
                    }
                }
                // skip imported globals
            }
            module.section(&globals);
        }

        if !tmp.exports.is_empty() {
            let mut exports = wasm_encoder::ExportSection::new();
            for export in tmp.exports.iter() {
                if !export.deleted {
                    match export.kind {
                        ExternalKind::Func => {
                            // Update the function indices
                            exports.export(
                                &export.name,
                                wasm_encoder::ExportKind::from(export.kind),
                                *func_mapping.get(&(export.index)).unwrap(),
                            );
                        }
                        ExternalKind::Memory => {
                            // Update the memory indices
                            exports.export(
                                &export.name,
                                wasm_encoder::ExportKind::from(export.kind),
                                *memory_mapping.get(&(export.index)).unwrap(),
                            );
                        }
                        ExternalKind::Global => {
                            // Update the global indices
                            let Some(new_index) = global_mapping.get(&export.index) else {
                                return Err(InstrumentationError(format!(
                                    "Export `{}` references a missing global, ID: {}",
                                    export.name, export.index
                                )));
                            };
                            exports.export(
                                &export.name,
                                wasm_encoder::ExportKind::from(export.kind),
                                *new_index,
                            );
                        }
                        _ => {
                            exports.export(
                                &export.name,
                                wasm_encoder::ExportKind::from(export.kind),
                                export.index,
                            );
                        }
                    }
                    if pull_side_effects {
                        if let Some(tag) = export.get_tag() {
                            add_injection(
                                &mut side_effects,
                                InjectType::Export,
                                Injection::Export {
                                    name: export.name.clone(),
                                    kind: export.kind,
                                    index: export.index,
                                    tag: tag.clone(),
                                },
                            );
                        }
                    }
                }
            }
            module.section(&exports);
        }

        if let Some(function_index) = tmp.start {
            module.section(&wasm_encoder::StartSection {
                function_index: *function_index,
            });
        }

        if !tmp.elements.is_empty() {
            let mut elements = wasm_encoder::ElementSection::new();
            let mut temp_const_exprs = vec![];
            let mut element_items = vec![];
            for element in tmp.elements.iter_mut() {
                temp_const_exprs.clear();
                element_items.clear();
                let (items, kind) = (&mut element.items, &mut element.kind);
                let element_items = match items {
                    // TODO: Update the elements section based on additions/deletion
                    ElementItems::Functions(funcs) => {
                        element_items = funcs
                            .iter()
                            .map(|f| *func_mapping.get(f).unwrap())
                            .collect();
                        wasm_encoder::Elements::Functions(Cow::from(element_items.as_slice()))
                    }
                    ElementItems::ConstExprs { ty, exprs } => {
                        temp_const_exprs.reserve(exprs.len());
                        for e in exprs.iter_mut() {
                            for i in e.exprs.iter_mut() {
                                i.fix_id_mapping(&func_mapping, &global_mapping)?;
                            }
                            temp_const_exprs.push(e.to_wasmencoder_type());
                        }
                        wasm_encoder::Elements::Expressions(
                            wasm_encoder::RefType {
                                nullable: ty.is_nullable(),
                                heap_type: reencode.heap_type(ty.heap_type()).unwrap(),
                            },
                            Cow::from(&temp_const_exprs),
                        )
                    }
                };

                match kind {
                    ElementKind::Passive => {
                        elements.passive(element_items);
                    }
                    ElementKind::Active {
                        table_index,
                        offset_expr,
                    } => {
                        for e in offset_expr.exprs.iter_mut() {
                            e.fix_id_mapping(&func_mapping, &global_mapping)?;
                        }
                        elements.active(
                            *table_index,
                            &offset_expr.to_wasmencoder_type(),
                            element_items,
                        );
                    }
                    ElementKind::Declared => {
                        elements.declared(element_items);
                    }
                }

                if pull_side_effects {
                    if let Some(tag) = element.get_tag() {
                        add_injection(
                            &mut side_effects,
                            InjectType::Element,
                            Injection::Element { tag: tag.clone() },
                        );
                    }
                }
            }
            module.section(&elements);
        }

        if tmp.data_count_section_exists {
            let data_count = wasm_encoder::DataCountSection {
                count: tmp.data.len() as u32,
            };
            module.section(&data_count);
        }

        if !tmp.num_local_functions > 0 {
            let mut code = wasm_encoder::CodeSection::new();

            #[cfg(feature = "parallel")]
            let functions = {
                let functions_mut = tmp.functions.iter_mut().collect::<Vec<_>>();
                functions_mut
                    .into_par_iter()
                    .enumerate()
                    .filter_map(|(idx, f)| {
                        if f.is_deleted() || f.is_import() {
                            return None;
                        }
                        let f = f.unwrap_local_mut().expect(
                            "Internal error: Should have found the local function successfully!",
                        );
                        let encoded_func = Self::encode_function(
                            f,
                            &func_mapping,
                            &global_mapping,
                            &memory_mapping,
                        );
                        Some((idx, f, encoded_func))
                    })
                    .collect::<Vec<_>>()
            };

            #[cfg(not(feature = "parallel"))]
            let functions = tmp
                .functions
                .iter_mut()
                .enumerate()
                .filter_map(|(idx, f)| {
                    if f.is_deleted() || f.is_import() {
                        return None;
                    }
                    let f = f.unwrap_local_mut().expect(
                        "Internal error: Should have found the local function successfully!",
                    );
                    let encoded_func =
                        Self::encode_function(f, &func_mapping, &global_mapping, &memory_mapping);
                    Some((idx, f, encoded_func))
                })
                .collect::<Vec<_>>();

            for (idx, original_func, function) in functions {
                // at this point the IDs in all the function instrumentation opcodes have been corrected
                // add the probe side effects!
                if pull_side_effects {
                    original_func.add_opcode_injections(idx as u32, &mut side_effects);
                }
                if let Some(name) = &original_func.body.name {
                    function_names.append(idx as u32, name.as_str());
                }
                if let Some(local_map) = &original_func.body.local_names {
                    local_names.append(idx as u32, local_map);
                }
                code.function(&function?);
            }
            module.section(&code);
        }

        if !tmp.data.is_empty() {
            let mut data = wasm_encoder::DataSection::new();
            for segment in tmp.data.iter_mut() {
                // save this off for the side effect processing before matching on segment.kind (due to rust borrow issues)
                let tag = segment.get_tag().clone();
                let segment_data = segment.data.iter().copied();
                match &mut segment.kind {
                    DataSegmentKind::Passive => {
                        if pull_side_effects {
                            if let Some(tag) = segment.get_tag() {
                                add_injection(
                                    &mut side_effects,
                                    InjectType::Data,
                                    Injection::PassiveData {
                                        data: segment.data.to_vec(),
                                        tag: tag.to_owned(),
                                    },
                                );
                            }
                        }
                        data.passive(segment_data)
                    }
                    DataSegmentKind::Active {
                        memory_index,
                        offset_expr,
                    } => {
                        for expr in offset_expr.exprs.iter_mut() {
                            expr.fix_id_mapping(&func_mapping, &global_mapping)?;
                        }
                        let new_idx = match memory_mapping.get(memory_index) {
                            Some(new_index) => *new_index,
                            None => {
                                return Err(InstrumentationError(format!(
                                    "Attempting to reference a deleted memory, ID: {}",
                                    memory_index
                                )))
                            }
                        };
                        if pull_side_effects {
                            if let Some(tag) = tag {
                                add_injection(
                                    &mut side_effects,
                                    InjectType::Data,
                                    Injection::ActiveData {
                                        memory_index: *memory_index,
                                        offset_expr: offset_expr.clone(),
                                        data: segment.data.to_vec(),
                                        tag: tag.to_owned(),
                                    },
                                );
                            }
                        }

                        data.active(new_idx, &offset_expr.to_wasmencoder_type(), segment_data)
                    }
                };
            }
            module.section(&data);
        }

        // the name section is not stored in tmp.custom_sections anymore
        let mut names = wasm_encoder::NameSection::new();

        if let Some(module_name) = &tmp.module_name {
            names.module(module_name);
        }
        names.functions(&function_names);
        names.locals(&local_names);
        let mut label_names = wasm_encoder::IndirectNameMap::new();
        for (idx, labels) in remap_names(&tmp.label_names, &func_mapping) {
            label_names.append(idx, labels);
        }
        let mut memory_names = wasm_encoder::NameMap::new();
        for (idx, name) in remap_names(&tmp.memory_names, &memory_mapping) {
            memory_names.append(idx, name);
        }
        let mut global_names = wasm_encoder::NameMap::new();
        for (idx, name) in remap_names(&tmp.global_names, &global_mapping) {
            global_names.append(idx, name);
        }
        names.labels(&label_names);
        names.types(&tmp.type_names);
        names.tables(&tmp.table_names);
        names.memories(&memory_names);
        names.globals(&global_names);
        names.elements(&tmp.elem_names);
        names.data(&tmp.data_names);
        names.fields(&tmp.field_names);
        names.tag(&tmp.tag_names);
        names.parameters(&tmp.parameter_names);
        names.tag_parameters(&tmp.tag_parameter_names);

        module.section(&names);

        // encode the rest of custom sections
        for section in tmp.custom_sections.iter() {
            if section.deleted {
                continue;
            }
            module.section(&wasm_encoder::CustomSection {
                name: std::borrow::Cow::Borrowed(section.name),
                data: section.data.clone(),
            });
        }

        Ok((module, side_effects))
    }

    // ==============================
    // ==== Module Manipulations ====
    // ==============================

    /// Add a new Data Segment to the module.
    /// Returns the index of the new Data Segment in the Data Section.
    pub fn add_data(&mut self, data: DataSegment) -> DataSegmentID {
        let index = self.data.len();
        self.data.push(data);
        DataSegmentID(index as u32)
    }

    /// Get the memory ID of a module. Does not support multiple memories
    pub fn get_memory_id(&self) -> Option<MemoryID> {
        if self.memories.as_vec().len() > 1 {
            unimplemented!("multiple memories unsupported")
        }

        if !self.memories.is_empty() {
            return Some(MemoryID(0));
        }
        // module does not export a memory
        None
    }

    pub(crate) fn add_import(&mut self, import: Import<'a>) -> (u32, ImportsID) {
        let (num_local, num_imported, num_total) = match import.ty {
            TypeRef::Func(..) => (
                self.num_local_functions,
                self.imports.num_funcs,
                self.functions.as_vec().len() as u32,
            ),
            TypeRef::Global(..) => (
                self.num_local_globals,
                self.imports.num_globals,
                self.globals.len() as u32,
            ),
            TypeRef::Table(..) => todo!(),
            TypeRef::Tag(..) => todo!(),
            TypeRef::Memory(..) => (
                self.num_local_memories,
                self.imports.num_memories,
                self.memories.as_vec().len() as u32,
            ),
            TypeRef::FuncExact(..) => (
                self.num_local_functions,
                self.imports.num_funcs,
                self.functions.as_vec().len() as u32,
            ),
        };

        let id = if num_local > 0 {
            num_total
        } else {
            num_imported
        };
        (id, self.imports.add(import))
    }

    pub fn add_custom_section(&mut self, section: CustomSection<'a>) -> CustomSectionID {
        self.custom_sections.add(section)
    }

    /// Delete a custom section from the module. The encoder skips deleted entries.
    pub fn delete_custom_section(&mut self, id: CustomSectionID) {
        if *id < self.custom_sections.len() as u32 {
            self.custom_sections.custom_sections[*id as usize].deleted = true;
        }
    }

    // ===========================
    // ==== Memory Management ====
    // ===========================

    /// Add a local memory to the module.
    pub fn add_local_memory(&mut self, ty: MemoryType) -> MemoryID {
        self.add_local_memory_with_tag(ty, Tag::default())
    }

    pub fn add_local_memory_with_tag(&mut self, ty: MemoryType, tag: Tag) -> MemoryID {
        let local_mem = LocalMemory {
            mem_id: MemoryID(0), // will be fixed
        };

        self.num_local_memories += 1;
        self.memories.add_local_mem(local_mem, ty, Some(tag))
    }

    /// Add a local memory to the module with an appended tag.
    pub fn add_import_memory(
        &mut self,
        module: String,
        name: String,
        ty: MemoryType,
    ) -> (MemoryID, ImportsID) {
        self.add_import_memory_with_tag(module, name, ty, Tag::default())
    }
    /// Add an imported memory to the module with an appended tag.
    pub fn add_import_memory_with_tag(
        &mut self,
        module: String,
        name: String,
        ty: MemoryType,
        tag: Tag,
    ) -> (MemoryID, ImportsID) {
        let (imp_mem_id, imp_id) = self.add_import(Import {
            module: module.into(),
            name: name.into(),
            ty: TypeRef::Memory(ty),
            custom_name: None,
            deleted: false,
            tag: Some(tag.clone()),
        });

        // Add to memories as well as it has imported memories
        self.memories
            .add_import_mem(imp_id, ty, imp_mem_id, Some(tag));
        (MemoryID(imp_mem_id), imp_id)
    }

    /// Delete a memory from the module.
    pub fn delete_memory(&mut self, mem_id: MemoryID) {
        self.memories.delete(mem_id);
        if let MemKind::Import(ImportedMemory { import_id, .. }) = self.memories.get_kind(mem_id) {
            self.imports.delete(*import_id);
        }
    }

    // =============================
    // ==== Function Management ====
    // =============================

    pub(crate) fn add_local_func_with_tag(
        &mut self,
        name: Option<String>,
        params: &[DataType],
        results: &[DataType],
        body: Body<'a>,
        tag: Tag,
    ) -> FunctionID {
        let ty = self
            .types
            .add_func_type_internal(params, results, Some(tag.clone()));
        let local_func = LocalFunction::new(
            ty,
            FunctionID(0), // will be fixed
            body,
            params.len(),
            Some(tag),
        );

        self.num_local_functions += 1;
        self.functions.add_local_func(local_func, name.clone())
    }

    /// Add a new function to the module, returns:
    ///
    /// - FunctionID: The ID that indexes into the function ID space. To be used when referring to the function, like in `call`.
    /// - ImportsID: The ID that indexes into the import section.
    pub fn add_import_func(
        &mut self,
        module: String,
        name: String,
        ty_id: TypeID,
    ) -> (FunctionID, ImportsID) {
        self.add_import_func_with_tag(module, name, ty_id, Tag::default())
    }

    /// Add a new function to the module, returns:
    ///
    /// - FunctionID: The ID that indexes into the function ID space. To be used when referring to the function, like in `call`.
    /// - ImportsID: The ID that indexes into the import section.
    pub fn add_import_func_with_tag(
        &mut self,
        module: String,
        name: String,
        ty_id: TypeID,
        tag: Tag,
    ) -> (FunctionID, ImportsID) {
        let (imp_fn_id, imp_id) = self.add_import(Import {
            module: module.into(),
            name: name.clone().into(),
            ty: TypeRef::Func(*ty_id),
            custom_name: None,
            deleted: false,
            tag: Some(tag),
        });

        // Add to functions as well as it has imported functions
        self.functions
            .add_import_func(imp_id, ty_id, Some(name), imp_fn_id);
        (FunctionID(imp_fn_id), imp_id)
    }

    /// Get the number of imported functions in the module (including any added ones).
    pub fn num_import_func(&self) -> u32 {
        self.imports.num_funcs
    }

    /// Get the number of imported globals in the module (including any added ones).
    pub fn num_import_global(&self) -> u32 {
        self.imports.num_globals
    }

    /// Get the number of imported memories in the module (including any added ones).
    pub fn num_import_memory(&self) -> u32 {
        self.imports.num_memories
    }

    /// Delete a function from the module.
    pub fn delete_func(&mut self, function_id: FunctionID) {
        self.functions.delete(function_id);
        if let FuncKind::Import(ImportedFunction { import_id, .. }) =
            self.functions.get_kind(function_id)
        {
            self.imports.delete(*import_id);
        }
    }

    /// Convert an imported function to a local function.
    /// The function ID inside the `local_function` parameter should equal the `function_id` specified.
    /// Continue using the FunctionID as normal (like in `call` instructions), this library will take care of ID changes for you during encoding.
    /// Returns false if it is a local function.
    pub(crate) fn convert_import_fn_to_local(
        &mut self,
        function_id: FunctionID,
        local_function: LocalFunction<'a>,
    ) -> bool {
        if self.functions.is_local(function_id) {
            warn!("This is a local function!");
            return false;
        }
        self.delete_func(function_id);
        self.functions
            .get_mut(function_id)
            .set_kind(FuncKind::Local(Box::new(local_function)));
        true
    }

    /// Convert a local function to an imported function.
    /// Continue using the FunctionID as normal (like in `call` instructions), this library will take care of ID changes for you during encoding.
    /// Returns false if it is an imported function.
    pub fn convert_local_fn_to_import(
        &mut self,
        function_id: FunctionID,
        module: String,
        name: String,
        ty_id: TypeID,
    ) -> bool {
        self.convert_local_fn_to_import_with_tag(function_id, module, name, ty_id, Tag::default())
    }

    /// Convert a local function to an imported function and append a tag to this operation.
    /// Continue using the FunctionID as normal (like in `call` instructions), this library will take care of ID changes for you during encoding.
    /// Returns false if it is an imported function.
    pub fn convert_local_fn_to_import_with_tag(
        &mut self,
        function_id: FunctionID,
        module: String,
        name: String,
        ty_id: TypeID,
        tag: Tag,
    ) -> bool {
        if self.functions.is_import(function_id) {
            warn!("This is an imported function!");
            return false;
        }
        // Delete the associated function
        self.delete_func(function_id);
        // Add import function to imports
        let (.., import_id) = self.add_import(Import {
            module: module.into(),
            name: name.clone().into(),
            ty: TypeRef::Func(*ty_id),
            custom_name: None,
            deleted: false,
            tag: Some(tag),
        });
        self.functions
            .get_mut(function_id)
            .set_kind(FuncKind::Import(ImportedFunction {
                import_id,
                import_fn_id: function_id,
                ty_id,
            }));
        let ok = self.functions.set_imported_fn_name(function_id, name);
        debug_assert!(ok);
        true
    }

    /// Set the name of a function using its ID.
    pub fn set_fn_name(&mut self, id: FunctionID, name: String) {
        if *id < self.imports.num_funcs {
            // the function is an import
            self.imports.set_fn_name(name.clone(), id);
            let ok = self.functions.set_imported_fn_name(id, name);
            debug_assert!(ok);
        } else {
            // the function is local
            let ok = self.functions.set_local_fn_name(id, name);
            debug_assert!(ok);
        }
    }

    // =============================
    // ==== Globals Management ====
    // =============================

    /// Add a new global to the module.
    pub(crate) fn add_global_internal(&mut self, global: Global) -> GlobalID {
        self.num_local_globals += 1;
        self.globals.add(global)
    }

    /// Create a new locally-defined global and add it to the module.
    pub fn add_global(
        &mut self,
        init_expr: InitExpr,
        content_ty: DataType,
        mutable: bool,
        shared: bool,
    ) -> GlobalID {
        self.add_global_with_tag(init_expr, content_ty, mutable, shared, Tag::default())
    }

    /// Create a new locally-defined global and add it to the module.
    pub fn add_global_with_tag(
        &mut self,
        init_expr: InitExpr,
        content_ty: DataType,
        mutable: bool,
        shared: bool,
        tag: Tag,
    ) -> GlobalID {
        self.add_global_internal(Global {
            kind: GlobalKind::Local(LocalGlobal {
                global_id: GlobalID(0), // gets set in `add`
                ty: GlobalType {
                    mutable,
                    content_type: wasmparser::ValType::from(&content_ty),
                    shared,
                },
                init_expr,
            }),
            deleted: false,
            tag: Some(tag),
        })
    }

    /// Add a new imported global to the module, returns:
    ///
    /// - GlobalID: The ID that indexes into the global ID space. To be used when referring to the global, like in `global.get`.
    /// - ImportsID: The ID that indexes into the import section.
    pub fn add_imported_global(
        &mut self,
        module: String,
        name: String,
        content_ty: DataType,
        mutable: bool,
        shared: bool,
    ) -> (GlobalID, ImportsID) {
        self.add_imported_global_with_tag(module, name, content_ty, mutable, shared, Tag::default())
    }

    /// Add a new imported global to the module, returns:
    ///
    /// - GlobalID: The ID that indexes into the global ID space. To be used when referring to the global, like in `global.get`.
    /// - ImportsID: The ID that indexes into the import section.
    pub fn add_imported_global_with_tag(
        &mut self,
        module: String,
        name: String,
        content_ty: DataType,
        mutable: bool,
        shared: bool,
        tag: Tag,
    ) -> (GlobalID, ImportsID) {
        let global_ty = GlobalType {
            mutable,
            content_type: wasmparser::ValType::from(&content_ty),
            shared,
        };
        let (imp_global_id, imp_id) = self.add_import(Import {
            module: module.into(),
            name: name.into(),
            ty: TypeRef::Global(global_ty),
            custom_name: None,
            deleted: false,
            tag: Some(tag.clone()),
        });

        // Add to globals as well since it has imported globals
        self.add_global_internal(Global::new(
            GlobalKind::Import(ImportedGlobal::new(
                imp_id,
                GlobalID(imp_global_id),
                global_ty,
            )),
            Some(tag),
        ));
        self.globals.recalculate_ids = true;
        (GlobalID(imp_global_id), imp_id)
    }

    /// Delete a global from the module (can either be an imported or locally-defined global).
    /// Use the global ID for this operation, not the import ID!
    pub fn delete_global(&mut self, global_id: GlobalID) {
        self.globals.delete(global_id);
        if let GlobalKind::Import(ImportedGlobal { import_id, .. }) =
            self.globals.get_kind(global_id)
        {
            self.imports.delete(*import_id);
        }
    }

    /// Change a locally-defined global's init expression.
    pub fn mod_global_init_expr(
        &mut self,
        global_id: GlobalID,
        new_expr: InitExpr,
    ) -> types::Result<()> {
        self.globals.mod_global_init_expr(*global_id, new_expr)
    }
}

pub trait GetID {
    fn get_id(&self) -> u32;
}

pub(crate) trait AsVec<T> {
    fn as_vec(&self) -> &Vec<T>;
    fn as_vec_mut(&mut self) -> &mut Vec<T>;
}

pub trait Push<T> {
    fn push(&mut self, val: T);
}

pub trait LocalOrImport {
    fn is_local(&self) -> bool;
    fn is_import(&self) -> bool;
    fn is_deleted(&self) -> bool;
}

pub(crate) fn add_injection<'a>(
    side_effects: &mut HashMap<InjectType, Vec<Injection<'a>>>,
    ty: InjectType,
    inj: Injection<'a>,
) {
    side_effects
        .entry(ty)
        .and_modify(|list: &mut Vec<Injection>| list.push(inj.clone()))
        .or_insert(vec![inj]);
}
