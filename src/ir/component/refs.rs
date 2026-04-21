use crate::ir::component::idx_spaces::{IndexSpaceOf, Space};
use crate::ir::types::CustomSection;
use crate::Module;
use wasmparser::{
    CanonicalFunction, CanonicalOption, ComponentAlias, ComponentDefinedType, ComponentExport,
    ComponentFuncType, ComponentImport, ComponentInstance, ComponentInstantiationArg,
    ComponentStartFunction, ComponentType, ComponentTypeDeclaration, ComponentTypeRef,
    ComponentValType, CompositeInnerType, CompositeType, ContType, CoreType, Export, FieldType,
    Instance, InstanceTypeDeclaration, InstantiationArg, ModuleTypeDeclaration, RecGroup, RefType,
    StorageType, SubType, TagType, TypeBounds, TypeRef, ValType, VariantCase,
};

/// A trait for extracting all referenced indices from an IR node.
///
/// This provides a unified way to retrieve all semantic references
/// contained within a node, regardless of their specific role.
///
/// Implementations typically delegate to one or more of the
/// `Get*Refs` traits depending on the node's structure.
///
/// References contained within inner nested scopes are not included.
/// For example, the references inside the nested core type in
/// the following WAT will not be returned:
///   (type (;0;)           ;; << outer component type
///     (instance
///       (core type (;0;)  ;; << inner core type (nested scope)
///         (module
///           ( . . . )     ;; << nodes within core type that could hold refs
///         )
///       )
///     )
///   )
/// This design decision simplifies reasoning about reference resolution.
/// If such references are needed, reference lookup must be performed on
/// those inner IR nodes.
pub trait ReferencedIndices {
    /// Returns all referenced indices contained within this node.
    ///
    /// The returned [`RefKind`] values include both:
    ///
    /// - The referenced [`IndexedRef`]
    /// - The semantic role of the reference
    fn referenced_indices(&self) -> Vec<RefKind>;
}
/// Extracts references to `components` from a node. References within inner
/// scopes are not included, see [`ReferencedIndices`];
pub trait GetCompRefs {
    fn get_comp_refs(&self) -> Vec<RefKind>;
}
/// Extracts references to `modules` from a node. References within inner
/// scopes are not included, see [`ReferencedIndices`];
pub trait GetModuleRefs {
    fn get_module_refs(&self) -> Vec<RefKind>;
}
/// Extracts references to component OR core `types` from a node. References within inner
/// scopes are not included, see [`ReferencedIndices`];
pub trait GetTypeRefs {
    fn get_type_refs(&self) -> Vec<RefKind>;
}
/// Extracts references to component OR core `functions` from a node. References within inner
/// scopes are not included, see [`ReferencedIndices`];
pub trait GetFuncRefs {
    fn get_func_refs(&self) -> Vec<RefKind>;
}
/// Extracts the single reference to a component OR core `function` the node has. References within inner
/// scopes are not included, see [`ReferencedIndices`];
pub trait GetFuncRef {
    fn get_func_ref(&self) -> RefKind;
}
/// Extracts references to `memories` from a node. References within inner
/// scopes are not included, see [`ReferencedIndices`];
pub trait GetMemRefs {
    fn get_mem_refs(&self) -> Vec<RefKind>;
}
/// Extracts references to `tables` from a node. References within inner
/// scopes are not included, see [`ReferencedIndices`];
pub trait GetTableRefs {
    fn get_tbl_refs(&self) -> Vec<RefKind>;
}
/// Extracts references to any `item` from a node. References within inner
/// scopes are not included, see [`ReferencedIndices`];
pub trait GetItemRefs {
    fn get_item_refs(&self) -> Vec<RefKind>;
}
/// Extracts the single reference to an `item` that the node has. References within inner
/// scopes are not included, see [`ReferencedIndices`];
pub trait GetItemRef {
    fn get_item_ref(&self) -> RefKind;
}
/// Extracts references to `parameters` from a node. References within inner
/// scopes are not included, see [`ReferencedIndices`];
pub trait GetParamRefs {
    fn get_param_refs(&self) -> Vec<RefKind>;
}
/// Extracts references to `results` from a node. References within inner
/// scopes are not included, see [`ReferencedIndices`];
pub trait GetResultRefs {
    fn get_result_refs(&self) -> Vec<RefKind>;
}
/// Extracts references to `arguments` from a node. References within inner
/// scopes are not included, see [`ReferencedIndices`];
pub trait GetArgRefs {
    fn get_arg_refs(&self) -> Vec<RefKind>;
}
/// Extracts references to `descriptors` from a node. References within inner
/// scopes are not included, see [`ReferencedIndices`];
pub trait GetDescriptorRefs {
    fn get_descriptor_refs(&self) -> Vec<RefKind>;
}
/// Extracts references to `describes` from a node. References within inner
/// scopes are not included, see [`ReferencedIndices`];
pub trait GetDescribesRefs {
    fn get_describes_refs(&self) -> Vec<RefKind>;
}

/// Describes the semantic role of a referenced index.
///
/// This distinguishes *how* a referenced item is used within a node.
/// For example, a function index may be referenced as:
///
/// - A declaration
/// - A parameter
/// - A result
/// - A descriptor
///
/// The role is orthogonal to the index space itself.
#[derive(Clone, Copy)]
pub enum RefRole {
    Comp,
    Module,
    Inst,
    Func,
    Type,
    Val,
    Mem,
    Table,
    Global,
    Tag,

    /// A declaration at the given position.
    Decl(usize),
    /// A parameter at the given index.
    Param(usize),
    /// A result at the given index.
    Result(usize),
    /// An argument at the given index.
    Arg(usize),
    /// A `descriptor` reference.
    Descriptor,
    /// A `describes` reference.
    Describes,
}
impl RefRole {
    pub(crate) fn role_of(space: &Space) -> Self {
        match space {
            Space::Comp => Self::Comp,
            Space::CoreModule => Self::Module,
            Space::CompInst | Space::CoreInst => Self::Inst,
            Space::CompFunc | Space::CoreFunc => Self::Func,
            Space::CompType | Space::CoreType => Self::Type,
            Space::CompVal => Self::Val,
            Space::CoreMemory => Self::Mem,
            Space::CoreTable => Self::Table,
            Space::CoreGlobal => Self::Global,
            Space::CoreTag => Self::Tag,
            Space::NA => unreachable!(),
        }
    }
}

