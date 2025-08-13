//! Intermediate representation of sections in a wasm module.

use log::error;
use std::borrow::Cow;
use std::cmp::PartialEq;
use std::collections::HashMap;
use std::fmt::Formatter;
use std::fmt::{self};
use std::mem::discriminant;
use std::slice::Iter;
use wasm_encoder::reencode::Reencode;
use wasm_encoder::{AbstractHeapType, Encode, Ieee32, Ieee64};

use wasmparser::types::TypeIdentifier;
use wasmparser::{ConstExpr, HeapType, Operator, RefType, ValType};

use crate::error::Error;
use crate::ir::id::{CustomSectionID, FunctionID, GlobalID, ModuleID, TypeID};
use crate::ir::module::side_effects::{InjectType, Injection};
use crate::ir::module::{add_injection, fix_op_id_mapping};

type Result<T> = std::result::Result<T, Error>;

/// An optional tag that flags items that have been added to the module.
/// It can also carry some bytes of information the explain why it was added.
/// For injected items it will be Some(_), for items already in the module, it will be None.
pub type InjectTag = Option<Tag>;

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct Tag {
    data: Vec<u8>,
}
impl Tag {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
    pub fn data_mut(&mut self) -> &mut Vec<u8> {
        &mut self.data
    }
    pub fn data(&self) -> &Vec<u8> {
        &self.data
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
pub(crate) trait TagUtils {
    fn get_or_create_tag(&mut self) -> &mut Tag;
    fn get_tag(&self) -> &Option<Tag>;
}
// Override the default private_bounds warning since I don't want the TagUtils trait to be public
#[allow(private_bounds)]
pub trait HasInjectTag: TagUtils {
    fn append_to_tag(&mut self, mut data: Vec<u8>) {
        self.get_or_create_tag().data.append(&mut data);
    }
}

/// Wirm's Datatype. Combination of multiple [`wasmparser`] datatypes.
///
/// [ValType]: https://docs.rs/wasmparser/latest/wasmparser/enum.ValType.html
#[derive(Debug, Clone, Eq, Hash, PartialEq, Copy)]
pub enum DataType {
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    V128,
    FuncRef,
    FuncRefNull,
    ExternRef,
    ExternRefNull,
    Any,
    AnyNull,
    None,
    NoneNull,
    NoExtern,
    NoExternNull,
    NoFunc,
    NoFuncNull,
    Eq,
    EqNull,
    Struct,
    StructNull,
    Array,
    ArrayNull,
    I31,
    I31Null,
    Exn,
    NoExn,
    Module { ty_id: u32, nullable: bool },
    RecGroup(u32),
    CoreTypeId(u32), // TODO: Look at this
    Cont,
    NoCont,
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            DataType::I8 => write!(f, "DataType: I8"),
            DataType::I16 => write!(f, "DataType: I16"),
            DataType::I32 => write!(f, "DataType: I32"),
            DataType::I64 => write!(f, "DataType: I64"),
            DataType::F32 => write!(f, "DataType: F32"),
            DataType::F64 => write!(f, "DataType: F64"),
            DataType::V128 => write!(f, "DataType: V128"),
            DataType::FuncRef => write!(f, "DataType: FuncRef"),
            DataType::ExternRef => write!(f, "DataType: ExternRef"),
            DataType::Any => write!(f, "DataType: Any"),
            DataType::None => write!(f, "DataType: None"),
            DataType::NoExtern => write!(f, "DataType: NoExtern"),
            DataType::NoFunc => write!(f, "DataType: NoFunc"),
            DataType::Eq => write!(f, "DataType: Eq"),
            DataType::Struct => write!(f, "DataType: Struct"),
            DataType::Array => write!(f, "DataType: Array"),
            DataType::I31 => write!(f, "DataType: I31"),
            DataType::Exn => write!(f, "DataType: Exn"),
            DataType::NoExn => write!(f, "DataType: NoExn"),
            DataType::Module { ty_id, nullable } => {
                write!(f, "module: {ty_id}, nullable: {nullable}",)
            }
            DataType::RecGroup(idx) => write!(f, "recgroup: {idx}"),
            DataType::CoreTypeId(idx) => write!(f, "coretypeid: {idx}"),
            DataType::Cont => write!(f, "cont"),
            DataType::NoCont => write!(f, "nocont"),
            DataType::FuncRefNull => write!(f, "funcref: null"),
            DataType::ExternRefNull => write!(f, "externref: null"),
            DataType::AnyNull => write!(f, "any: null"),
            DataType::NoneNull => write!(f, "none: null"),
            DataType::NoFuncNull => write!(f, "nofunc: null"),
            DataType::NoExternNull => write!(f, "noextern: null"),
            DataType::EqNull => write!(f, "eq: null"),
            DataType::StructNull => write!(f, "struct: null"),
            DataType::ArrayNull => write!(f, "array: null"),
            DataType::I31Null => write!(f, "i31: null"),
        }
    }
}

impl From<wasmparser::StorageType> for DataType {
    fn from(value: wasmparser::StorageType) -> Self {
        match value {
            wasmparser::StorageType::I8 => DataType::I8,
            wasmparser::StorageType::I16 => DataType::I16,
            wasmparser::StorageType::Val(val) => DataType::from(val),
        }
    }
}

impl From<DataType> for wasm_encoder::StorageType {
    fn from(value: DataType) -> Self {
        match value {
            DataType::I8 => wasm_encoder::StorageType::I8,
            DataType::I16 => wasm_encoder::StorageType::I16,
            _ => wasm_encoder::StorageType::Val(wasm_encoder::ValType::from(&value)),
        }
    }
}

impl From<ValType> for DataType {
    fn from(value: ValType) -> Self {
        match value {
            ValType::I32 => DataType::I32,
            ValType::I64 => DataType::I64,
            ValType::F32 => DataType::F32,
            ValType::F64 => DataType::F64,
            ValType::V128 => DataType::V128,
            ValType::Ref(ref_type) => match ref_type.heap_type() {
                HeapType::Abstract { shared: _, ty } => match ty {
                    wasmparser::AbstractHeapType::Func => {
                        if ref_type.is_nullable() {
                            DataType::FuncRefNull
                        } else {
                            DataType::FuncRef
                        }
                    }
                    wasmparser::AbstractHeapType::Extern => {
                        if ref_type.is_nullable() {
                            DataType::ExternRefNull
                        } else {
                            DataType::ExternRef
                        }
                    }
                    wasmparser::AbstractHeapType::Any => {
                        if ref_type.is_nullable() {
                            DataType::AnyNull
                        } else {
                            DataType::Any
                        }
                    }
                    wasmparser::AbstractHeapType::None => {
                        if ref_type.is_nullable() {
                            DataType::NoneNull
                        } else {
                            DataType::None
                        }
                    }
                    wasmparser::AbstractHeapType::NoExtern => {
                        if ref_type.is_nullable() {
                            DataType::NoExternNull
                        } else {
                            DataType::NoExtern
                        }
                    }
                    wasmparser::AbstractHeapType::NoFunc => {
                        if ref_type.is_nullable() {
                            DataType::NoFuncNull
                        } else {
                            DataType::NoFunc
                        }
                    }
                    wasmparser::AbstractHeapType::Eq => {
                        if ref_type.is_nullable() {
                            DataType::EqNull
                        } else {
                            DataType::Eq
                        }
                    }
                    wasmparser::AbstractHeapType::Struct => {
                        if ref_type.is_nullable() {
                            DataType::StructNull
                        } else {
                            DataType::Struct
                        }
                    }
                    wasmparser::AbstractHeapType::Array => {
                        if ref_type.is_nullable() {
                            DataType::ArrayNull
                        } else {
                            DataType::Array
                        }
                    }
                    wasmparser::AbstractHeapType::I31 => {
                        if ref_type.is_nullable() {
                            DataType::I31Null
                        } else {
                            DataType::I31
                        }
                    }
                    wasmparser::AbstractHeapType::Exn => DataType::Exn,
                    wasmparser::AbstractHeapType::NoExn => DataType::NoExn,
                    wasmparser::AbstractHeapType::Cont => DataType::Cont,
                    wasmparser::AbstractHeapType::NoCont => DataType::NoCont,
                },
                HeapType::Concrete(u) => match u {
                    wasmparser::UnpackedIndex::Module(idx) => DataType::Module {
                        ty_id: *ModuleID(idx),
                        nullable: ref_type.is_nullable(),
                    },
                    wasmparser::UnpackedIndex::RecGroup(idx) => DataType::RecGroup(idx),
                    wasmparser::UnpackedIndex::Id(_id) => panic!("Not supported yet!"),
                },
            },
        }
    }
}

