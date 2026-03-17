//! Fully-resolved, index-free representations of WebAssembly component types.
//!
//! These types are produced by [`Component::concretize_import`] and
//! [`Component::concretize_export`], which follow the entire reference chain
//! (outer aliases, imports, nested scopes) and return concrete Rust types with
//! no remaining index references.
//!
//! # WIT interface focus
//!
//! The current implementation is scoped to the needs of [WIT]-defined interfaces,
//! where component imports and exports are either:
//!
//! - **Instance types** whose exports are all **functions** (`ComponentType::Instance`
//!   with `ComponentTypeRef::Func` exports), or
//! - **Function types** (`ComponentType::Func`).
//!
//! Non-function instance exports (nested instances, type exports, value exports) are
//! intentionally skipped.  This covers the full surface of every WIT interface today.
//!
//! [WIT]: https://component-model.bytecodealliance.org/design/wit.html
//!
//! # TODO: extend beyond WIT interfaces
//!
//! A future release should generalise [`ConcreteType::Instance`] to carry all export
//! kinds, not just functions.  Candidates include:
//!
//! - Type exports (resource declarations, defined types)
//! - Nested instance exports
//! - Value exports
//!
//! Until then, non-function exports silently produce no entry in the `Instance` vec.

use crate::ir::component::idx_spaces::Space;
use crate::ir::component::refs::{Depth, GetCompRefs, GetItemRef, GetTypeRefs, IndexedRef};
use crate::ir::component::visitor::{ResolvedItem, ScopedVisitCtx};
use crate::Component;
use wasmparser::{
    ComponentAlias, ComponentDefinedType, ComponentFuncType, ComponentType, ComponentValType,
    InstanceTypeDeclaration, PrimitiveValType,
};

// ============================================================
// Public output types
// ============================================================

/// A fully-resolved component type with no remaining index references.
///
/// Produced by [`Component::concretize_import`] and [`Component::concretize_export`].
#[derive(Debug, Clone)]
pub enum ConcreteType<'a> {
    /// A WIT instance interface — a named set of exported functions.
    ///
    /// Each entry is `(function_name, signature)`.  Only `ComponentTypeRef::Func`
    /// exports are included; this matches the WIT interface model where every
    /// instance export is a function.  See the [module-level TODO](self) for plans
    /// to extend this to other export kinds in a future release.
    Instance(Vec<(&'a str, ConcreteFuncType<'a>)>),
    /// A single function type.
    Func(ConcreteFuncType<'a>),
    /// A resource (own or borrow handle).
    Resource,
}

/// A fully-resolved function signature with no index references.
#[derive(Debug, Clone)]
pub struct ConcreteFuncType<'a> {
    /// Named parameters.
    pub params: Vec<(&'a str, ConcreteValType<'a>)>,
    /// Return type, if any.
    pub result: Option<ConcreteValType<'a>>,
}