/// A single referenced index annotated with its semantic role.
///
/// This is the fundamental unit returned by `Get*Refs` and
/// `ReferencedIndices`.
///
/// A `RefKind` combines:
///
/// - The raw [`IndexedRef`] (depth + space + index)
/// - The semantic [`RefRole`] describing how it is used
#[derive(Clone, Copy)]
pub struct RefKind {
    pub role: RefRole,
    pub ref_: IndexedRef,
}
impl RefKind {
    pub(crate) fn new(ref_: IndexedRef) -> Self {
        let role = RefRole::role_of(&ref_.space);
        Self { role, ref_ }
    }
    pub(crate) fn decl(idx: usize, ref_: IndexedRef) -> Self {
        Self {
            role: RefRole::Decl(idx),
            ref_,
        }
    }
    pub(crate) fn param(idx: usize, ref_: IndexedRef) -> Self {
        Self {
            role: RefRole::Param(idx),
            ref_,
        }
    }
    pub(crate) fn result(idx: usize, ref_: IndexedRef) -> Self {
        Self {
            role: RefRole::Result(idx),
            ref_,
        }
    }
    pub(crate) fn descriptor(ref_: IndexedRef) -> Self {
        Self {
            role: RefRole::Descriptor,
            ref_,
        }
    }
    pub(crate) fn describes(ref_: IndexedRef) -> Self {
        Self {
            role: RefRole::Descriptor,
            ref_,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Depth(usize);
impl Depth {
    pub fn val(&self) -> usize {
        self.0
    }
    pub fn is_curr(&self) -> bool {
        self.0 == 0
    }
    pub fn outer(mut self) -> Self {
        self.0 += 1;
        self
    }
    pub fn outer_at(mut self, depth: u32) -> Self {
        self.0 += depth as usize;
        self
    }
    pub fn parent() -> Self {
        Self(1)
    }
}

/// A raw indexed reference into a specific index space.
///
/// This represents an unresolved reference that must be interpreted
/// relative to a [`crate::ir::component::visitor::VisitCtx`].
///
/// Fields:
///
/// - `depth` → Which scope the reference should be resolved in
/// - `space` → The index namespace (component, module, type, etc.)
/// - `index` → The numeric index within that namespace
///
/// Resolution is performed via [`crate::ir::component::visitor::VisitCtx::resolve`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct IndexedRef {
    /// The depth of the index space scope to look this up in.
    ///
    /// - `0` → current scope
    /// - Positive → outer scope(s)
    /// - Negative → inner scope(s)
    pub depth: Depth,
    /// The index namespace this reference belongs to.
    pub space: Space,
    /// The numeric index within the specified namespace.
    pub index: u32,
}

impl ReferencedIndices for Module<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        vec![]
    }
}

impl ReferencedIndices for ComponentType<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        refs.extend(self.get_type_refs());
        refs.extend(self.get_func_refs());
        refs.extend(self.get_param_refs());
        refs.extend(self.get_result_refs());

        refs
    }
}
impl GetTypeRefs for ComponentType<'_> {
    fn get_type_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        match self {
            ComponentType::Defined(ty) => refs.extend(ty.get_type_refs()),
            ComponentType::Func(_) => {}
            ComponentType::Component(tys) => {
                for (idx, ty) in tys.iter().enumerate() {
                    for ty_ref in ty.get_type_refs() {
                        refs.push(RefKind::decl(idx, ty_ref.ref_));
                    }
                }
            }
            ComponentType::Instance(tys) => {
                for (idx, ty) in tys.iter().enumerate() {
                    for ty_ref in ty.get_type_refs() {
                        refs.push(RefKind::decl(idx, ty_ref.ref_));
                    }
                }
            }
            ComponentType::Resource { rep, .. } => refs.extend(rep.get_type_refs()),
        }
        refs
    }
}
impl GetFuncRefs for ComponentType<'_> {
    fn get_func_refs(&self) -> Vec<RefKind> {
        match self {
            Self::Resource { dtor, .. } => {
                if let Some(func) = dtor.map(|id| IndexedRef {
                    depth: Depth::default(),
                    space: Space::CoreFunc,
                    index: id,
                }) {
                    return vec![RefKind::new(func)];
                }
            }

            ComponentType::Defined(_)
            | ComponentType::Func(_)
            | ComponentType::Component(_)
            | ComponentType::Instance(_) => {}
        }

        vec![]
    }
}
impl GetParamRefs for ComponentType<'_> {
    fn get_param_refs(&self) -> Vec<RefKind> {
        match self {
            ComponentType::Func(ty) => ty.get_param_refs(),

            ComponentType::Defined(_)
            | ComponentType::Component(_)
            | ComponentType::Instance(_)
            | ComponentType::Resource { .. } => vec![],
        }
    }
}
impl GetResultRefs for ComponentType<'_> {
    fn get_result_refs(&self) -> Vec<RefKind> {
        match self {
            ComponentType::Func(ty) => ty.get_result_refs(),
            ComponentType::Defined(_)
            | ComponentType::Component(_)
            | ComponentType::Instance(_)
            | ComponentType::Resource { .. } => vec![],
        }
    }
}

impl ReferencedIndices for ComponentFuncType<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        let mut all_refs = vec![];
        all_refs.extend(self.get_param_refs());
        all_refs.extend(self.get_result_refs());

        all_refs
    }
}
impl GetParamRefs for ComponentFuncType<'_> {
    fn get_param_refs(&self) -> Vec<RefKind> {
        let mut all_refs = vec![];
        for (idx, (_, ty)) in self.params.iter().enumerate() {
            for r in ty.get_type_refs() {
                all_refs.push(RefKind::param(idx, r.ref_));
            }
        }
        all_refs
    }
}
impl GetResultRefs for ComponentFuncType<'_> {
    fn get_result_refs(&self) -> Vec<RefKind> {
        let mut all_refs = vec![];
        if let Some(ty) = self.result {
            for r in ty.get_type_refs() {
                all_refs.push(RefKind::result(0, r.ref_));
            }
        }
        all_refs
    }
}