/// Converts from Wirm's DataType to [`wasm_encoder::ValType`].
///
/// [`wasm_encoder::ValType`]: https://docs.rs/wasm-encoder/0.214.0/wasm_encoder/enum.ValType.html
impl From<&DataType> for wasm_encoder::ValType {
    fn from(ty: &DataType) -> Self {
        match ty {
            DataType::I8 | DataType::I16 => panic!("Not valtype equivalent!"),
            DataType::I32 => wasm_encoder::ValType::I32,
            DataType::I64 => wasm_encoder::ValType::I64,
            DataType::F32 => wasm_encoder::ValType::F32,
            DataType::F64 => wasm_encoder::ValType::F64,
            DataType::V128 => wasm_encoder::ValType::V128,
            DataType::FuncRef => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Func,
                },
            }),
            DataType::ExternRef => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Extern,
                },
            }),
            DataType::Any => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Any,
                },
            }),
            DataType::NoneNull => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: true,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::None,
                },
            }),
            DataType::None => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::None,
                },
            }),
            DataType::NoExtern => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::NoExtern,
                },
            }),
            DataType::NoExternNull => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: true,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::NoExtern,
                },
            }),
            DataType::NoFunc => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::NoFunc,
                },
            }),
            DataType::NoFuncNull => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: true,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::NoFunc,
                },
            }),
            DataType::Eq => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Eq,
                },
            }),
            DataType::Struct => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Struct,
                },
            }),
            DataType::Array => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Array,
                },
            }),
            DataType::I31 => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::I31,
                },
            }),
            DataType::Exn => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Exn,
                },
            }),
            DataType::NoExn => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::NoExn,
                },
            }),
            DataType::Module { ty_id, nullable } => {
                wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                    nullable: *nullable,
                    heap_type: wasm_encoder::HeapType::Concrete(*ty_id),
                })
            }
            DataType::RecGroup(idx) => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Concrete(*idx),
            }),
            DataType::CoreTypeId(idx) => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Concrete(*idx),
            }),
            DataType::Cont => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Cont,
                },
            }),
            DataType::NoCont => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::NoCont,
                },
            }),
            DataType::FuncRefNull => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: true,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Func,
                },
            }),
            DataType::ExternRefNull => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: true,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Extern,
                },
            }),
            DataType::AnyNull => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: true,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Any,
                },
            }),
            DataType::EqNull => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: true,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Eq,
                },
            }),
            DataType::StructNull => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: true,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Struct,
                },
            }),
            DataType::ArrayNull => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: true,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Array,
                },
            }),
            DataType::I31Null => wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                nullable: true,
                heap_type: wasm_encoder::HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::I31,
                },
            }),
        }
    }
}

impl From<&DataType> for ValType {
    fn from(ty: &DataType) -> Self {
        match ty {
            DataType::I8 | DataType::I16 => panic!("No valtype equivalent!"),
            DataType::I32 => ValType::I32,
            DataType::I64 => ValType::I64,
            DataType::F32 => ValType::F32,
            DataType::F64 => ValType::F64,
            DataType::V128 => ValType::V128,
            DataType::FuncRef => ValType::FUNCREF,
            DataType::ExternRef => ValType::EXTERNREF,
            DataType::Any => ValType::Ref(
                RefType::new(
                    false,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::Any,
                    },
                )
                .unwrap(),
            ),
            DataType::None => ValType::Ref(
                RefType::new(
                    false,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::None,
                    },
                )
                .unwrap(),
            ),
            DataType::NoneNull => ValType::Ref(
                RefType::new(
                    true,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::None,
                    },
                )
                .unwrap(),
            ),
            DataType::NoExtern => ValType::Ref(
                RefType::new(
                    false,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::NoExtern,
                    },
                )
                .unwrap(),
            ),
            DataType::NoExternNull => ValType::Ref(
                RefType::new(
                    true,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::NoExtern,
                    },
                )
                .unwrap(),
            ),
            DataType::NoFunc => ValType::Ref(
                RefType::new(
                    false,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::NoFunc,
                    },
                )
                .unwrap(),
            ),
            DataType::NoFuncNull => ValType::Ref(
                RefType::new(
                    true,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::NoFunc,
                    },
                )
                .unwrap(),
            ),
            DataType::Eq => ValType::Ref(
                RefType::new(
                    false,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::Eq,
                    },
                )
                .unwrap(),
            ),
            DataType::Struct => ValType::Ref(
                RefType::new(
                    false,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::Struct,
                    },
                )
                .unwrap(),
            ),
            DataType::Array => ValType::Ref(
                RefType::new(
                    false,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::Array,
                    },
                )
                .unwrap(),
            ),
            DataType::I31 => ValType::Ref(
                RefType::new(
                    false,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::I31,
                    },
                )
                .unwrap(),
            ),
            DataType::Exn => ValType::Ref(
                RefType::new(
                    false,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::Exn,
                    },
                )
                .unwrap(),
            ),
            DataType::NoExn => ValType::Ref(
                RefType::new(
                    false,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::NoExn,
                    },
                )
                .unwrap(),
            ),
            DataType::Module { ty_id, nullable } => ValType::Ref(
                RefType::new(
                    *nullable,
                    HeapType::Concrete(wasmparser::UnpackedIndex::Module(*ty_id)),
                )
                .unwrap(),
            ),
            DataType::RecGroup(idx) => ValType::Ref(
                RefType::new(
                    false,
                    HeapType::Concrete(wasmparser::UnpackedIndex::RecGroup(*idx)),
                )
                .unwrap(),
            ),
            DataType::CoreTypeId(_idx) => panic!("Not Supported Yet!"),
            DataType::Cont => ValType::Ref(
                RefType::new(
                    false,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::Cont,
                    },
                )
                .unwrap(),
            ),
            DataType::NoCont => ValType::Ref(
                RefType::new(
                    false,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::NoCont,
                    },
                )
                .unwrap(),
            ),
            DataType::FuncRefNull => ValType::Ref(
                RefType::new(
                    true,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::Func,
                    },
                )
                .unwrap(),
            ),
            DataType::ExternRefNull => ValType::Ref(
                RefType::new(
                    true,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::Extern,
                    },
                )
                .unwrap(),
            ),
            DataType::AnyNull => ValType::Ref(
                RefType::new(
                    true,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::Any,
                    },
                )
                .unwrap(),
            ),
            DataType::EqNull => ValType::Ref(
                RefType::new(
                    true,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::Eq,
                    },
                )
                .unwrap(),
            ),
            DataType::StructNull => ValType::Ref(
                RefType::new(
                    true,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::Struct,
                    },
                )
                .unwrap(),
            ),
            DataType::ArrayNull => ValType::Ref(
                RefType::new(
                    true,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::Array,
                    },
                )
                .unwrap(),
            ),
            DataType::I31Null => ValType::Ref(
                RefType::new(
                    true,
                    HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::I31,
                    },
                )
                .unwrap(),
            ),
        }
    }
}

