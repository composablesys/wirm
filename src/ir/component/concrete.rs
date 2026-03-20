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
use crate::ir::component::visitor::utils::{TypeBodyDecls, VisitCtxInner};
use crate::ir::component::visitor::{ResolvedItem, VisitCtx};
use crate::Component;
use wasmparser::{
    ComponentAlias, ComponentDefinedType, ComponentExport, ComponentExternalKind, ComponentFuncType,
    ComponentInstance, ComponentType, ComponentValType, InstanceTypeDeclaration, PrimitiveValType,
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
// Public API and helpers on Component
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
            // The export resolves to a synthetic `FromExports` instance — i.e. the component
            // contains `(instance $out (export "foo" (func $f)) ...)` and then
            // `(export "name" (instance $out))`.  These instances have no associated
            // `ComponentType::Instance` node, so we build the type by following each
            // function alias to its declaration.
            ResolvedItem::CompInst(_, ComponentInstance::FromExports(exports)) => {
                concretize_from_exports_instance(self, exports)
            }
            _ => None,
        }
    }
    /// Create a [`VisitCtx`] rooted at this component for resolving refs inside a
    /// component-type body that **belongs to this component**.
    ///
    /// Used internally by [`Component::concretize_import`] and [`Component::concretize_export`]
    /// to ensure outer-alias refs (e.g. `alias outer 1 …`) inside a type body resolve against
    /// this component's own index space rather than a walk-time context.
    fn enter_type_scope(&'a self, ty: &'a ComponentType<'a>) -> VisitCtx<'a> {
        let mut inner = VisitCtxInner::new(self);
        inner.push_component(self);
        inner.maybe_enter_scope(ty);
        // Mirror what the visitor driver does: push the type body's decl slice so
        // that `resolve()` dispatches body-relative refs into the right namespace
        // rather than falling through to the component's main type index space.
        match ty {
            ComponentType::Instance(decls) => inner.push_type_body(TypeBodyDecls::Inst(decls)),
            ComponentType::Component(decls) => inner.push_type_body(TypeBodyDecls::Comp(decls)),
            _ => {}
        }
        VisitCtx { inner }
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
    cx: &VisitCtx<'a>,
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
    cx: &VisitCtx<'a>,
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
            if let Some(nested_comp) = resolve_instantiated_comp(comp, *instance_index) {
                // Instance is a locally-instantiated component — look up the export type.
                match nested_comp.concretize_export(name) {
                    Some(ConcreteType::Func(ft)) => Some(ft),
                    _ => None,
                }
            } else {
                // Instance is an import (not a local instantiation) — extract the function
                // type from the import's declared instance type.
                resolve_func_from_import_instance(comp, *instance_index, name)
            }
        }
        _ => None,
    }
}

fn concretize_func_ty<'a>(
    ft: &'a ComponentFuncType<'a>,
    comp: &'a Component<'a>,
    cx: &VisitCtx<'a>,
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
    cx: &VisitCtx<'a>,
) -> ConcreteValType<'a> {
    match ty {
        ComponentValType::Primitive(p) => ConcreteValType::Primitive(*p),
        ComponentValType::Type(_) => {
            if let Some(type_ref) = ty.get_type_refs().first() {
                concretize_from_resolved(cx.resolve(&type_ref.ref_), comp, cx)
            } else {
                unreachable!("`ComponentValType::Type(idx)` always carries exactly one type ref in a valid binary")
            }
        }
    }
}

fn concretize_from_resolved<'a>(
    resolved: ResolvedItem<'a, 'a>,
    comp: &'a Component<'a>,
    cx: &VisitCtx<'a>,
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
                // TODO(beyond-wit): `FromExports` synthetic instances and out-of-range
                // instance indices are valid in the component model but aren't backed by
                // a `Component` we can look into. Extend `resolve_instantiated_comp` to
                // handle these cases if concretization is ever needed beyond WIT.
                return ConcreteValType::Resource;
            };
            match nested_comp.concretize_export(name) {
                Some(ConcreteType::Resource) | None => ConcreteValType::Resource,
                // TODO(beyond-wit): An `InstanceExport` alias used as a *value type*
                // should only ever resolve to a resource or defined type in the WIT
                // subset — functions and instances are not value types. If you hit
                // this with a non-WIT component, extend `ConcreteValType` and add a
                // proper case here.
                Some(ConcreteType::Instance(_) | ConcreteType::Func(_)) => {
                    ConcreteValType::Resource
                }
            }
        }
        ResolvedItem::Import(_, import) => {
            if let Some(type_ref) = import.get_type_refs().into_iter().next() {
                concretize_from_resolved(cx.resolve(&type_ref.ref_), comp, cx)
            } else {
                // TODO(beyond-wit): In WIT, an import used as a val type always carries
                // a type ref. Module imports have no type refs but can't appear as val
                // types. Audit this if concretizing non-WIT component imports.
                ConcreteValType::Resource
            }
        }
        ResolvedItem::InstTyDeclExport(_, decl) => {
            if let Some(type_ref) = decl.get_type_refs().into_iter().next() {
                concretize_from_resolved(cx.resolve(&type_ref.ref_), comp, cx)
            } else {
                // TODO(beyond-wit): Same as the Import case above — no type ref on a
                // decl used as a val type shouldn't arise in WIT. Audit for non-WIT use.
                ConcreteValType::Resource
            }
        }
        // TODO(beyond-wit): All other `ResolvedItem` variants (`CompInst`, `Component`,
        // `Module`, `CoreType`, etc.) cannot appear as val types in a valid WIT binary.
        // If you extend concretization beyond WIT, audit every variant of `ResolvedItem`
        // and add explicit arms for any that can legitimately carry a val type.
        _ => ConcreteValType::Resource,
    }
}

