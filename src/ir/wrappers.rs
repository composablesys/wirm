//! Wrapper functions

use std::collections::HashMap;

use wasm_encoder::reencode::{Reencode, ReencodeComponent, RoundtripReencoder};
use wasm_encoder::{
    Alias, ComponentCoreTypeEncoder, ComponentFuncTypeEncoder, ComponentTypeEncoder,
    CoreTypeEncoder, InstanceType,
};
use wasmparser::{
    ComponentAlias, ComponentType, ComponentTypeDeclaration, ComponentValType, CoreType,
    InstanceTypeDeclaration, Operator, SubType,
};

// Not added to wasm-tools
/// Convert ModuleTypeDeclaration to ModuleType
pub fn convert_module_type_declaration(
    module: &[wasmparser::ModuleTypeDeclaration],
    enc: ComponentCoreTypeEncoder,
    reencode: &mut RoundtripReencoder,
) {
    let mut mty = wasm_encoder::ModuleType::new();
    for m in module.iter() {
        match m {
            wasmparser::ModuleTypeDeclaration::Type(recgroup) => {
                let types = recgroup
                    .types()
                    .map(|ty| {
                        reencode.sub_type(ty.to_owned()).unwrap_or_else(|_| {
                            panic!("Could not encode type as subtype: {:?}", ty)
                        })
                    })
                    .collect::<Vec<_>>();

                if recgroup.is_explicit_rec_group() {
                    mty.ty().rec(types);
                } else {
                    // it's implicit!
                    for subty in types {
                        mty.ty().subtype(&subty);
                    }
                }
            }
            wasmparser::ModuleTypeDeclaration::Export { name, ty } => {
                mty.export(name, reencode.entity_type(*ty).unwrap());
            }
            wasmparser::ModuleTypeDeclaration::OuterAlias {
                kind: _kind,
                count,
                index,
            } => {
                mty.alias_outer_core_type(*count, *index);
            }
            wasmparser::ModuleTypeDeclaration::Import(import) => {
                mty.import(
                    import.module,
                    import.name,
                    reencode.entity_type(import.ty).unwrap(),
                );
            }
        }
    }
    enc.module(&mty);
}

// Not added to wasm-tools
/// Convert Instance Types
pub fn convert_instance_type(
    instance: &[InstanceTypeDeclaration],
    reencode: &mut RoundtripReencoder,
) -> InstanceType {
    let mut ity = InstanceType::new();
    for value in instance.iter() {
        match value {
            InstanceTypeDeclaration::CoreType(core_type) => match core_type {
                CoreType::Rec(recgroup) => {
                    for sub in recgroup.types() {
                        let enc = ity.core_type().core();
                        encode_core_type_subtype(enc, sub, reencode);
                    }
                }
                CoreType::Module(module) => {
                    let enc = ity.core_type();
                    convert_module_type_declaration(module, enc, reencode);
                }
            },
            InstanceTypeDeclaration::Type(ty) => {
                let enc = ity.ty();
                convert_component_type(ty, enc, reencode);
            }
            InstanceTypeDeclaration::Alias(alias) => match alias {
                ComponentAlias::InstanceExport {
                    kind,
                    instance_index,
                    name,
                } => {
                    ity.alias(Alias::InstanceExport {
                        instance: *instance_index,
                        kind: reencode.component_export_kind(*kind),
                        name,
                    });
                }
                ComponentAlias::CoreInstanceExport {
                    kind,
                    instance_index,
                    name,
                } => {
                    ity.alias(Alias::CoreInstanceExport {
                        instance: *instance_index,
                        kind: do_reencode(
                            *kind,
                            RoundtripReencoder::export_kind,
                            reencode,
                            "export kind",
                        ),
                        name,
                    });
                }
                ComponentAlias::Outer { kind, count, index } => {
                    ity.alias(Alias::Outer {
                        kind: reencode.component_outer_alias_kind(*kind),
                        count: *count,
                        index: *index,
                    });
                }
            },
            InstanceTypeDeclaration::Export { name, ty } => {
                ity.export(
                    name.0,
                    do_reencode(
                        *ty,
                        RoundtripReencoder::component_type_ref,
                        reencode,
                        "component type",
                    ),
                );
            }
        }
    }
    ity
}

// Not added to wasm-tools
/// Convert Func Results
pub fn convert_results(
    result: Option<ComponentValType>,
    mut enc: ComponentFuncTypeEncoder,
    reencode: &mut RoundtripReencoder,
) {
    enc.result(result.map(|v| reencode.component_val_type(v)));
}