/// Converts [`ValType`] to [`wasm_encoder::ValType`].
///
/// [`wasm_encoder::ValType`]: https://docs.rs/wasm-encoder/0.214.0/wasm_encoder/enum.ValType.html
/// [`ValType`]: https://docs.rs/wasmparser/latest/wasmparser/enum.ValType.html
pub fn valtype_to_wasmencoder_type(val_type: &ValType) -> wasm_encoder::ValType {
    let mut reencoder = wasm_encoder::reencode::RoundtripReencoder;
    reencoder.val_type(*val_type).unwrap()
}

#[derive(Debug, Clone)]
/// Data Segment in a wasm module.
pub struct DataSegment {
    /// The kind of data segment.
    pub kind: DataSegmentKind,
    /// The data of the data segment.
    pub data: Vec<u8>,
    pub tag: InjectTag,
}

impl DataSegment {
    pub fn from_wasmparser(data: wasmparser::Data) -> Result<DataSegment> {
        Ok(DataSegment {
            kind: DataSegmentKind::from_wasmparser(data.kind)?,
            data: data.data.to_vec(),
            tag: None,
        })
    }
}
impl TagUtils for DataSegment {
    fn get_or_create_tag(&mut self) -> &mut Tag {
        self.tag.get_or_insert_default()
    }

    fn get_tag(&self) -> &Option<Tag> {
        &self.tag
    }
}

/// The kind of data segment.
#[derive(Debug, Clone)]
pub enum DataSegmentKind {
    /// The data segment is passive.
    Passive,
    /// The data segment is active.
    Active {
        /// The memory index for the data segment.
        memory_index: u32,
        /// The memory offset where this active data segment will be automatically
        /// initialized.
        offset_expr: InitExpr,
    },
}

impl DataSegmentKind {
    pub(crate) fn from_wasmparser(kind: wasmparser::DataKind) -> Result<DataSegmentKind> {
        Ok(match kind {
            wasmparser::DataKind::Passive => DataSegmentKind::Passive,
            wasmparser::DataKind::Active {
                memory_index,
                offset_expr,
            } => DataSegmentKind::Active {
                memory_index,
                offset_expr: InitExpr::eval(&offset_expr),
            },
        })
    }
}

#[derive(Debug, Clone)]
/// Kind of Element
pub enum ElementKind {
    Passive,
    Active {
        table_index: Option<u32>,
        offset_expr: InitExpr,
    },
    Declared,
}

impl ElementKind {
    pub(crate) fn from_wasmparser(kind: wasmparser::ElementKind) -> Result<ElementKind> {
        match kind {
            wasmparser::ElementKind::Passive => Ok(ElementKind::Passive),
            wasmparser::ElementKind::Declared => Ok(ElementKind::Declared),
            wasmparser::ElementKind::Active {
                table_index,
                offset_expr,
            } => Ok(ElementKind::Active {
                table_index,
                offset_expr: InitExpr::eval(&offset_expr),
            }),
        }
    }
}

#[derive(Debug, Clone)]
/// Type of element
pub enum ElementItems {
    Functions(Vec<FunctionID>),
    ConstExprs { ty: RefType, exprs: Vec<InitExpr> },
}

impl ElementItems {
    pub(crate) fn from_wasmparser(items: wasmparser::ElementItems) -> Result<ElementItems> {
        match items {
            wasmparser::ElementItems::Functions(reader) => {
                let functions = reader
                    .into_iter()
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                // unsure how to avoid a second iteration (cast while iterating above)
                let fids = functions.iter().map(|id| FunctionID(*id)).collect();
                Ok(ElementItems::Functions(fids))
            }
            wasmparser::ElementItems::Expressions(ref_type, reader) => {
                let exprs = reader
                    .into_iter()
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(ElementItems::ConstExprs {
                    ty: ref_type,
                    exprs: exprs.iter().map(|expr| InitExpr::eval(expr)).collect(),
                })
            }
        }
    }
}

#[derive(Debug, Clone)]
/// Mode of Function in case the function is mark as instrumented
pub enum FuncInstrMode {
    Entry,
    Exit,
}

#[derive(Default, Debug, Clone)]
/// Instrumentation Data that is stored with every function
pub struct FuncInstrFlag<'a> {
    /// boolean flag to say whether there are special instrumentation
    /// modes to resolve for this function (see InstrumentationMode variants)
    pub has_special_instr: bool,
    pub current_mode: Option<FuncInstrMode>,
    pub entry: InjectedInstrs<'a>,
    pub exit: InjectedInstrs<'a>,
}

impl TagUtils for FuncInstrFlag<'_> {
    fn get_or_create_tag(&mut self) -> &mut Tag {
        match self.current_mode {
            None => {
                panic!("Current mode is not set...cannot append to the tag!")
            }
            Some(FuncInstrMode::Entry) => self.entry.get_or_create_tag(),
            Some(FuncInstrMode::Exit) => self.exit.get_or_create_tag(),
        }
    }

    fn get_tag(&self) -> &Option<Tag> {
        match self.current_mode {
            None => {
                panic!("Current mode is not set...cannot append to the tag!")
            }
            Some(FuncInstrMode::Entry) => self.entry.get_tag(),
            Some(FuncInstrMode::Exit) => self.exit.get_tag(),
        }
    }
}
impl HasInjectTag for FuncInstrFlag<'_> {}
impl fmt::Display for FuncInstrFlag<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let FuncInstrFlag {
            has_special_instr,
            entry,
            exit,
            current_mode: _,
        } = self;
        if !self.has_instr() {
            write!(f, "Not Instrumented")?;
        }
        write!(
            f,
            "Has special instrumentation: {}\n \
             Func Entry: {:?} instructions\n \
             Func Exit: {:?} instructions",
            has_special_instr,
            entry.instrs.len(),
            exit.instrs.len()
        )
    }
}