impl ReferencedIndices for ComponentDefinedType<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        self.get_type_refs()
    }
}
impl GetTypeRefs for ComponentDefinedType<'_> {
    fn get_type_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        match self {
            ComponentDefinedType::Record(records) => {
                for (_, ty) in records.iter() {
                    refs.extend(ty.get_type_refs());
                }
            }
            ComponentDefinedType::Variant(variants) => {
                // Explanation of variants.refines:
                // This case `refines` (is a subtype/specialization of) another case in the same variant.
                // So the u32 refers to: the index of another case within the current variant’s case list.
                // It is NOT an index into some global index space (hence not handling it here)
                for VariantCase {
                    name: _,
                    ty,
                    refines: _,
                } in variants.iter()
                {
                    if let Some(t) = ty {
                        refs.extend(t.get_type_refs());
                    }
                }
            }
            ComponentDefinedType::List(ty)
            | ComponentDefinedType::FixedSizeList(ty, _)
            | ComponentDefinedType::Option(ty) => refs.extend(ty.get_type_refs()),
            ComponentDefinedType::Tuple(tys) => {
                for ty in tys.iter() {
                    refs.extend(ty.get_type_refs());
                }
            }
            ComponentDefinedType::Result { ok, err } => {
                ok.map(|ty| refs.extend(ty.get_type_refs()));
                err.map(|ty| refs.extend(ty.get_type_refs()));
            }
            ComponentDefinedType::Own(ty) | ComponentDefinedType::Borrow(ty) => {
                refs.push(RefKind::new(IndexedRef {
                    depth: Depth::default(),
                    space: Space::CompType,
                    index: *ty,
                }))
            }
            ComponentDefinedType::Future(ty) | ComponentDefinedType::Stream(ty) => {
                ty.map(|ty| refs.extend(ty.get_type_refs()));
            }
            ComponentDefinedType::Map(key_ty, val_ty) => {
                refs.extend(key_ty.get_type_refs());
                refs.extend(val_ty.get_type_refs());
            }
            ComponentDefinedType::Primitive(_)
            | ComponentDefinedType::Enum(_)
            | ComponentDefinedType::Flags(_) => {}
        }
        refs
    }
}

impl ReferencedIndices for ComponentTypeDeclaration<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        refs.extend(self.get_type_refs());
        refs.extend(self.get_item_refs());

        refs
    }
}
impl GetTypeRefs for ComponentTypeDeclaration<'_> {
    fn get_type_refs(&self) -> Vec<RefKind> {
        match self {
            ComponentTypeDeclaration::Export { ty, .. } => ty.get_type_refs(),
            ComponentTypeDeclaration::Import(import) => import.get_type_refs(),
            ComponentTypeDeclaration::CoreType(_) // these are inner refs
            | ComponentTypeDeclaration::Type(_)   // these are inner refs
            | ComponentTypeDeclaration::Alias(_) => vec![],
        }
    }
}
impl GetItemRefs for ComponentTypeDeclaration<'_> {
    fn get_item_refs(&self) -> Vec<RefKind> {
        match self {
            ComponentTypeDeclaration::Alias(ty) => vec![ty.get_item_ref()],
            ComponentTypeDeclaration::CoreType(_) // these are inner refs
            | ComponentTypeDeclaration::Type(_)   // these are inner refs
            | ComponentTypeDeclaration::Export { .. }
            | ComponentTypeDeclaration::Import(_) => vec![],
        }
    }
}

impl ReferencedIndices for InstanceTypeDeclaration<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        refs.extend(self.get_type_refs());
        refs.extend(self.get_item_refs());

        refs
    }
}
impl GetTypeRefs for InstanceTypeDeclaration<'_> {
    fn get_type_refs(&self) -> Vec<RefKind> {
        match self {
            InstanceTypeDeclaration::Export { ty, .. } => ty.get_type_refs(),
            InstanceTypeDeclaration::CoreType(_)         // these are inner refs
            | InstanceTypeDeclaration::Type(_)           // these are inner refs
            | InstanceTypeDeclaration::Alias(_) => vec![],
        }
    }
}
impl GetItemRefs for InstanceTypeDeclaration<'_> {
    fn get_item_refs(&self) -> Vec<RefKind> {
        match self {
            InstanceTypeDeclaration::Alias(ty) => vec![ty.get_item_ref()],
            InstanceTypeDeclaration::CoreType(_)    // these are inner refs
            | InstanceTypeDeclaration::Type(_)      // these are inner refs
            | InstanceTypeDeclaration::Export { .. } => vec![],
        }
    }
}

impl ReferencedIndices for CoreType<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        self.get_type_refs()
    }
}
impl GetTypeRefs for CoreType<'_> {
    fn get_type_refs(&self) -> Vec<RefKind> {
        match self {
            CoreType::Rec(group) => group.get_type_refs(),
            CoreType::Module(_) => vec![], // these are inner refs
        }
    }
}

impl ReferencedIndices for RecGroup {
    fn referenced_indices(&self) -> Vec<RefKind> {
        self.get_type_refs()
    }
}
impl GetTypeRefs for RecGroup {
    fn get_type_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        self.types().for_each(|subty| {
            refs.extend(subty.get_type_refs());
        });

        refs
    }
}

impl ReferencedIndices for SubType {
    fn referenced_indices(&self) -> Vec<RefKind> {
        self.get_type_refs()
    }
}
impl GetTypeRefs for SubType {
    fn get_type_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        if let Some(packed) = self.supertype_idx {
            refs.push(RefKind::new(IndexedRef {
                depth: Depth::default(),
                space: Space::CoreType,
                index: packed.unpack().as_module_index().unwrap(),
            }))
        }

        refs.extend(self.composite_type.get_type_refs());

        refs
    }
}

impl ReferencedIndices for CompositeType {
    fn referenced_indices(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        refs.extend(self.get_type_refs());
        refs.extend(self.get_param_refs());
        refs.extend(self.get_result_refs());

        refs
    }
}
impl GetTypeRefs for CompositeType {
    fn get_type_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        refs.extend(self.inner.get_type_refs());