/// A fully-resolved value type with no index references.
#[derive(Debug, Clone)]
pub enum ConcreteValType<'a> {
    Primitive(PrimitiveValType),
    Record(Vec<(&'a str, Box<ConcreteValType<'a>>)>),
    Variant(Vec<(&'a str, Option<Box<ConcreteValType<'a>>>)>),
    List(Box<ConcreteValType<'a>>),
    Tuple(Vec<ConcreteValType<'a>>),
    Option(Box<ConcreteValType<'a>>),
    Result {
        ok: Option<Box<ConcreteValType<'a>>>,
        err: Option<Box<ConcreteValType<'a>>>,
    },
    Flags(Vec<&'a str>),
    Enum(Vec<&'a str>),
    Map(Box<ConcreteValType<'a>>, Box<ConcreteValType<'a>>),
    FixedSizeList(Box<ConcreteValType<'a>>, u32),
    /// A resource handle (`own<T>` or `borrow<T>`).
    Resource,
    /// An async handle (`future<T>` or `stream<T>`).
    AsyncHandle,
}

// ============================================================
// Public API on Component
// ============================================================

impl<'a> Component<'a> {
    /// Resolve an import by name to its fully-concrete type.
    ///
    /// Follows all alias chains, outer references, and index lookups so that
    /// the returned [`ConcreteType`] contains no remaining index references.
    ///
    /// Returns `None` if no import with the given name exists, or if its type
    /// is not one wirm currently concretizes (e.g. a raw module import).
    pub fn concretize_import(&'a self, name: &str) -> Option<ConcreteType<'a>> {
        match self.resolve_named_import(name)? {
            ResolvedItem::CompType(_, ty) => concretize_comp_type(self, ty),
            _ => None,
        }
    }

    /// Resolve an export by name to its fully-concrete type.
    ///
    /// Follows all alias chains, outer references, and index lookups so that
    /// the returned [`ConcreteType`] contains no remaining index references.
    ///
    /// Returns `None` if no export with the given name exists, or if its type
    /// is not one wirm currently concretizes.
    pub fn concretize_export(&'a self, name: &str) -> Option<ConcreteType<'a>> {
        match self.resolve_named_export(name)? {
            ResolvedItem::CompType(_, ty) => concretize_comp_type(self, ty),
            _ => None,
        }
    }
}

// ============================================================
// Internal concretization logic
// ============================================================

fn concretize_comp_type<'a>(
    comp: &'a Component<'a>,
    ty: &'a ComponentType<'a>,
) -> Option<ConcreteType<'a>> {
    match ty {
        ComponentType::Instance(decls) => {
            let cx = comp.enter_type_scope(ty);
            Some(ConcreteType::Instance(concretize_instance_decls(
                comp, decls, &cx,
            )))
        }
        ComponentType::Func(ft) => {
            let cx = comp.enter_type_scope(ty);
            Some(ConcreteType::Func(concretize_func_ty(ft, comp, &cx)))
        }
        ComponentType::Resource { .. } => Some(ConcreteType::Resource),
        _ => None,
    }
}

fn concretize_instance_decls<'a>(
    comp: &'a Component<'a>,
    decls: &'a [InstanceTypeDeclaration<'a>],
    cx: &ScopedVisitCtx<'a>,
) -> Vec<(&'a str, ConcreteFuncType<'a>)> {
    let mut funcs = vec![];
    for decl in decls {
        if let InstanceTypeDeclaration::Export { name, .. } = decl {
            if let Some(type_ref) = decl.get_type_refs().first() {
                let resolved = cx.resolve(&type_ref.ref_);
                if let Some(ft) = resolve_and_concretize_func(resolved, comp, cx) {
                    funcs.push((name.0, ft));
                }
            }
        }
    }
    funcs
}

/// Follow aliases until we reach a function type, then concretize it.
///
/// Only `ComponentType::Func` is considered a match; all other resolved types
/// return `None`.  This intentionally limits instance-export concretization to
/// WIT function exports — see the [module-level TODO](self) for the plan to
/// extend beyond WIT interfaces in a future release.
///
/// Returns an owned [`ConcreteFuncType`] (rather than a borrowed
/// `&ComponentFuncType`) so that cross-scope resolution via `InstanceExport`
/// aliases — where the func type lives in a different component — can be
/// returned without lifetime issues.
fn resolve_and_concretize_func<'a>(
    resolved: ResolvedItem<'a, 'a>,
    comp: &'a Component<'a>,
    cx: &ScopedVisitCtx<'a>,
) -> Option<ConcreteFuncType<'a>> {
    match resolved {
        ResolvedItem::CompType(_, ComponentType::Func(ft)) => {
            Some(concretize_func_ty(ft, comp, cx))
        }
        ResolvedItem::Alias(_, alias @ ComponentAlias::Outer { .. }) => {
            resolve_and_concretize_func(cx.resolve(&alias.get_item_ref().ref_), comp, cx)
        }
        // `InstanceExport` aliases carry the instance index relative to the owning component's
        // instance namespace.  Resolve through the instantiated component's export instead of
        // calling `cx.resolve()`, which would incorrectly dispatch depth=0 into the type body.
        ResolvedItem::Alias(
            _,
            ComponentAlias::InstanceExport {
                instance_index,
                name,
                ..
            },
        ) => {
            let nested_comp = resolve_instantiated_comp(comp, *instance_index)?;
            match nested_comp.concretize_export(name)? {
                ConcreteType::Func(ft) => Some(ft),
                _ => None,
            }
        }
        _ => None,
    }
}

fn concretize_func_ty<'a>(
    ft: &'a ComponentFuncType<'a>,
    comp: &'a Component<'a>,
    cx: &ScopedVisitCtx<'a>,
) -> ConcreteFuncType<'a> {
    ConcreteFuncType {
        params: ft
            .params
            .iter()
            .map(|(name, ty)| (*name, concretize_val_type(ty, comp, cx)))
            .collect(),
        result: ft
            .result
            .as_ref()
            .map(|ty| concretize_val_type(ty, comp, cx)),
    }
}

fn concretize_val_type<'a>(
    ty: &'a ComponentValType,
    comp: &'a Component<'a>,
    cx: &ScopedVisitCtx<'a>,
) -> ConcreteValType<'a> {
    match ty {
        ComponentValType::Primitive(p) => ConcreteValType::Primitive(*p),
        ComponentValType::Type(_) => {
            if let Some(type_ref) = ty.get_type_refs().first() {
                concretize_from_resolved(cx.resolve(&type_ref.ref_), comp, cx)
            } else {
                ConcreteValType::Resource
            }
        }
    }
}