impl PartialEq for FuncInstrFlag<'_> {
    fn eq(&self, other: &Self) -> bool {
        // Using pattern match to help identify when this function needs to be extended in the future
        let Self {
            has_special_instr,
            entry,
            exit,
            current_mode,
        } = self;
        let mut result = *has_special_instr == other.has_special_instr;
        result &= entry.eq(&other.entry);
        result &= exit.eq(&other.exit);
        result &= discriminant(current_mode) == discriminant(&other.current_mode);

        result
    }
}

impl Eq for FuncInstrFlag<'_> {}

impl<'a> FuncInstrFlag<'a> {
    pub fn has_instr(&self) -> bool {
        // Using pattern match to help identify when this function needs to be extended in the future
        let Self {
            entry,
            exit,
            has_special_instr: _,
            current_mode: _,
        } = self;
        !entry.instrs.is_empty() || !exit.instrs.is_empty()
    }

    pub fn has_special_instr(&self) -> bool {
        self.has_special_instr
    }

    /// Add an instruction to the current FuncInstrMode's list
    pub fn add_instr(&mut self, val: Operator<'a>) {
        self.has_special_instr = true;
        match self.current_mode {
            None => {
                panic!("Current mode is not set...cannot inject instructions!")
            }
            Some(FuncInstrMode::Entry) => self.entry.instrs.push(val),
            Some(FuncInstrMode::Exit) => self.exit.instrs.push(val),
        }
    }

    /// Get an instruction to the current FuncInstrMode's list
    pub fn get_instr(&self, idx: usize) -> &Operator {
        match self.current_mode {
            None => {
                panic!("Current mode is not set...cannot grab instruction without context!")
            }
            Some(FuncInstrMode::Entry) => self.entry.instrs.get(idx).unwrap(),
            Some(FuncInstrMode::Exit) => self.exit.instrs.get(idx).unwrap(),
        }
    }

    pub fn instr_len(&self) -> usize {
        match self.current_mode {
            None => {
                panic!("Current mode is not set...cannot grab instruction without context!")
            }
            Some(FuncInstrMode::Entry) => self.entry.instrs.len(),
            Some(FuncInstrMode::Exit) => self.exit.instrs.len(),
        }
    }

    pub fn add_injections(
        &mut self,
        fid: u32,
        func_mapping: &HashMap<u32, u32>,
        global_mapping: &HashMap<u32, u32>,
        memory_mapping: &HashMap<u32, u32>,
        side_effects: &mut HashMap<InjectType, Vec<Injection<'a>>>,
    ) {
        let Self { entry, exit, .. } = self;
        let mut add_inj = |mode: FuncInstrMode, instrs: &mut InjectedInstrs<'a>| {
            // Fix the ID mapping in each of the injected opcodes.
            for op in instrs.instrs.iter_mut() {
                fix_op_id_mapping(op, func_mapping, global_mapping, memory_mapping);
            }

            if instrs.instrs.is_empty() {
                return;
            }

            add_injection(
                side_effects,
                InjectType::Probe,
                Injection::FuncProbe {
                    target_fid: fid,
                    mode,
                    body: instrs.instrs.clone(),
                    tag: instrs.tag.clone().unwrap_or_default(),
                },
            );
        };

        add_inj(FuncInstrMode::Entry, entry);
        add_inj(FuncInstrMode::Exit, exit);
    }

    /// Can be called after finishing some instrumentation to reset the mode.
    pub fn finish_instr(&mut self) {
        self.current_mode = None
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
/// Mode of Instruction in case the instruction is marked as Instrumented
pub enum InstrumentationMode {
    Before,
    After,
    Alternate,

    // special modes
    SemanticAfter,
    BlockEntry,
    BlockExit,
    BlockAlt,
}

#[derive(Default, Debug, Clone)]
/// Instrumentation Data that is stored with every instruction
pub struct InstrumentationFlag<'a> {
    pub current_mode: Option<InstrumentationMode>,
    pub before: InjectedInstrs<'a>,
    pub after: InjectedInstrs<'a>,
    /// None means to replace with no instructions (effectively removing the original)
    /// Some(vec) means to replace with the vec of instructions
    /// Some(empty vec) means there is no alt instrumentation
    pub alternate: Option<InjectedInstrs<'a>>,

    // special modes
    pub semantic_after: InjectedInstrs<'a>,
    pub block_entry: InjectedInstrs<'a>,
    pub block_exit: InjectedInstrs<'a>,
    /// None means to replace with no instructions (effectively removing the original)
    /// Some(vec) means to replace with the vec of instructions
    /// Some(empty vec) means there is no alt instrumentation
    pub block_alt: Option<InjectedInstrs<'a>>,
}
impl TagUtils for InstrumentationFlag<'_> {
    fn get_or_create_tag(&mut self) -> &mut Tag {
        match self.current_mode {
            None => {
                panic!("Current mode is not set...cannot get the tag!")
            }
            Some(InstrumentationMode::Before) => self.before.get_or_create_tag(),
            Some(InstrumentationMode::After) => self.after.get_or_create_tag(),
            Some(InstrumentationMode::Alternate) => {
                self.alternate.get_or_insert_default().get_or_create_tag()
            }
            Some(InstrumentationMode::SemanticAfter) => self.semantic_after.get_or_create_tag(),
            Some(InstrumentationMode::BlockEntry) => self.block_entry.get_or_create_tag(),
            Some(InstrumentationMode::BlockExit) => self.block_exit.get_or_create_tag(),
            Some(InstrumentationMode::BlockAlt) => {
                self.block_alt.get_or_insert_default().get_or_create_tag()
            }
        }
    }

    fn get_tag(&self) -> &Option<Tag> {
        match self.current_mode {
            None => {
                panic!("Current mode is not set...cannot get the tag!")
            }
            Some(InstrumentationMode::Before) => self.before.get_tag(),
            Some(InstrumentationMode::After) => self.after.get_tag(),
            Some(InstrumentationMode::Alternate) => {
                if let Some(alt) = &self.alternate {
                    alt.get_tag()
                } else {
                    &None
                }
            }
            Some(InstrumentationMode::SemanticAfter) => self.semantic_after.get_tag(),
            Some(InstrumentationMode::BlockEntry) => self.block_entry.get_tag(),
            Some(InstrumentationMode::BlockExit) => self.block_exit.get_tag(),
            Some(InstrumentationMode::BlockAlt) => {
                if let Some(alt) = &self.block_alt {
                    alt.get_tag()
                } else {
                    &None
                }
            }
        }
    }
}
impl HasInjectTag for InstrumentationFlag<'_> {}

impl fmt::Display for InstrumentationFlag<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let InstrumentationFlag {
            before,
            after,
            alternate,
            semantic_after,
            block_entry,
            block_exit,
            block_alt,
            current_mode: _,
        } = self;
        if !self.has_instr() {
            write!(f, "Not Instrumented")?;
        }
        write!(
            f,
            "Before: {:?} instructions\n \
                   After: {:?} instructions\n \
                   Alternate: {:?} instructions\n \
                   Semantic After: {:?} instructions\n \
                   Block Entry: {:?} instructions\n \
                   Block Exit: {:?} instructions\n \
                   Block Alt: {:?} instructions",
            before.instrs.len(),
            after.instrs.len(),
            alternate.as_ref().unwrap().instrs.len(),
            semantic_after.instrs.len(),
            block_entry.instrs.len(),
            block_exit.instrs.len(),
            block_alt.as_ref().unwrap().instrs.len()
        )
    }
}