        if let Some(descriptor) = self.descriptor_idx {
            refs.push(RefKind::descriptor(IndexedRef {
                depth: Depth::default(),
                space: Space::CompType,
                index: descriptor.unpack().as_module_index().unwrap(),
            }))
        }
        if let Some(describes) = self.describes_idx {
            refs.push(RefKind::describes(IndexedRef {
                depth: Depth::default(),
                space: Space::CompType,
                index: describes.unpack().as_module_index().unwrap(),
            }))
        }

        refs
    }
}
impl GetParamRefs for CompositeType {
    fn get_param_refs(&self) -> Vec<RefKind> {
        self.inner.get_param_refs()
    }
}
impl GetResultRefs for CompositeType {
    fn get_result_refs(&self) -> Vec<RefKind> {
        self.inner.get_result_refs()
    }
}

impl ReferencedIndices for CompositeInnerType {
    fn referenced_indices(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        refs.extend(self.get_type_refs());
        refs.extend(self.get_param_refs());
        refs.extend(self.get_result_refs());

        refs
    }
}
impl GetTypeRefs for CompositeInnerType {
    fn get_type_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        match self {
            CompositeInnerType::Array(a) => refs.extend(a.0.get_type_refs()),
            CompositeInnerType::Struct(s) => {
                for ty in s.fields.iter() {
                    refs.extend(ty.get_type_refs());
                }
            }
            CompositeInnerType::Cont(ContType(ty)) => refs.push(RefKind::new(IndexedRef {
                depth: Depth::default(),
                space: Space::CompType,
                index: ty.unpack().as_module_index().unwrap(),
            })),
            CompositeInnerType::Func(_) => {}
        }
        refs
    }
}
impl GetParamRefs for CompositeInnerType {
    fn get_param_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        match self {
            CompositeInnerType::Func(f) => {
                for (idx, ty) in f.params().iter().enumerate() {
                    for r in ty.get_type_refs() {
                        refs.push(RefKind::param(idx, r.ref_));
                    }
                }
            }
            CompositeInnerType::Array(_)
            | CompositeInnerType::Struct(_)
            | CompositeInnerType::Cont(_) => {}
        }
        refs
    }
}
impl GetResultRefs for CompositeInnerType {
    fn get_result_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        match self {
            CompositeInnerType::Func(f) => {
                for (idx, ty) in f.results().iter().enumerate() {
                    for r in ty.get_type_refs() {
                        refs.push(RefKind::result(idx, r.ref_));
                    }
                }
            }
            CompositeInnerType::Array(_)
            | CompositeInnerType::Struct(_)
            | CompositeInnerType::Cont(_) => {}
        }
        refs
    }
}

impl ReferencedIndices for FieldType {
    fn referenced_indices(&self) -> Vec<RefKind> {
        self.get_type_refs()
    }
}
impl GetTypeRefs for FieldType {
    fn get_type_refs(&self) -> Vec<RefKind> {
        self.element_type.get_type_refs()
    }
}

impl ReferencedIndices for StorageType {
    fn referenced_indices(&self) -> Vec<RefKind> {
        self.get_type_refs()
    }
}
impl GetTypeRefs for StorageType {
    fn get_type_refs(&self) -> Vec<RefKind> {
        match self {
            StorageType::I8 | StorageType::I16 => vec![],
            StorageType::Val(value) => value.get_type_refs(),
        }
    }
}

impl ReferencedIndices for ModuleTypeDeclaration<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        match self {
            ModuleTypeDeclaration::Type(group) => group.referenced_indices(),
            ModuleTypeDeclaration::Export { ty, .. } => ty.referenced_indices(),
            ModuleTypeDeclaration::Import(i) => i.ty.referenced_indices(),
            ModuleTypeDeclaration::OuterAlias { kind, count, index } => {
                vec![RefKind::new(IndexedRef {
                    depth: Depth(*count as usize),
                    space: kind.index_space_of(),
                    index: *index,
                })]
            }
        }
    }
}
impl GetTypeRefs for ModuleTypeDeclaration<'_> {
    fn get_type_refs(&self) -> Vec<RefKind> {
        match self {
            ModuleTypeDeclaration::Type(group) => group.get_type_refs(),
            ModuleTypeDeclaration::Export { ty, .. } => ty.get_type_refs(),
            ModuleTypeDeclaration::Import(i) => i.ty.get_type_refs(),
            ModuleTypeDeclaration::OuterAlias { kind, count, index } => {
                vec![RefKind::new(IndexedRef {
                    depth: Depth(*count as usize),
                    space: kind.index_space_of(),
                    index: *index,
                })]
            }
        }
    }
}

impl ReferencedIndices for VariantCase<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        self.get_type_refs()
    }
}
impl GetTypeRefs for VariantCase<'_> {
    fn get_type_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        if let Some(ty) = self.ty {
            refs.extend(ty.get_type_refs())
        }

        if let Some(index) = self.refines {
            refs.push(RefKind::new(IndexedRef {
                depth: Depth::default(),
                space: Space::CompType,
                index,
            }))
        }

        refs
    }
}

impl ReferencedIndices for ValType {
    fn referenced_indices(&self) -> Vec<RefKind> {
        self.get_type_refs()
    }
}
impl GetTypeRefs for ValType {
    fn get_type_refs(&self) -> Vec<RefKind> {
        match self {
            ValType::I32 | ValType::I64 | ValType::F32 | ValType::F64 | ValType::V128 => vec![],
            ValType::Ref(r) => r.get_type_refs(),
        }
    }
}

impl ReferencedIndices for RefType {
    fn referenced_indices(&self) -> Vec<RefKind> {
        self.get_type_refs()
    }
}
impl GetTypeRefs for RefType {
    fn get_type_refs(&self) -> Vec<RefKind> {
        let index = if self.is_concrete_type_ref() {
            self.type_index()
                .unwrap()
                .unpack()
                .as_module_index()
                .unwrap()
        } else if self.is_exact_type_ref() {
            todo!("Need to support this still, we don't have a test case that we can check implementation with yet!")
        } else {
            // This doesn't actually reference anything
            return vec![];
        };

        vec![RefKind::new(IndexedRef {
            depth: Depth::default(),
            space: Space::CoreType,
            index,
        })]
    }
}