// Not added to wasm-tools
/// CoreTypeEncoding
pub fn encode_core_type_subtype(
    enc: CoreTypeEncoder,
    subtype: &SubType,
    reencode: &mut RoundtripReencoder,
) {
    let subty = reencode
        .sub_type(subtype.to_owned())
        .unwrap_or_else(|_| panic!("Could not encode type as subtype: {:?}", subtype));
    enc.subtype(&subty);
}

/// Process Alias
pub fn process_alias<'a>(
    a: &'a ComponentAlias<'a>,
    reencode: &mut RoundtripReencoder,
) -> Alias<'a> {
    match a {
        ComponentAlias::InstanceExport {
            kind,
            instance_index,
            name,
        } => {
            println!("[process_alias::InstanceExport] with name: {name}");
            Alias::InstanceExport {
                instance: *instance_index,
                kind: reencode.component_export_kind(*kind),
                name,
            }
        },
        ComponentAlias::CoreInstanceExport {
            kind,
            instance_index,
            name,
        } => {
            println!("[process_alias::CoreInstanceExport] with name: {name}");
            Alias::CoreInstanceExport {
                instance: *instance_index,
                kind: do_reencode(
                    *kind,
                    RoundtripReencoder::export_kind,
                    reencode,
                    "export kind",
                ),
                name,
            }
        },
        ComponentAlias::Outer { kind, count, index } => Alias::Outer {
            kind: reencode.component_outer_alias_kind(*kind),
            count: *count,
            index: *index,
        },
    }
}

/// Convert Component Type
pub fn convert_component_type(
    ty: &ComponentType,
    enc: ComponentTypeEncoder,
    reencode: &mut RoundtripReencoder,
) {
    match ty {
        ComponentType::Defined(comp_ty) => {
            let def_enc = enc.defined_type();
            match comp_ty {
                wasmparser::ComponentDefinedType::Primitive(p) => {
                    def_enc.primitive(wasm_encoder::PrimitiveValType::from(*p))
                }
                wasmparser::ComponentDefinedType::Record(record) => {
                    def_enc.record(
                        record
                            .iter()
                            .map(|record| (record.0, reencode.component_val_type(record.1))),
                    );
                }
                wasmparser::ComponentDefinedType::Variant(variant) => {
                    def_enc.variant(variant.iter().map(|variant| {
                        (
                            variant.name,
                            variant.ty.map(|ty| reencode.component_val_type(ty)),
                            variant.refines,
                        )
                    }))
                }
                wasmparser::ComponentDefinedType::List(l) => {
                    def_enc.list(reencode.component_val_type(*l))
                }
                wasmparser::ComponentDefinedType::Tuple(tup) => def_enc.tuple(
                    tup.iter()
                        .map(|val_type| reencode.component_val_type(*val_type)),
                ),
                wasmparser::ComponentDefinedType::Flags(flags) => {
                    def_enc.flags((*flags).clone().into_vec())
                }
                wasmparser::ComponentDefinedType::Enum(en) => {
                    def_enc.enum_type((*en).clone().into_vec())
                }
                wasmparser::ComponentDefinedType::Option(opt) => {
                    def_enc.option(reencode.component_val_type(*opt))
                }
                wasmparser::ComponentDefinedType::Result { ok, err } => def_enc.result(
                    ok.map(|val_type| reencode.component_val_type(val_type)),
                    err.map(|val_type| reencode.component_val_type(val_type)),
                ),
                wasmparser::ComponentDefinedType::Own(u) => def_enc.own(*u),
                wasmparser::ComponentDefinedType::Borrow(u) => def_enc.borrow(*u),
                wasmparser::ComponentDefinedType::Future(opt) => match opt {
                    Some(u) => def_enc.future(Some(reencode.component_val_type(*u))),
                    None => def_enc.future(None),
                },
                wasmparser::ComponentDefinedType::Stream(opt) => match opt {
                    Some(u) => def_enc.stream(Some(reencode.component_val_type(*u))),
                    None => def_enc.future(None),
                },
                wasmparser::ComponentDefinedType::FixedSizeList(ty, len) => {
                    def_enc.fixed_size_list(reencode.component_val_type(*ty), *len)
                }
            }
        }
        ComponentType::Func(func_ty) => {
            let mut new_enc = enc.function();
            new_enc.params(
                func_ty
                    .clone()
                    .params
                    .into_vec()
                    .into_iter()
                    .map(|p| (p.0, reencode.component_val_type(p.1))),
            );
            convert_results(func_ty.clone().result, new_enc, reencode);
        }
        ComponentType::Component(comp) => {
            let mut new_comp = wasm_encoder::ComponentType::new();
            for c in comp.iter() {
                match c {
                    ComponentTypeDeclaration::CoreType(core) => match core {
                        CoreType::Rec(recgroup) => {
                            for sub in recgroup.types() {
                                let enc = new_comp.core_type().core();
                                encode_core_type_subtype(enc, sub, reencode);
                            }
                        }
                        CoreType::Module(module) => {
                            let enc = new_comp.core_type();
                            convert_module_type_declaration(module, enc, reencode);
                        }
                    },
                    ComponentTypeDeclaration::Type(typ) => {
                        let enc = new_comp.ty();
                        convert_component_type(typ, enc, reencode);
                    }
                    ComponentTypeDeclaration::Alias(a) => {
                        new_comp.alias(process_alias(a, reencode));
                    }
                    ComponentTypeDeclaration::Export { name, ty } => {
                        new_comp.export(
                            name.0,
                            do_reencode(
                                *ty,
                                RoundtripReencoder::component_type_ref,
                                reencode,
                                "component type",
                            ),
                        );
                    }
                    ComponentTypeDeclaration::Import(imp) => {
                        new_comp.import(
                            imp.name.0,
                            do_reencode(
                                imp.ty,
                                RoundtripReencoder::component_type_ref,
                                reencode,
                                "component type",
                            ),
                        );
                    }
                }
            }
            enc.component(&new_comp);
        }
        ComponentType::Instance(inst) => {
            let ity = convert_instance_type(inst, reencode);
            enc.instance(&ity);
        }
        ComponentType::Resource { rep, dtor } => {
            enc.resource(reencode.val_type(*rep).unwrap(), *dtor);
        }
    }
}