impl PartialEq for InstrumentationFlag<'_> {
    fn eq(&self, other: &Self) -> bool {
        // Using pattern match to help identify when this function needs to be extended in the future
        let Self {
            before,
            after,
            alternate,
            semantic_after,
            block_entry,
            block_exit,
            block_alt,
            current_mode,
        } = self;
        let mut result = before.eq(&other.before);
        result &= after.eq(&other.after);
        result &= *alternate == other.alternate;
        result &= semantic_after.eq(&other.semantic_after);
        result &= block_entry.eq(&other.block_entry);
        result &= block_exit.eq(&other.block_exit);
        result &= block_alt.eq(&other.block_alt);
        result &= *current_mode == other.current_mode;

        result
    }
}

impl Eq for InstrumentationFlag<'_> {}

impl<'a> InstrumentationFlag<'a> {
    pub fn has_instr(&self) -> bool {
        // Using pattern match to help identify when this function needs to be extended in the future
        let Self {
            before,
            after,
            alternate,
            semantic_after,
            block_entry,
            block_exit,
            block_alt,
            current_mode: _,
        } = self;
        !before.instrs.is_empty()
            || !after.instrs.is_empty()
            || !alternate.is_none() // Some(vec![]) means instruction removal!
            || !semantic_after.instrs.is_empty()
            || !block_entry.instrs.is_empty()
            || !block_exit.instrs.is_empty()
            || !block_alt.is_none() // Some(vec![]) means block removal!
    }

    pub(crate) fn check_special_is_resolved(&self) {
        let Self {
            semantic_after,
            block_entry,
            block_exit,
            block_alt,
            ..
        } = self;

        // Check if special instrumentation modes have been resolved!
        if !semantic_after.instrs.is_empty() {
            error!(
                "BUG: Semantic after instrumentation should be resolved already, please report."
            );
        }
        if !block_entry.instrs.is_empty() {
            error!("BUG: Block entry instrumentation should be resolved already, please report.");
        }
        if !block_exit.instrs.is_empty() {
            error!("BUG: Block exit instrumentation should be resolved already, please report.");
        }
        if !block_alt.is_none() {
            error!("BUG: Block alt instrumentation should be resolved already, please report.");
        }
    }

    pub(crate) fn add_injections(
        &self,
        fid: u32,
        idx: u32,
        side_effects: &mut HashMap<InjectType, Vec<Injection<'a>>>,
    ) {
        let Self {
            before,
            after,
            alternate,
            ..
        } = self;

        let mut add_inj = |mode: InstrumentationMode, instrs: &InjectedInstrs<'a>| {
            if instrs.instrs.is_empty() {
                return;
            }
            add_injection(
                side_effects,
                InjectType::Probe,
                Injection::FuncLocProbe {
                    target_fid: fid,
                    target_opcode_idx: idx,
                    mode,
                    body: instrs.instrs.clone(),
                    tag: instrs.tag.clone().unwrap_or_default(),
                },
            );
        };

        add_inj(InstrumentationMode::Before, before);
        add_inj(InstrumentationMode::After, after);
        if let Some(alt) = alternate {
            add_inj(InstrumentationMode::Alternate, alt);
        }
    }

    /// Add an instruction to the current InstrumentationMode's list
    /// Returns whether the instrumentation was a 'special' mode
    pub fn add_instr(&mut self, op: &Operator, val: Operator<'a>) -> bool {
        match self.current_mode {
            None => {
                panic!("Current mode is not set...cannot inject instructions!")
            }
            Some(InstrumentationMode::Before) => {
                self.before.instrs.push(val);
                false
            }
            Some(InstrumentationMode::After) => {
                self.after.instrs.push(val);
                false
            }
            Some(InstrumentationMode::Alternate) => {
                match &mut self.alternate {
                    None => {
                        self.alternate = Some(InjectedInstrs {
                            instrs: vec![val],
                            tag: None,
                        })
                    }
                    Some(alternate) => alternate.instrs.push(val),
                }
                false
            }
            Some(InstrumentationMode::SemanticAfter) => {
                if Self::is_block_style_op(op) || Self::is_branching_op(op) {
                    self.semantic_after.instrs.push(val);
                    true
                } else {
                    // instrumentation type not applicable!
                    panic!(
                        "Cannot apply semantic after instrumentation mode to op type: {:?}",
                        op
                    );
                }
            }
            Some(InstrumentationMode::BlockEntry) => {
                if Self::is_block_style_op(op) {
                    self.block_entry.instrs.push(val);
                    true
                } else {
                    // instrumentation type not applicable!
                    panic!(
                        "Cannot apply block entry instrumentation mode to op type: {:?}",
                        op
                    );
                }
            }
            Some(InstrumentationMode::BlockExit) => {
                if Self::is_block_style_op(op) {
                    self.block_exit.instrs.push(val);
                    true
                } else {
                    // instrumentation type not applicable!
                    panic!(
                        "Cannot apply block exit instrumentation mode to op type: {:?}",
                        op
                    );
                }
            }
            Some(InstrumentationMode::BlockAlt) => {
                if Self::is_block_style_op(op) {
                    match &mut self.block_alt {
                        None => {
                            self.block_alt = Some(InjectedInstrs {
                                instrs: vec![val],
                                tag: None,
                            })
                        }
                        Some(block_alt) => block_alt.instrs.push(val),
                    }
                    true
                } else {
                    // instrumentation type not applicable!
                    panic!(
                        "Cannot apply block alternate instrumentation mode to op type: {:?}",
                        op
                    );
                }
            }
        }
    }

    pub fn instr_len(&self) -> usize {
        match self.current_mode {
            None => {
                panic!("Current mode is not set...cannot inject instructions!")
            }
            Some(InstrumentationMode::Before) => self.before.instrs.len(),
            Some(InstrumentationMode::After) => self.after.instrs.len(),
            Some(InstrumentationMode::Alternate) => match &self.alternate {
                None => 0,
                Some(alternate) => alternate.instrs.len(),
            },
            Some(InstrumentationMode::SemanticAfter) => self.semantic_after.instrs.len(),
            Some(InstrumentationMode::BlockEntry) => self.block_entry.instrs.len(),
            Some(InstrumentationMode::BlockExit) => self.block_exit.instrs.len(),
            Some(InstrumentationMode::BlockAlt) => match &self.block_alt {
                None => 0,
                Some(block_alt) => block_alt.instrs.len(),
            },
        }
    }

    pub fn clear_instr(&mut self, mode: InstrumentationMode) {
        match mode {
            InstrumentationMode::Before => {
                self.before.instrs.clear();
            }
            InstrumentationMode::After => self.after.instrs.clear(),
            InstrumentationMode::Alternate => {
                self.alternate = None;
            }
            InstrumentationMode::SemanticAfter => self.semantic_after.instrs.clear(),
            InstrumentationMode::BlockEntry => self.block_entry.instrs.clear(),
            InstrumentationMode::BlockExit => self.block_exit.instrs.clear(),
            InstrumentationMode::BlockAlt => {
                self.block_alt = None;
            }
        }
    }

    fn is_block_style_op(op: &Operator) -> bool {
        matches!(
            op,
            Operator::Block { .. }
                | Operator::Loop { .. }
                | Operator::If { .. }
                | Operator::Else { .. }
        )
    }

    fn is_branching_op(op: &Operator) -> bool {
        matches!(
            op,
            Operator::Br { .. }
                | Operator::BrIf { .. }
                | Operator::BrTable { .. }
                | Operator::BrOnCast { .. }
                | Operator::BrOnCastFail { .. }
                | Operator::BrOnNull { .. }
                | Operator::BrOnNonNull { .. }
        )
    }

    /// Get an instruction to the current InstrumentationMode's list
    pub fn get_instr(&self, idx: usize) -> &Operator {
        match self.current_mode {
            None => {
                panic!("Current mode is not set...cannot grab instruction without context!")
            }
            Some(InstrumentationMode::Before) => self.before.instrs.get(idx).unwrap(),
            Some(InstrumentationMode::After) => self.after.instrs.get(idx).unwrap(),
            Some(InstrumentationMode::Alternate) => match &self.alternate {
                None => panic!("No alternate instructions to pull idx '{}' from", idx),
                Some(alternate) => alternate.instrs.get(idx).unwrap(),
            },
            Some(InstrumentationMode::SemanticAfter) => {
                self.semantic_after.instrs.get(idx).unwrap()
            }
            Some(InstrumentationMode::BlockEntry) => self.block_entry.instrs.get(idx).unwrap(),
            Some(InstrumentationMode::BlockExit) => self.block_exit.instrs.get(idx).unwrap(),
            Some(InstrumentationMode::BlockAlt) => match &self.block_alt {
                None => panic!("No block alt instructions to pull idx '{}' from", idx),
                Some(block_alt) => block_alt.instrs.get(idx).unwrap(),
            },
        }
    }

    /// Can be called after finishing some instrumentation to reset the mode.
    pub fn finish_instr(&mut self) {
        self.current_mode = None
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InjectedInstrs<'a> {
    pub(crate) instrs: Vec<Operator<'a>>,
    pub(crate) tag: InjectTag,
}
impl TagUtils for InjectedInstrs<'_> {
    fn get_or_create_tag(&mut self) -> &mut Tag {
        self.tag.get_or_insert_default()
    }

    fn get_tag(&self) -> &Option<Tag> {
        &self.tag
    }
}
impl HasInjectTag for InjectedInstrs<'_> {}