impl ReferencedIndices for CanonicalFunction {
    fn referenced_indices(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        refs.extend(self.get_func_refs());
        refs.extend(self.get_type_refs());
        refs.extend(self.get_mem_refs());
        refs.extend(self.get_tbl_refs());

        refs
    }
}
impl GetFuncRefs for CanonicalFunction {
    fn get_func_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        match self {
            CanonicalFunction::Lift {
                core_func_index,
                options,
                ..
            } => {
                refs.push(RefKind::new(IndexedRef {
                    depth: Depth::default(),
                    space: Space::CoreFunc,
                    index: *core_func_index,
                }));

                for opt in options.iter() {
                    refs.extend(opt.get_func_refs());
                }
            }

            CanonicalFunction::Lower {
                func_index,
                options,
            } => {
                refs.push(RefKind::new(IndexedRef {
                    depth: Depth::default(),
                    space: Space::CompFunc,
                    index: *func_index,
                }));
                for opt in options.iter() {
                    refs.extend(opt.get_func_refs());
                }
            }

            CanonicalFunction::ThreadSpawnRef { func_ty_index } => {
                refs.push(RefKind::new(IndexedRef {
                    depth: Depth::default(),
                    space: Space::CompFunc,
                    index: *func_ty_index,
                }))
            }
            CanonicalFunction::TaskReturn { options, .. }
            | CanonicalFunction::StreamRead { options, .. }
            | CanonicalFunction::StreamWrite { options, .. }
            | CanonicalFunction::FutureRead { options, .. }
            | CanonicalFunction::FutureWrite { options, .. }
            | CanonicalFunction::ErrorContextNew { options, .. }
            | CanonicalFunction::ErrorContextDebugMessage { options, .. } => {
                for opt in options.iter() {
                    refs.extend(opt.get_func_refs());
                }
            }
            CanonicalFunction::ResourceNew { .. }
            | CanonicalFunction::ResourceDrop { .. }
            | CanonicalFunction::ResourceDropAsync { .. }
            | CanonicalFunction::ResourceRep { .. }
            | CanonicalFunction::ThreadSpawnIndirect { .. }
            | CanonicalFunction::ThreadAvailableParallelism
            | CanonicalFunction::BackpressureInc
            | CanonicalFunction::BackpressureDec
            | CanonicalFunction::TaskCancel
            | CanonicalFunction::ContextGet(_)
            | CanonicalFunction::ContextSet(_)
            | CanonicalFunction::ThreadYield { .. }
            | CanonicalFunction::SubtaskDrop
            | CanonicalFunction::SubtaskCancel { .. }
            | CanonicalFunction::StreamNew { .. }
            | CanonicalFunction::StreamCancelRead { .. }
            | CanonicalFunction::StreamCancelWrite { .. }
            | CanonicalFunction::StreamDropReadable { .. }
            | CanonicalFunction::StreamDropWritable { .. }
            | CanonicalFunction::FutureNew { .. }
            | CanonicalFunction::FutureCancelRead { .. }
            | CanonicalFunction::FutureCancelWrite { .. }
            | CanonicalFunction::FutureDropReadable { .. }
            | CanonicalFunction::FutureDropWritable { .. }
            | CanonicalFunction::ErrorContextDrop
            | CanonicalFunction::WaitableSetNew
            | CanonicalFunction::WaitableSetWait { .. }
            | CanonicalFunction::WaitableSetPoll { .. }
            | CanonicalFunction::WaitableSetDrop
            | CanonicalFunction::WaitableJoin
            | CanonicalFunction::ThreadIndex
            | CanonicalFunction::ThreadNewIndirect { .. }
            | CanonicalFunction::ThreadSwitchTo { .. }
            | CanonicalFunction::ThreadSuspend { .. }
            | CanonicalFunction::ThreadResumeLater
            | CanonicalFunction::ThreadYieldTo { .. } => {}
        }
        refs
    }
}
impl GetTypeRefs for CanonicalFunction {
    fn get_type_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        match self {
            CanonicalFunction::Lift {
                type_index,
                options,
                ..
            } => {
                refs.push(RefKind::new(IndexedRef {
                    depth: Depth::default(),
                    space: Space::CompType,
                    index: *type_index,
                }));

                for opt in options.iter() {
                    refs.extend(opt.get_type_refs());
                }
            }
            CanonicalFunction::Lower { options, .. } => {
                for opt in options.iter() {
                    refs.extend(opt.get_type_refs());
                }
            }
            CanonicalFunction::ResourceNew { resource }
            | CanonicalFunction::ResourceDrop { resource }
            | CanonicalFunction::ResourceDropAsync { resource }
            | CanonicalFunction::ResourceRep { resource } => refs.push(RefKind::new(IndexedRef {
                depth: Depth::default(),
                space: Space::CompType,
                index: *resource,
            })),
            CanonicalFunction::ThreadSpawnIndirect { func_ty_index, .. }
            | CanonicalFunction::ThreadNewIndirect { func_ty_index, .. } => {
                refs.push(RefKind::new(IndexedRef {
                    depth: Depth::default(),
                    space: Space::CoreType,
                    index: *func_ty_index,
                }))
            }
            CanonicalFunction::TaskReturn { result, options } => {
                result.map(|ty| {
                    refs.extend(ty.get_type_refs());
                });

                for opt in options.iter() {
                    refs.extend(opt.get_type_refs());
                }
            }
            CanonicalFunction::StreamNew { ty }
            | CanonicalFunction::StreamDropReadable { ty }
            | CanonicalFunction::StreamDropWritable { ty }
            | CanonicalFunction::StreamCancelRead { ty, .. }
            | CanonicalFunction::StreamCancelWrite { ty, .. }
            | CanonicalFunction::FutureNew { ty }
            | CanonicalFunction::FutureDropReadable { ty }
            | CanonicalFunction::FutureDropWritable { ty }
            | CanonicalFunction::FutureCancelRead { ty, .. }
            | CanonicalFunction::FutureCancelWrite { ty, .. } => {
                refs.push(RefKind::new(IndexedRef {
                    depth: Depth::default(),
                    space: Space::CompType,
                    index: *ty,
                }))
            }
            CanonicalFunction::StreamRead { ty, options }
            | CanonicalFunction::StreamWrite { ty, options }
            | CanonicalFunction::FutureRead { ty, options }
            | CanonicalFunction::FutureWrite { ty, options } => {
                refs.push(RefKind::new(IndexedRef {
                    depth: Depth::default(),
                    space: Space::CompType,
                    index: *ty,
                }));

                for opt in options.iter() {
                    refs.extend(opt.get_type_refs());
                }
            }
            CanonicalFunction::ErrorContextNew { options }
            | CanonicalFunction::ErrorContextDebugMessage { options } => {
                for opt in options.iter() {
                    refs.extend(opt.get_type_refs());
                }
            }
            CanonicalFunction::ThreadSpawnRef { .. }
            | CanonicalFunction::ThreadAvailableParallelism
            | CanonicalFunction::BackpressureInc
            | CanonicalFunction::BackpressureDec
            | CanonicalFunction::TaskCancel
            | CanonicalFunction::ContextGet(_)
            | CanonicalFunction::ContextSet(_)
            | CanonicalFunction::ThreadYield { .. }
            | CanonicalFunction::SubtaskDrop
            | CanonicalFunction::SubtaskCancel { .. }
            | CanonicalFunction::ErrorContextDrop
            | CanonicalFunction::WaitableSetNew
            | CanonicalFunction::WaitableSetWait { .. }
            | CanonicalFunction::WaitableSetPoll { .. }
            | CanonicalFunction::WaitableSetDrop
            | CanonicalFunction::WaitableJoin
            | CanonicalFunction::ThreadIndex
            | CanonicalFunction::ThreadSwitchTo { .. }
            | CanonicalFunction::ThreadSuspend { .. }
            | CanonicalFunction::ThreadResumeLater
            | CanonicalFunction::ThreadYieldTo { .. } => {}
        }