pub fn indirect_namemap_parser2encoder(
    namemap: wasmparser::IndirectNameMap,
) -> wasm_encoder::IndirectNameMap {
    let mut names = wasm_encoder::IndirectNameMap::new();
    for name in namemap {
        let naming = name.unwrap();
        names.append(naming.index, &namemap_parser2encoder(naming.names));
    }
    names
}

pub fn namemap_parser2encoder(namemap: wasmparser::NameMap) -> wasm_encoder::NameMap {
    let mut names = wasm_encoder::NameMap::new();
    for name in namemap {
        let naming = name.unwrap();
        names.append(naming.index, naming.name);
    }
    names
}

pub fn add_to_namemap(namemap: &mut wasm_encoder::NameMap, names: wasmparser::NameMap) {
    for name in names {
        let naming = name.unwrap();
        namemap.append(naming.index, naming.name);
    }
}

pub(crate) fn refers_to_func(op: &Operator) -> bool {
    matches!(
        op,
        Operator::Call { .. } | Operator::RefFunc { .. } | Operator::ReturnCall { .. }
    )
}

pub(crate) fn refers_to_global(op: &Operator) -> bool {
    matches!(
        op,
        Operator::GlobalGet { .. }
            | Operator::GlobalSet { .. }
            | Operator::GlobalAtomicGet { .. }
            | Operator::GlobalAtomicSet { .. }
            | Operator::GlobalAtomicRmwAdd { .. }
            | Operator::GlobalAtomicRmwAnd { .. }
            | Operator::GlobalAtomicRmwCmpxchg { .. }
            | Operator::GlobalAtomicRmwOr { .. }
            | Operator::GlobalAtomicRmwSub { .. }
            | Operator::GlobalAtomicRmwXchg { .. }
            | Operator::GlobalAtomicRmwXor { .. }
    )
}