fn concretize_from_resolved<'a>(
    resolved: ResolvedItem<'a, 'a>,
    comp: &'a Component<'a>,
    cx: &ScopedVisitCtx<'a>,
) -> ConcreteValType<'a> {
    match resolved {
        ResolvedItem::CompType(_, ty) => concretize_comp_type_to_val(ty, comp, cx),
        ResolvedItem::Alias(_, alias @ ComponentAlias::Outer { .. }) => {
            concretize_from_resolved(cx.resolve(&alias.get_item_ref().ref_), comp, cx)
        }
        // Same fix as in `resolve_and_concretize_func`: bypass `cx.resolve()` for InstanceExport
        // and look up the type directly through the instantiated component's export chain.
        ResolvedItem::Alias(
            _,
            ComponentAlias::InstanceExport {
                instance_index,
                name,
                ..
            },
        ) => {
            let Some(nested_comp) = resolve_instantiated_comp(comp, *instance_index) else {
                return ConcreteValType::Resource;
            };
            match nested_comp.concretize_export(name) {
                Some(ConcreteType::Resource) | None => ConcreteValType::Resource,
                Some(ConcreteType::Instance(_) | ConcreteType::Func(_)) => {
                    ConcreteValType::Resource
                }
            }
        }
        ResolvedItem::Import(_, import) => {
            if let Some(type_ref) = import.get_type_refs().into_iter().next() {
                concretize_from_resolved(cx.resolve(&type_ref.ref_), comp, cx)
            } else {
                ConcreteValType::Resource
            }
        }
        ResolvedItem::InstTyDeclExport(_, decl) => {
            if let Some(type_ref) = decl.get_type_refs().into_iter().next() {
                concretize_from_resolved(cx.resolve(&type_ref.ref_), comp, cx)
            } else {
                ConcreteValType::Resource
            }
        }
        _ => ConcreteValType::Resource,
    }
}

fn concretize_comp_type_to_val<'a>(
    ty: &'a ComponentType<'a>,
    comp: &'a Component<'a>,
    cx: &ScopedVisitCtx<'a>,
) -> ConcreteValType<'a> {
    match ty {
        ComponentType::Defined(def) => concretize_defined_type(def, comp, cx),
        _ => ConcreteValType::Resource,
    }
}

fn concretize_defined_type<'a>(
    ty: &'a ComponentDefinedType,
    comp: &'a Component<'a>,
    cx: &ScopedVisitCtx<'a>,
) -> ConcreteValType<'a> {
    match ty {
        ComponentDefinedType::Primitive(p) => ConcreteValType::Primitive(*p),
        ComponentDefinedType::Record(fields) => ConcreteValType::Record(
            fields
                .iter()
                .map(|(name, ty)| (*name, Box::new(concretize_val_type(ty, comp, cx))))
                .collect(),
        ),
        ComponentDefinedType::Variant(cases) => ConcreteValType::Variant(
            cases
                .iter()
                .map(|c| {
                    (
                        c.name,
                        c.ty.as_ref()
                            .map(|t| Box::new(concretize_val_type(t, comp, cx))),
                    )
                })
                .collect(),
        ),
        ComponentDefinedType::List(ty) => {
            ConcreteValType::List(Box::new(concretize_val_type(ty, comp, cx)))
        }
        ComponentDefinedType::Tuple(types) => ConcreteValType::Tuple(
            types
                .iter()
                .map(|t| concretize_val_type(t, comp, cx))
                .collect(),
        ),
        ComponentDefinedType::Option(ty) => {
            ConcreteValType::Option(Box::new(concretize_val_type(ty, comp, cx)))
        }
        ComponentDefinedType::Result { ok, err } => ConcreteValType::Result {
            ok: ok
                .as_ref()
                .map(|t| Box::new(concretize_val_type(t, comp, cx))),
            err: err
                .as_ref()
                .map(|t| Box::new(concretize_val_type(t, comp, cx))),
        },
        ComponentDefinedType::Flags(names) => ConcreteValType::Flags(names.to_vec()),
        ComponentDefinedType::Enum(names) => ConcreteValType::Enum(names.to_vec()),
        ComponentDefinedType::Map(key, val) => ConcreteValType::Map(
            Box::new(concretize_val_type(key, comp, cx)),
            Box::new(concretize_val_type(val, comp, cx)),
        ),
        ComponentDefinedType::FixedSizeList(elem, size) => {
            ConcreteValType::FixedSizeList(Box::new(concretize_val_type(elem, comp, cx)), *size)
        }
        ComponentDefinedType::Own(_) | ComponentDefinedType::Borrow(_) => ConcreteValType::Resource,
        ComponentDefinedType::Future(_) | ComponentDefinedType::Stream(_) => {
            ConcreteValType::AsyncHandle
        }
    }
}

/// Given an `instance_index` in `comp`'s instance namespace, resolve the component being
/// instantiated and return a reference to it.
///
/// Returns `None` if the instance index is out of range, the instance is a `FromExports`
/// synthetic instance, or the component ref cannot be resolved.
fn resolve_instantiated_comp<'a>(
    comp: &'a Component<'a>,
    instance_index: u32,
) -> Option<&'a Component<'a>> {
    let inst_ref = IndexedRef {
        depth: Depth::default(),
        space: Space::CompInst,
        index: instance_index,
    };
    let inst = match comp.resolve(&inst_ref) {
        ResolvedItem::CompInst(_, inst) => inst,
        _ => return None,
    };
    let comp_ref = inst.get_comp_refs().into_iter().next()?;
    match comp.resolve(&comp_ref.ref_) {
        ResolvedItem::Component(_, nested_comp) => Some(nested_comp),
        _ => None,
    }
}