/// Used to represent a unique location in a wasm component or module.
#[derive(Debug, Clone, Copy)]
pub enum Location {
    Component {
        mod_idx: ModuleID,
        func_idx: FunctionID,
        instr_idx: usize,
    },
    Module {
        func_idx: FunctionID,
        instr_idx: usize,
    },
}

#[derive(Debug, Default, Clone)]
/// Body of a function in a wasm module
pub struct Body<'a> {
    /// Local variables of the function, given as tuples of (# of locals, type).
    /// Note that these do not include the function parameters which are given
    /// indices before the locals. So if a function has 2 parameters and a local
    /// defined here then local indices 0 and 1 will refer to the parameters and
    /// index 2 will refer to the local here.
    pub locals: Vec<(u32, DataType)>,
    pub num_locals: u32,
    // accessing operators by .0 is not very clear
    pub instructions: Vec<Instruction<'a>>,
    pub num_instructions: usize,
    pub name: Option<String>,
}

// 'b should outlive 'a
impl<'a, 'b> Body<'a>
where
    'b: 'a,
{
    /// Push a new operator (instruction) to the end of the body
    pub fn push_op(&mut self, op: Operator<'b>) {
        self.instructions.push(Instruction::new(op));
        self.num_instructions += 1;
    }

    /// Get some operator (instruction) at the specified index of the body
    pub fn get_op(&self, idx: usize) -> &Operator {
        &self.instructions[idx].op
    }

    /// Get the instrumentation of some operator in the body
    pub fn get_instr_flag(&self, idx: usize) -> &InstrumentationFlag {
        &self.instructions[idx].instr_flag
    }

    /// Get the instrumentation of some operator in the body
    pub fn clear_instr(&mut self, idx: usize, mode: InstrumentationMode) {
        self.instructions[idx].instr_flag.clear_instr(mode);
    }

    /// Push an end operator (instruction) to the end of the body
    pub fn end(&mut self) {
        self.push_op(Operator::End);
    }

    pub fn locals_as_vec(&self) -> Vec<DataType> {
        let mut locals = vec![];
        for (count, ty) in self.locals.iter() {
            for _ in 0..*count {
                locals.push(*ty);
            }
        }
        locals
    }
}