pub(crate) fn refers_to_memory(op: &Operator) -> bool {
    matches!(
        op,
        Operator::I32Load { .. } |
        Operator::I32Load8S { .. } |
        Operator::I32Load8U { .. } |
        Operator::I32Load16S { .. } |
        Operator::I32Load16U { .. } |
        Operator::I64Load { .. } |
        Operator::I64Load8U { .. } |
        Operator::I64Load8S { .. } |
        Operator::I64Load16U { .. } |
        Operator::I64Load16S { .. } |
        Operator::I64Load32U { .. } |
        Operator::I64Load32S { .. } |
        Operator::F32Load { .. } |
        Operator::F64Load { .. } |
        Operator::V128Load { .. } |
        Operator::I32AtomicLoad { .. } |
        Operator::I32AtomicLoad8U { .. } |
        Operator::I32AtomicLoad16U { .. } |
        Operator::I64AtomicLoad8U { .. } |
        Operator::I64AtomicLoad16U { .. } |
        Operator::I64AtomicLoad32U { .. } |
        Operator::V128Load8Lane { .. } |
        Operator::V128Load16Lane { .. } |
        Operator::V128Load32Lane { .. } |
        Operator::V128Load64Lane { .. } |
        Operator::V128Load8Splat { .. } |
        Operator::V128Load16Splat { .. } |
        Operator::V128Load32Splat { .. } |
        Operator::V128Load64Splat { .. } |
        Operator::V128Load8x8S { .. } |
        Operator::V128Load8x8U { .. } |
        Operator::V128Load16x4U { .. } |
        Operator::V128Load16x4S { .. } |
        Operator::V128Load32Zero { .. } |
        Operator::V128Load32x2S { .. } |
        Operator::V128Load32x2U { .. } |
        Operator::V128Load64Zero { .. } |

        // stores
        Operator::I32Store { .. } |
        Operator::I32Store8 { .. } |
        Operator::I32Store16 { .. } |
        Operator::I64Store { .. } |
        Operator::I64Store8 { .. } |
        Operator::I64Store16 { .. } |
        Operator::I64Store32 { .. } |
        Operator::F32Store { .. } |
        Operator::F64Store { .. } |
        Operator::I32AtomicStore { .. } |
        Operator::I32AtomicStore8 { .. } |
        Operator::I32AtomicStore16 { .. } |
        Operator::I64AtomicStore { .. } |
        Operator::I64AtomicStore8 { .. } |
        Operator::I64AtomicStore16 { .. } |
        Operator::I64AtomicStore32 { .. } |
        Operator::V128Store { .. } |
        Operator::V128Store8Lane { .. } |
        Operator::V128Store16Lane { .. } |
        Operator::V128Store32Lane { .. } |
        Operator::V128Store64Lane { .. } |

        // memory operations
        Operator::MemoryAtomicNotify { .. } |
        Operator::MemoryAtomicWait32 { .. } |
        Operator::MemoryAtomicWait64 { .. } |
        Operator::MemoryGrow { .. } |
        Operator::MemoryFill { .. } |
        Operator::MemoryInit { .. } |
        Operator::MemorySize { .. } |
        Operator::MemoryDiscard { .. } |
        Operator::MemoryCopy { .. }
    )
}

pub(crate) fn update_fn_instr(op: &mut Operator, mapping: &HashMap<u32, u32>) {
    match op {
        Operator::Call { function_index }
        | Operator::RefFunc { function_index }
        | Operator::ReturnCall { function_index } => match mapping.get(&(*function_index)) {
            Some(new_index) => {
                *function_index = *new_index;
            }
            None => panic!("Deleted function!"),
        },
        _ => panic!("Operation doesn't need to be checked for function IDs!"),
    }
}

pub(crate) fn update_global_instr(op: &mut Operator, mapping: &HashMap<u32, u32>) {
    match op {
        Operator::GlobalGet { global_index }
        | Operator::GlobalSet { global_index }
        | Operator::GlobalAtomicGet { global_index, .. }
        | Operator::GlobalAtomicSet { global_index, .. }
        | Operator::GlobalAtomicRmwAdd { global_index, .. }
        | Operator::GlobalAtomicRmwAnd { global_index, .. }
        | Operator::GlobalAtomicRmwCmpxchg { global_index, .. }
        | Operator::GlobalAtomicRmwOr { global_index, .. }
        | Operator::GlobalAtomicRmwSub { global_index, .. }
        | Operator::GlobalAtomicRmwXchg { global_index, .. }
        | Operator::GlobalAtomicRmwXor { global_index, .. } => {
            match mapping.get(&(*global_index)) {
                Some(new_index) => {
                    *global_index = *new_index;
                }
                None => panic!("Deleted global!"),
            }
        }
        _ => panic!("Operation doesn't need to be checked for global IDs!"),
    }
}