        refs
    }
}
impl GetMemRefs for CanonicalFunction {
    fn get_mem_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        match self {
            CanonicalFunction::Lift { options, .. }
            | CanonicalFunction::Lower { options, .. }
            | CanonicalFunction::TaskReturn { options, .. }
            | CanonicalFunction::StreamRead { options, .. }
            | CanonicalFunction::StreamWrite { options, .. }
            | CanonicalFunction::FutureRead { options, .. }
            | CanonicalFunction::FutureWrite { options, .. }
            | CanonicalFunction::ErrorContextNew { options }
            | CanonicalFunction::ErrorContextDebugMessage { options } => {
                for opt in options.iter() {
                    refs.extend(opt.get_mem_refs());
                }
            }
            CanonicalFunction::WaitableSetWait { memory, .. }
            | CanonicalFunction::WaitableSetPoll { memory, .. } => {
                refs.push(RefKind::new(IndexedRef {
                    depth: Depth::default(),
                    space: Space::CoreMemory,
                    index: *memory,
                }))
            }
            CanonicalFunction::ResourceNew { .. }
            | CanonicalFunction::ResourceDrop { .. }
            | CanonicalFunction::ResourceDropAsync { .. }
            | CanonicalFunction::ResourceRep { .. }
            | CanonicalFunction::ThreadSpawnRef { .. }
            | CanonicalFunction::ThreadSpawnIndirect { .. }
            | CanonicalFunction::ThreadAvailableParallelism
            | CanonicalFunction::BackpressureInc
            | CanonicalFunction::BackpressureDec
            | CanonicalFunction::TaskCancel
            | CanonicalFunction::ContextGet(_)
            | CanonicalFunction::ContextSet(_)
            | CanonicalFunction::ThreadYield { .. }
            | CanonicalFunction::SubtaskDrop
            | CanonicalFunction::SubtaskCancel { .. }
            | CanonicalFunction::StreamNew { .. }
            | CanonicalFunction::StreamCancelRead { .. }
            | CanonicalFunction::StreamCancelWrite { .. }
            | CanonicalFunction::StreamDropReadable { .. }
            | CanonicalFunction::StreamDropWritable { .. }
            | CanonicalFunction::FutureNew { .. }
            | CanonicalFunction::FutureCancelRead { .. }
            | CanonicalFunction::FutureCancelWrite { .. }
            | CanonicalFunction::FutureDropReadable { .. }
            | CanonicalFunction::FutureDropWritable { .. }
            | CanonicalFunction::ErrorContextDrop
            | CanonicalFunction::WaitableSetNew
            | CanonicalFunction::WaitableSetDrop
            | CanonicalFunction::WaitableJoin
            | CanonicalFunction::ThreadIndex
            | CanonicalFunction::ThreadNewIndirect { .. }
            | CanonicalFunction::ThreadSwitchTo { .. }
            | CanonicalFunction::ThreadSuspend { .. }
            | CanonicalFunction::ThreadResumeLater
            | CanonicalFunction::ThreadYieldTo { .. } => {}
        }
        refs
    }
}
impl GetTableRefs for CanonicalFunction {
    fn get_tbl_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        match self {
            CanonicalFunction::ThreadSpawnIndirect { table_index, .. }
            | CanonicalFunction::ThreadNewIndirect { table_index, .. } => {
                refs.push(RefKind::new(IndexedRef {
                    depth: Depth::default(),
                    space: Space::CoreTable,
                    index: *table_index,
                }))
            }
            CanonicalFunction::Lift { .. }
            | CanonicalFunction::Lower { .. }
            | CanonicalFunction::ResourceNew { .. }
            | CanonicalFunction::ResourceDrop { .. }
            | CanonicalFunction::ResourceDropAsync { .. }
            | CanonicalFunction::ResourceRep { .. }
            | CanonicalFunction::ThreadSpawnRef { .. }
            | CanonicalFunction::ThreadAvailableParallelism
            | CanonicalFunction::BackpressureInc
            | CanonicalFunction::BackpressureDec
            | CanonicalFunction::TaskReturn { .. }
            | CanonicalFunction::TaskCancel
            | CanonicalFunction::ContextGet(_)
            | CanonicalFunction::ContextSet(_)
            | CanonicalFunction::ThreadYield { .. }
            | CanonicalFunction::SubtaskDrop
            | CanonicalFunction::SubtaskCancel { .. }
            | CanonicalFunction::StreamNew { .. }
            | CanonicalFunction::StreamRead { .. }
            | CanonicalFunction::StreamWrite { .. }
            | CanonicalFunction::StreamCancelRead { .. }
            | CanonicalFunction::StreamCancelWrite { .. }
            | CanonicalFunction::StreamDropReadable { .. }
            | CanonicalFunction::StreamDropWritable { .. }
            | CanonicalFunction::FutureNew { .. }
            | CanonicalFunction::FutureRead { .. }
            | CanonicalFunction::FutureWrite { .. }
            | CanonicalFunction::FutureCancelRead { .. }
            | CanonicalFunction::FutureCancelWrite { .. }
            | CanonicalFunction::FutureDropReadable { .. }
            | CanonicalFunction::FutureDropWritable { .. }
            | CanonicalFunction::ErrorContextNew { .. }
            | CanonicalFunction::ErrorContextDebugMessage { .. }
            | CanonicalFunction::ErrorContextDrop
            | CanonicalFunction::WaitableSetNew
            | CanonicalFunction::WaitableSetWait { .. }
            | CanonicalFunction::WaitableSetPoll { .. }
            | CanonicalFunction::WaitableSetDrop
            | CanonicalFunction::WaitableJoin
            | CanonicalFunction::ThreadIndex
            | CanonicalFunction::ThreadSwitchTo { .. }
            | CanonicalFunction::ThreadSuspend { .. }
            | CanonicalFunction::ThreadResumeLater
            | CanonicalFunction::ThreadYieldTo { .. } => {}
        }
        refs
    }
}