fn concretize_comp_type_to_val<'a>(
    ty: &'a ComponentType<'a>,
    comp: &'a Component<'a>,
    cx: &VisitCtx<'a>,
) -> ConcreteValType<'a> {
    match ty {
        ComponentType::Defined(def) => concretize_defined_type(def, comp, cx),
        // `ComponentType::Resource` legitimately maps to `ConcreteValType::Resource`.
        // TODO(beyond-wit): `Func`, `Instance`, and `Component` variants here indicate
        // a type reference that resolved to a non-value type, which shouldn't happen in
        // a valid WIT binary. Add explicit handling if concretizing non-WIT components.
        _ => ConcreteValType::Resource,
    }
}

fn concretize_defined_type<'a>(
    ty: &'a ComponentDefinedType,
    comp: &'a Component<'a>,
    cx: &VisitCtx<'a>,
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

/// Concretize a `FromExports` synthetic instance into a [`ConcreteType::Instance`].
///
/// Handles the case where a component export resolves to an instance built with
/// `(instance $out (export "name" (func $f)) ...)` rather than a typed instance import.
/// Each `Func` export in the instance is resolved to its concrete signature by following
/// the alias chain to the underlying function type declaration.
fn concretize_from_exports_instance<'a>(
    comp: &'a Component<'a>,
    exports: &'a [ComponentExport<'a>],
) -> Option<ConcreteType<'a>> {
    // Build a root-level context for resolving aliases in the component's own namespace.
    let cx = {
        let mut inner = VisitCtxInner::new(comp);
        inner.push_component(comp);
        VisitCtx { inner }
    };

    let mut funcs = vec![];
    for export in exports.iter() {
        if export.kind != ComponentExternalKind::Func {
            continue; // Skip non-function exports (nested instances, types, etc.)
        }
        let func_ref = IndexedRef {
            depth: Depth::default(),
            space: Space::CompFunc,
            index: export.index,
        };
        let resolved = comp.resolve(&func_ref);
        if let Some(ft) = resolve_and_concretize_func(resolved, comp, &cx) {
            funcs.push((export.name.0, ft));
        }
    }

    Some(ConcreteType::Instance(funcs))
}

/// Follow a function alias that points into an **import** instance (not a locally-instantiated
/// component).
///
/// When `(alias export $import-inst "func-name" (func $f))` appears inside a component and
/// `$import-inst` was provided as an import (rather than instantiated locally), we cannot
/// reach its type via [`resolve_instantiated_comp`].  Instead, we look up the import's
/// declared instance type and extract the named function signature from it.
fn resolve_func_from_import_instance<'a>(
    comp: &'a Component<'a>,
    instance_index: u32,
    func_name: &str,
) -> Option<ConcreteFuncType<'a>> {
    let inst_ref = IndexedRef {
        depth: Depth::default(),
        space: Space::CompInst,
        index: instance_index,
    };
    let import = match comp.resolve(&inst_ref) {
        ResolvedItem::Import(_, imp) => imp,
        _ => return None,
    };

    // The import's type must be ComponentTypeRef::Instance.
    let type_ref = import.get_type_refs().into_iter().next()?;
    let ty = match comp.resolve(&type_ref.ref_) {
        ResolvedItem::CompType(_, ty) => ty,
        _ => return None,
    };

    // Concretize the full instance type and find the named function.
    match concretize_comp_type(comp, ty)? {
        ConcreteType::Instance(funcs) => funcs
            .into_iter()
            .find(|(name, _)| *name == func_name)
            .map(|(_, ft)| ft),
        _ => None,
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