#[derive(Debug, Clone)]
pub struct Instruction<'a> {
    pub op: Operator<'a>,
    pub instr_flag: InstrumentationFlag<'a>,
}
impl<'a, 'b> Instruction<'a>
where
    'b: 'a,
{
    pub fn new(op: Operator<'b>) -> Self {
        Self {
            op,
            instr_flag: InstrumentationFlag::default(),
        }
    }

    pub fn add_instr(&mut self, val: Operator<'a>) -> bool {
        self.instr_flag.add_instr(&self.op, val)
    }

    pub fn instr_len(&self) -> usize {
        self.instr_flag.instr_len()
    }

    pub fn extract_op(&'a self) -> Operator<'a> {
        self.op.clone()
    }
}

/// A constant expression which is produced in WebAssembly, typically used in global
/// initializers or element/data offsets.
#[derive(Debug, Clone)]
pub struct InitExpr {
    pub exprs: Vec<InitInstr>,
}

/// Set of instructions that can be used in Initialization Expressions
#[derive(Debug, Copy, Clone)]
pub enum InitInstr {
    /// An immediate constant value
    Value(Value),
    /// A constant value referenced by the global specified
    Global(GlobalID),
    /// A null reference
    RefNull(RefType),
    /// A function initializer
    RefFunc(FunctionID),
    /// Struct Initializer
    StructNew(TypeID),
    /// Struct Default
    StructNewDefault(TypeID),
    /// Array Initializer
    ArrayNew(TypeID),
    /// Default Initialisation
    ArrayNewDefault(TypeID),
    /// Fixed Array
    RefArrayFixed {
        array_type_index: u32,
        array_size: u32,
    },
    /// Array from Data
    RefArrayData {
        array_type_index: u32,
        array_data_index: u32,
    },
    /// Array from Elem
    RefArrayElem {
        array_type_index: u32,
        array_elem_index: u32,
    },
    RefI31,
}
impl InitInstr {
    pub fn fix_id_mapping(
        &mut self,
        func_mapping: &HashMap<u32, u32>,
        global_mapping: &HashMap<u32, u32>,
    ) {
        match self {
            InitInstr::Global(id) => match global_mapping.get(&(*id)) {
                Some(new_index) => {
                    **id = *new_index;
                }
                None => panic!("Deleted global!"),
            },
            InitInstr::RefFunc(id) => match func_mapping.get(&(*id)) {
                Some(new_index) => {
                    **id = *new_index;
                }
                None => panic!("Deleted function!"),
            },
            _ => {}
        }
    }
}

impl InitExpr {
    /// Create a new initialisation expression given a vector of Instructions
    pub fn new(instructions: Vec<InitInstr>) -> Self {
        InitExpr {
            exprs: instructions,
        }
    }

    pub fn instructions(&self) -> &[InitInstr] {
        self.exprs.as_slice()
    }

    pub(crate) fn eval(init: &ConstExpr) -> InitExpr {
        use wasmparser::Operator::*;
        let mut reader = init.get_operators_reader();
        let mut instrs = vec![];
        loop {
            let val = match reader.read().unwrap() {
                I32Const { value } => InitInstr::Value(Value::I32(value)),
                I64Const { value } => InitInstr::Value(Value::I64(value)),
                F32Const { value } => InitInstr::Value(Value::F32(f32::from_bits(value.bits()))),
                F64Const { value } => InitInstr::Value(Value::F64(f64::from_bits(value.bits()))),
                V128Const { value } => InitInstr::Value(Value::V128(v128_to_u128(&value))),
                GlobalGet { global_index } => InitInstr::Global(GlobalID(global_index)),
                // Marking nullable as true as it's a null reference
                RefNull { hty } => InitInstr::RefNull(RefType::new(true, hty).unwrap()),
                RefFunc { function_index } => InitInstr::RefFunc(FunctionID(function_index)),
                StructNew { struct_type_index } => InitInstr::StructNew(TypeID(struct_type_index)),
                StructNewDefault { struct_type_index } => {
                    InitInstr::StructNewDefault(TypeID(struct_type_index))
                }
                ArrayNew { array_type_index } => InitInstr::ArrayNew(TypeID(array_type_index)),
                ArrayNewDefault { array_type_index } => {
                    InitInstr::ArrayNewDefault(TypeID(array_type_index))
                }
                ArrayNewFixed {
                    array_type_index,
                    array_size,
                } => InitInstr::RefArrayFixed {
                    array_type_index,
                    array_size,
                },
                ArrayNewData {
                    array_type_index,
                    array_data_index,
                } => InitInstr::RefArrayData {
                    array_data_index,
                    array_type_index,
                },
                ArrayNewElem {
                    array_type_index,
                    array_elem_index,
                } => InitInstr::RefArrayElem {
                    array_type_index,
                    array_elem_index,
                },
                RefI31 => InitInstr::RefI31,
                End => break,
                _ => panic!("Invalid constant expression"),
            };
            instrs.push(val);
        }
        if !reader.eof() {
            panic!("There was more data after the function end!");
        }
        InitExpr { exprs: instrs }
    }

    pub(crate) fn to_wasmencoder_type(&self) -> wasm_encoder::ConstExpr {
        let mut bytes = vec![];
        for instr in self.exprs.iter() {
            match instr {
                InitInstr::Value(v) => match v {
                    Value::I32(v) => wasm_encoder::Instruction::I32Const(*v).encode(&mut bytes),
                    Value::I64(v) => wasm_encoder::Instruction::I64Const(*v).encode(&mut bytes),
                    Value::F32(v) => {
                        wasm_encoder::Instruction::F32Const(Ieee32::from(*v)).encode(&mut bytes)
                    }
                    Value::F64(v) => {
                        wasm_encoder::Instruction::F64Const(Ieee64::from(*v)).encode(&mut bytes)
                    }
                    Value::V128(v) => {
                        wasm_encoder::Instruction::V128Const(*v as i128).encode(&mut bytes)
                    }
                },
                InitInstr::Global(g) => {
                    wasm_encoder::Instruction::GlobalGet(**g).encode(&mut bytes)
                }
                InitInstr::RefNull(ty) => {
                    wasm_encoder::Instruction::RefNull(match ty.heap_type() {
                        HeapType::Abstract { shared, ty } => match ty {
                            wasmparser::AbstractHeapType::Func => {
                                wasm_encoder::HeapType::Abstract {
                                    ty: AbstractHeapType::Func,
                                    shared,
                                }
                            }
                            wasmparser::AbstractHeapType::Extern => {
                                wasm_encoder::HeapType::Abstract {
                                    ty: AbstractHeapType::Extern,
                                    shared,
                                }
                            }
                            wasmparser::AbstractHeapType::Any => wasm_encoder::HeapType::Abstract {
                                ty: AbstractHeapType::Any,
                                shared,
                            },
                            wasmparser::AbstractHeapType::None => {
                                wasm_encoder::HeapType::Abstract {
                                    ty: AbstractHeapType::None,
                                    shared,
                                }
                            }
                            wasmparser::AbstractHeapType::NoExtern => {
                                wasm_encoder::HeapType::Abstract {
                                    ty: AbstractHeapType::NoExtern,
                                    shared,
                                }
                            }
                            wasmparser::AbstractHeapType::NoFunc => {
                                wasm_encoder::HeapType::Abstract {
                                    ty: AbstractHeapType::NoFunc,
                                    shared,
                                }
                            }
                            wasmparser::AbstractHeapType::Eq => wasm_encoder::HeapType::Abstract {
                                ty: AbstractHeapType::Eq,
                                shared,
                            },
                            wasmparser::AbstractHeapType::Struct => {
                                wasm_encoder::HeapType::Abstract {
                                    ty: AbstractHeapType::Struct,
                                    shared,
                                }
                            }
                            wasmparser::AbstractHeapType::Array => {
                                wasm_encoder::HeapType::Abstract {
                                    ty: AbstractHeapType::Array,
                                    shared,
                                }
                            }
                            wasmparser::AbstractHeapType::I31 => wasm_encoder::HeapType::Abstract {
                                ty: AbstractHeapType::I31,
                                shared,
                            },
                            wasmparser::AbstractHeapType::Exn => wasm_encoder::HeapType::Abstract {
                                ty: AbstractHeapType::Exn,
                                shared,
                            },
                            wasmparser::AbstractHeapType::NoExn => {
                                wasm_encoder::HeapType::Abstract {
                                    ty: AbstractHeapType::NoExn,
                                    shared,
                                }
                            }
                            wasmparser::AbstractHeapType::Cont => {
                                wasm_encoder::HeapType::Abstract {
                                    ty: AbstractHeapType::Cont,
                                    shared,
                                }
                            }
                            wasmparser::AbstractHeapType::NoCont => {
                                wasm_encoder::HeapType::Abstract {
                                    ty: AbstractHeapType::NoCont,
                                    shared,
                                }
                            }
                        },
                        HeapType::Concrete(id) => {
                            if let Some(mod_id) = id.as_module_index() {
                                wasm_encoder::HeapType::Concrete(mod_id)
                            } else if let Some(rg_id) = id.as_rec_group_index() {
                                wasm_encoder::HeapType::Concrete(rg_id)
                            } else if let Some(core) = id.as_core_type_id() {
                                wasm_encoder::HeapType::Concrete(core.index() as u32)
                            } else {
                                panic!("Did not unpack concrete type!")
                            }
                        }
                    })
                    .encode(&mut bytes)
                }
                InitInstr::RefFunc(f) => wasm_encoder::Instruction::RefFunc(**f).encode(&mut bytes),
                InitInstr::StructNew(id) => {
                    wasm_encoder::Instruction::StructNew(**id).encode(&mut bytes);
                }
                InitInstr::ArrayNew(id) => {
                    wasm_encoder::Instruction::ArrayNew(**id).encode(&mut bytes);
                }

                InitInstr::StructNewDefault(id) => {
                    wasm_encoder::Instruction::StructNewDefault(**id).encode(&mut bytes);
                }
                InitInstr::ArrayNewDefault(id) => {
                    wasm_encoder::Instruction::ArrayNewDefault(**id).encode(&mut bytes);
                }
                InitInstr::RefArrayFixed {
                    array_type_index,
                    array_size,
                } => {
                    wasm_encoder::Instruction::ArrayNewFixed {
                        array_size: *array_size,
                        array_type_index: *array_type_index,
                    }
                    .encode(&mut bytes);
                }
                InitInstr::RefArrayData {
                    array_type_index,
                    array_data_index,
                } => {
                    wasm_encoder::Instruction::ArrayNewData {
                        array_data_index: *array_data_index,
                        array_type_index: *array_type_index,
                    }
                    .encode(&mut bytes);
                }
                InitInstr::RefArrayElem {
                    array_type_index,
                    array_elem_index,
                } => {
                    wasm_encoder::Instruction::ArrayNewElem {
                        array_elem_index: *array_elem_index,
                        array_type_index: *array_type_index,
                    }
                    .encode(&mut bytes);
                }
                InitInstr::RefI31 => wasm_encoder::Instruction::RefI31.encode(&mut bytes),
            }
        }
        wasm_encoder::ConstExpr::raw(bytes)
    }
}

/// Constant values that can show up in WebAssembly
#[derive(Debug, Clone, Copy)]
pub enum Value {
    /// A constant 32-bit integer
    I32(i32),
    /// A constant 64-bit integer
    I64(i64),
    /// A constant 32-bit float
    F32(f32),
    /// A constant 64-bit float
    F64(f64),
    /// A constant 128-bit vector register
    V128(u128),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Value::I32(i) => i.fmt(f),
            Value::I64(i) => i.fmt(f),
            Value::F32(i) => i.fmt(f),
            Value::F64(i) => i.fmt(f),
            Value::V128(i) => i.fmt(f),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BlockType {
    /// The block produces consumes nor produces any values.
    Empty,
    /// The block produces a singular value of the given type ([] -> \[t]).
    Type(DataType),
    /// The block is described by a function type.
    ///
    /// The index is to a function type in the types section.
    FuncType(TypeID),
}

impl From<wasmparser::BlockType> for BlockType {
    fn from(value: wasmparser::BlockType) -> Self {
        match value {
            wasmparser::BlockType::Empty => BlockType::Empty,
            wasmparser::BlockType::FuncType(u) => BlockType::FuncType(TypeID(u)),
            wasmparser::BlockType::Type(val) => BlockType::Type(DataType::from(val)),
        }
    }
}

impl From<BlockType> for wasmparser::BlockType {
    fn from(ty: BlockType) -> Self {
        match ty {
            BlockType::Empty => wasmparser::BlockType::Empty,
            BlockType::FuncType(u) => wasmparser::BlockType::FuncType(*u),
            BlockType::Type(data) => wasmparser::BlockType::Type(ValType::from(&data)),
        }
    }
}

/// Intermediate Representation of Custom Sections
#[derive(Clone, Debug, Default)]
pub struct CustomSections<'a> {
    custom_sections: Vec<CustomSection<'a>>,
}

impl<'a> CustomSections<'a> {
    pub fn new(custom_sections: Vec<(&'a str, &'a [u8])>) -> Self {
        CustomSections {
            custom_sections: custom_sections
                .iter()
                .map(|cs| CustomSection::new_borrowed(cs.0, cs.1))
                .collect(),
        }
    }

    /// Get a custom section ID by name
    pub fn get_id(&self, name: String) -> Option<CustomSectionID> {
        for (index, section) in self.custom_sections.iter().enumerate() {
            if section.name == name {
                return Some(CustomSectionID(index as u32));
            }
        }
        None
    }

    /// Get a custom section by its ID
    pub fn get_by_id(&self, custom_section_id: CustomSectionID) -> &CustomSection {
        if *custom_section_id < self.custom_sections.len() as u32 {
            return &self.custom_sections[*custom_section_id as usize];
        }
        panic!("Invalid custom section ID");
    }

    /// Delete a Custom Section by its ID
    pub fn delete(&mut self, id: CustomSectionID) {
        if *id < self.custom_sections.len() as u32 {
            self.custom_sections.remove(*id as usize);
        }
    }

    /// Number of custom sections
    pub fn len(&self) -> usize {
        self.custom_sections.len()
    }

    /// Check if there are any custom sections
    pub fn is_empty(&self) -> bool {
        self.custom_sections.is_empty()
    }

    /// Creates an iterable over the custom sections
    pub fn iter(&self) -> Iter<'_, CustomSection<'a>> {
        self.custom_sections.iter()
    }

    /// Get mutable reference to section data
    pub fn get_section_data_mut(&mut self, section_id: CustomSectionID) -> Option<&mut Vec<u8>> {
        if *section_id < self.custom_sections.len() as u32 {
            Some(self.custom_sections[*section_id as usize].data.to_mut())
        } else {
            None
        }
    }

    /// Add a new custom section and return its ID
    pub fn add(&mut self, section: CustomSection<'a>) -> CustomSectionID {
        let id = CustomSectionID(self.custom_sections.len() as u32);
        self.custom_sections.push(section);
        id
    }
}

/// Intermediate Representation of a single Custom Section
#[derive(Clone, Debug)]
pub struct CustomSection<'a> {
    pub name: &'a str,
    pub data: std::borrow::Cow<'a, [u8]>,
}