impl ReferencedIndices for CanonicalOption {
    fn referenced_indices(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        refs.extend(self.get_type_refs());
        refs.extend(self.get_func_refs());
        refs.extend(self.get_mem_refs());

        refs
    }
}
impl GetTypeRefs for CanonicalOption {
    fn get_type_refs(&self) -> Vec<RefKind> {
        match self {
            CanonicalOption::CoreType(id) => vec![RefKind::new(IndexedRef {
                depth: Depth::default(),
                space: Space::CoreType,
                index: *id,
            })],
            CanonicalOption::UTF8
            | CanonicalOption::UTF16
            | CanonicalOption::CompactUTF16
            | CanonicalOption::Memory(_)
            | CanonicalOption::Realloc(_)
            | CanonicalOption::PostReturn(_)
            | CanonicalOption::Async
            | CanonicalOption::Callback(_)
            | CanonicalOption::Gc => vec![],
        }
    }
}
impl GetFuncRefs for CanonicalOption {
    fn get_func_refs(&self) -> Vec<RefKind> {
        match self {
            CanonicalOption::Realloc(id)
            | CanonicalOption::PostReturn(id)
            | CanonicalOption::Callback(id) => vec![RefKind::new(IndexedRef {
                depth: Depth::default(),
                space: Space::CoreFunc,
                index: *id,
            })],
            CanonicalOption::CoreType(_)
            | CanonicalOption::UTF8
            | CanonicalOption::UTF16
            | CanonicalOption::CompactUTF16
            | CanonicalOption::Memory(_)
            | CanonicalOption::Async
            | CanonicalOption::Gc => vec![],
        }
    }
}
impl GetMemRefs for CanonicalOption {
    fn get_mem_refs(&self) -> Vec<RefKind> {
        match self {
            CanonicalOption::Memory(id) => vec![RefKind::new(IndexedRef {
                depth: Depth::default(),
                space: Space::CoreMemory,
                index: *id,
            })],
            CanonicalOption::CoreType(_)
            | CanonicalOption::UTF8
            | CanonicalOption::UTF16
            | CanonicalOption::CompactUTF16
            | CanonicalOption::Realloc(_)
            | CanonicalOption::PostReturn(_)
            | CanonicalOption::Async
            | CanonicalOption::Callback(_)
            | CanonicalOption::Gc => vec![],
        }
    }
}

impl ReferencedIndices for ComponentImport<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        self.get_type_refs()
    }
}
impl GetTypeRefs for ComponentImport<'_> {
    fn get_type_refs(&self) -> Vec<RefKind> {
        self.ty.get_type_refs()
    }
}

impl ReferencedIndices for ComponentTypeRef {
    fn referenced_indices(&self) -> Vec<RefKind> {
        self.get_type_refs()
    }
}
impl GetTypeRefs for ComponentTypeRef {
    fn get_type_refs(&self) -> Vec<RefKind> {
        let depth = Depth::default();
        match &self {
            // The reference is to a core module type.
            // The index is expected to be core type index to a core module type.
            ComponentTypeRef::Module(id) => vec![RefKind::new(IndexedRef {
                depth,
                space: Space::CoreType,
                index: *id,
            })],
            ComponentTypeRef::Func(id) => vec![RefKind::new(IndexedRef {
                depth,
                space: Space::CompType,
                index: *id,
            })],
            ComponentTypeRef::Instance(id) => vec![RefKind::new(IndexedRef {
                depth,
                space: Space::CompType,
                index: *id,
            })],
            ComponentTypeRef::Component(id) => vec![RefKind::new(IndexedRef {
                depth,
                space: Space::CompType, // verified in wat (instantiate.wast)
                index: *id,
            })],
            ComponentTypeRef::Value(ty) => ty.get_type_refs(),
            ComponentTypeRef::Type(ty_bounds) => ty_bounds.get_type_refs(),
        }
    }
}

impl ReferencedIndices for TypeBounds {
    fn referenced_indices(&self) -> Vec<RefKind> {
        self.get_type_refs()
    }
}
impl GetTypeRefs for TypeBounds {
    fn get_type_refs(&self) -> Vec<RefKind> {
        match self {
            TypeBounds::Eq(id) => vec![RefKind::new(IndexedRef {
                depth: Depth::default(),
                space: Space::CompType,
                index: *id,
            })],
            TypeBounds::SubResource => vec![],
        }
    }
}

impl ReferencedIndices for ComponentValType {
    fn referenced_indices(&self) -> Vec<RefKind> {
        self.get_type_refs()
    }
}
impl GetTypeRefs for ComponentValType {
    fn get_type_refs(&self) -> Vec<RefKind> {
        match self {
            ComponentValType::Primitive(_) => vec![],
            ComponentValType::Type(id) => vec![RefKind::new(IndexedRef {
                depth: Depth::default(),
                space: Space::CompType,
                index: *id,
            })],
        }
    }
}