pub(crate) fn update_memory_instr(op: &mut Operator, mapping: &HashMap<u32, u32>) {
    match op {
        // loads
        Operator::I32Load { memarg } |
        Operator::I32Load8S { memarg } |
        Operator::I32Load8U { memarg } |
        Operator::I32Load16S { memarg } |
        Operator::I32Load16U { memarg } |
        Operator::I64Load { memarg } |
        Operator::I64Load8U { memarg } |
        Operator::I64Load8S { memarg } |
        Operator::I64Load16U { memarg } |
        Operator::I64Load16S { memarg } |
        Operator::I64Load32U { memarg } |
        Operator::I64Load32S { memarg } |
        Operator::F32Load { memarg } |
        Operator::F64Load { memarg } |
        Operator::V128Load { memarg } |
        Operator::I32AtomicLoad { memarg } |
        Operator::I32AtomicLoad8U { memarg } |
        Operator::I32AtomicLoad16U { memarg } |
        Operator::I64AtomicLoad8U { memarg } |
        Operator::I64AtomicLoad16U { memarg } |
        Operator::I64AtomicLoad32U { memarg } |
        Operator::V128Load8Lane { memarg, .. } |
        Operator::V128Load16Lane { memarg, .. } |
        Operator::V128Load32Lane { memarg, .. } |
        Operator::V128Load64Lane { memarg, .. } |
        Operator::V128Load8Splat { memarg } |
        Operator::V128Load16Splat { memarg } |
        Operator::V128Load32Splat { memarg } |
        Operator::V128Load64Splat { memarg } |
        Operator::V128Load8x8S { memarg } |
        Operator::V128Load8x8U { memarg } |
        Operator::V128Load16x4U { memarg } |
        Operator::V128Load16x4S { memarg } |
        Operator::V128Load32Zero { memarg } |
        Operator::V128Load32x2S { memarg } |
        Operator::V128Load32x2U { memarg } |
        Operator::V128Load64Zero { memarg } |

        // stores
        Operator::I32Store {memarg} |
        Operator::I32Store8 {memarg} |
        Operator::I32Store16 {memarg} |
        Operator::I64Store {memarg} |
        Operator::I64Store8 {memarg} |
        Operator::I64Store16 {memarg} |
        Operator::I64Store32 {memarg} |
        Operator::F32Store {memarg} |
        Operator::F64Store {memarg} |
        Operator::I32AtomicStore {memarg} |
        Operator::I32AtomicStore8 {memarg} |
        Operator::I32AtomicStore16 {memarg} |
        Operator::I64AtomicStore {memarg} |
        Operator::I64AtomicStore8 {memarg} |
        Operator::I64AtomicStore16 {memarg} |
        Operator::I64AtomicStore32 {memarg} |
        Operator::V128Store {memarg} |
        Operator::V128Store8Lane {memarg, ..} |
        Operator::V128Store16Lane {memarg, ..} |
        Operator::V128Store32Lane {memarg, ..} |
        Operator::V128Store64Lane {memarg, ..} |

        // memory operations
        Operator::MemoryAtomicNotify {memarg} |
        Operator::MemoryAtomicWait32 {memarg} |
        Operator::MemoryAtomicWait64 {memarg} => {
            match mapping.get(&(memarg.memory)) {
                Some(new_index) => {
                    memarg.memory = *new_index;
                }
                None => panic!("Attempting to reference a deleted memory, ID: {}", memarg.memory),
            }
        }
        Operator::MemoryGrow {mem} |
        Operator::MemoryFill {mem} |
        Operator::MemoryInit {mem, ..} |
        Operator::MemorySize {mem} |
        Operator::MemoryDiscard {mem} => {
            match mapping.get(mem) {
                Some(new_index) => {
                    *mem = *new_index;
                }
                None => panic!("Attempting to reference a deleted memory, ID: {}", mem),
            }
        }
        Operator::MemoryCopy {src_mem, dst_mem} => {
            match mapping.get(src_mem) {
                Some(new_index) => {
                    *src_mem = *new_index;
                }
                None => panic!("Attempting to reference a deleted memory, ID: {}", src_mem),
            }
            match mapping.get(dst_mem) {
                Some(new_index) => {
                    *dst_mem = *new_index;
                }
                None => panic!("Attempting to reference a deleted memory, ID: {}", dst_mem),
            }
        }
        _ => panic!("Operation doesn't need to be checked for memory IDs!"),
    }
}

pub(crate) fn do_reencode<I, O>(
    i: I,
    reencode: fn(&mut RoundtripReencoder, I) -> Result<O, wasm_encoder::reencode::Error>,
    inst: &mut RoundtripReencoder,
    msg: &str,
) -> O {
    match reencode(inst, i) {
        Ok(o) => o,
        Err(e) => panic!("Couldn't encode {} due to error: {}", msg, e),
    }
}