impl<'a> CustomSection<'a> {
    /// Create a new custom section
    pub fn new(name: &'a str, data: Vec<u8>) -> Self {
        CustomSection {
            name,
            data: Cow::Owned(data),
        }
    }

    /// Create a new custom section with borrowed data (private)
    fn new_borrowed(name: &'a str, data: &'a [u8]) -> Self {
        CustomSection {
            name,
            data: Cow::Borrowed(data),
        }
    }
}

#[allow(clippy::identity_op)]
pub(crate) fn v128_to_u128(value: &wasmparser::V128) -> u128 {
    let n = value.bytes();
    ((n[0] as u128) << 0)
        | ((n[1] as u128) << 8)
        | ((n[2] as u128) << 16)
        | ((n[3] as u128) << 24)
        | ((n[4] as u128) << 32)
        | ((n[5] as u128) << 40)
        | ((n[6] as u128) << 48)
        | ((n[7] as u128) << 56)
        | ((n[8] as u128) << 64)
        | ((n[9] as u128) << 72)
        | ((n[10] as u128) << 80)
        | ((n[11] as u128) << 88)
        | ((n[12] as u128) << 96)
        | ((n[13] as u128) << 104)
        | ((n[14] as u128) << 112)
        | ((n[15] as u128) << 120)
}