impl ReferencedIndices for ComponentInstantiationArg<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        vec![self.get_item_ref()]
    }
}
impl GetItemRef for ComponentInstantiationArg<'_> {
    fn get_item_ref(&self) -> RefKind {
        RefKind::new(IndexedRef {
            depth: Depth::default(),
            space: self.kind.index_space_of(),
            index: self.index,
        })
    }
}

impl ReferencedIndices for ComponentExport<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        refs.extend(self.get_type_refs());
        refs.push(self.get_item_ref());

        refs
    }
}
impl GetTypeRefs for ComponentExport<'_> {
    fn get_type_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        if let Some(ty) = self.ty {
            refs.extend(ty.get_type_refs())
        }

        refs
    }
}
impl GetItemRef for ComponentExport<'_> {
    fn get_item_ref(&self) -> RefKind {
        RefKind::new(IndexedRef {
            depth: Depth::default(),
            space: self.kind.index_space_of(),
            index: self.index,
        })
    }
}

impl ReferencedIndices for Export<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        vec![self.get_item_ref()]
    }
}
impl GetItemRef for Export<'_> {
    fn get_item_ref(&self) -> RefKind {
        RefKind::new(IndexedRef {
            depth: Depth::default(),
            space: self.kind.index_space_of(),
            index: self.index,
        })
    }
}

impl ReferencedIndices for InstantiationArg<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        vec![self.get_item_ref()]
    }
}
impl GetItemRef for InstantiationArg<'_> {
    fn get_item_ref(&self) -> RefKind {
        RefKind::new(IndexedRef {
            depth: Depth::default(),
            space: self.kind.index_space_of(),
            index: self.index,
        })
    }
}

impl ReferencedIndices for Instance<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        refs.extend(self.get_module_refs());
        refs.extend(self.get_item_refs());

        refs
    }
}
impl GetModuleRefs for Instance<'_> {
    fn get_module_refs(&self) -> Vec<RefKind> {
        match self {
            Instance::Instantiate { module_index, .. } => vec![RefKind::new(IndexedRef {
                depth: Depth::default(),
                space: Space::CoreModule,
                index: *module_index,
            })],
            Instance::FromExports(_) => vec![],
        }
    }
}
impl GetItemRefs for Instance<'_> {
    fn get_item_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        match self {
            Instance::Instantiate { args, .. } => {
                // Recursively include indices from options
                for arg in args.iter() {
                    refs.push(arg.get_item_ref());
                }
            }
            Instance::FromExports(exports) => {
                // Recursively include indices from options
                for exp in exports.iter() {
                    refs.push(exp.get_item_ref());
                }
            }
        }
        refs
    }
}

impl ReferencedIndices for TypeRef {
    fn referenced_indices(&self) -> Vec<RefKind> {
        self.get_type_refs()
    }
}
impl GetTypeRefs for TypeRef {
    fn get_type_refs(&self) -> Vec<RefKind> {
        match self {
            TypeRef::Func(ty)
            | TypeRef::Tag(TagType {
                kind: _,
                func_type_idx: ty,
            }) => vec![RefKind::new(IndexedRef {
                depth: Depth::default(),
                space: Space::CoreType,
                index: *ty,
            })],
            TypeRef::Table(_) | TypeRef::Memory(_) | TypeRef::Global(_) | TypeRef::FuncExact(_) => {
                vec![]
            }
        }
    }
}

impl ReferencedIndices for ComponentAlias<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        vec![self.get_item_ref()]
    }
}
impl GetItemRef for ComponentAlias<'_> {
    fn get_item_ref(&self) -> RefKind {
        match self {
            ComponentAlias::InstanceExport { instance_index, .. } => RefKind::new(IndexedRef {
                depth: Depth::default(),
                space: Space::CompInst,
                index: *instance_index,
            }),
            ComponentAlias::CoreInstanceExport { instance_index, .. } => RefKind::new(IndexedRef {
                depth: Depth::default(),
                space: Space::CoreInst,
                index: *instance_index,
            }),
            ComponentAlias::Outer { count, index, kind } => RefKind::new(IndexedRef {
                depth: Depth(*count as usize),
                space: kind.index_space_of(),
                index: *index,
            }),
        }
    }
}

impl ReferencedIndices for ComponentInstance<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        refs.extend(self.get_comp_refs());
        refs.extend(self.get_item_refs());

        refs
    }
}
impl GetCompRefs for ComponentInstance<'_> {
    fn get_comp_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        match self {
            ComponentInstance::Instantiate {
                component_index, ..
            } => {
                refs.push(RefKind::new(IndexedRef {
                    depth: Depth::default(),
                    space: Space::Comp, // verified in alias.wast
                    index: *component_index,
                }));
            }
            ComponentInstance::FromExports(_) => {}
        }
        refs
    }
}
impl GetItemRefs for ComponentInstance<'_> {
    fn get_item_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        match self {
            ComponentInstance::Instantiate { args, .. } => {
                // Recursively include indices from args
                for arg in args.iter() {
                    refs.push(arg.get_item_ref());
                }
            }
            ComponentInstance::FromExports(export) => {
                // Recursively include indices from args
                for exp in export.iter() {
                    refs.push(exp.get_item_ref());
                }
            }
        }
        refs
    }
}

impl ReferencedIndices for CustomSection<'_> {
    fn referenced_indices(&self) -> Vec<RefKind> {
        vec![]
    }
}

impl ReferencedIndices for ComponentStartFunction {
    fn referenced_indices(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        refs.push(self.get_func_ref());
        refs.extend(self.get_arg_refs());

        refs
    }
}
impl GetFuncRef for ComponentStartFunction {
    fn get_func_ref(&self) -> RefKind {
        RefKind::new(IndexedRef {
            depth: Depth::default(),
            space: Space::CompFunc,
            index: self.func_index,
        })
    }
}
impl GetArgRefs for ComponentStartFunction {
    fn get_arg_refs(&self) -> Vec<RefKind> {
        let mut refs = vec![];
        for (idx, v) in self.arguments.iter().enumerate() {
            refs.push(RefKind::result(
                idx,
                IndexedRef {
                    depth: Depth::default(),
                    space: Space::CompVal,
                    index: *v,
                },
            ));
        }
        refs
    }
}
