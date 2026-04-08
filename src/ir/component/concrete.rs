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
use std::collections::HashMap;
use wasmparser::{
    ComponentAlias, ComponentDefinedType, ComponentExport, ComponentExternalKind,
    ComponentFuncType, ComponentInstance, ComponentType, ComponentTypeRef, ComponentValType,
    InstanceTypeDeclaration, PrimitiveValType, TypeBounds,
};
// ============================================================
// Public output types
// ============================================================

/// A fully-resolved component type with no remaining index references.
///
/// Produced by [`Component::concretize_import`] and [`Component::concretize_export`].
#[derive(Debug, Clone)]
pub enum ConcreteType<'a> {
    /// A WIT instance interface — exported functions and named type exports.
    ///
    /// `funcs`: `(function_name, signature)` pairs for each exported function.
    /// `type_exports`: `(export_name, concrete_val_type)` pairs for named type
    /// exports (records, variants, resources exported with `(type (eq N))` or
    /// `(type (sub resource))` bounds).  These are needed by proxy generators
    /// that must re-export the same types from a types-instance import.
    Instance {
        funcs: Vec<(&'a str, ConcreteFuncType<'a>)>,
        type_exports: Vec<(&'a str, ConcreteValType<'a>)>,
    },
    /// A single function type.
    Func(ConcreteFuncType<'a>),
    /// A resource (own or borrow handle).
    Resource,
}

/// A fully-resolved function signature with no index references.
#[derive(Debug, Clone)]
pub struct ConcreteFuncType<'a> {
    /// Whether this is an `async` function.
    pub is_async: bool,
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
    /// A resource handle whose export name is known (from an instance-type scope).
    ///
    /// Produced when concretizing instance types that explicitly export named resource
    /// types (e.g. `wasi:http/handler` exporting `"request"` and `"response"`).
    /// The `&str` is the export name of the resource within the instance type.
    NamedResource(&'a str),
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
            ResolvedItem::CompInst(_, ComponentInstance::FromExports(exports)) => {
                concretize_from_exports_instance(self, exports)
            }
            // The export resolves to a real instantiated component (e.g. a wit-component shim
            // that re-exports individual functions under short names like "handle").
            //
            // Strategy: if the outer component imports an interface under the *same name*
            // (the common wit-component shim pattern where the middleware imports and then
            // The export is a real instantiated component (e.g. a wit-component shim).
            // Prefer reconstructing from the nested component's exports (which preserves
            // resource names via build_component_resource_map).  Fall back to the import's
            // declared type when the nested component can't be resolved.
            ResolvedItem::CompInst(_, inst @ ComponentInstance::Instantiate { .. }) => {
                let comp_result = {
                    let comp_ref = inst.get_comp_refs().into_iter().next();
                    comp_ref.and_then(|cr| match self.resolve(&cr.ref_) {
                        ResolvedItem::Component(_, nested) => {
                            concretize_comp_func_exports(nested)
                        }
                        _ => None,
                    })
                };
                comp_result.or_else(|| self.concretize_import(name))
            }
            // The export directly re-exposes an imported instance (pass-through middleware).
            // Concretize by following the import's declared instance type.
            ResolvedItem::Import(_, imp) => {
                let type_ref = imp.get_type_refs().into_iter().next()?;
                let ty = match self.resolve(&type_ref.ref_) {
                    ResolvedItem::CompType(_, ty) => ty,
                    _ => return None,
                };
                concretize_comp_type(self, ty)
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
            let d = concretize_instance_decls(comp, decls, &cx);
            Some(ConcreteType::Instance {
                funcs: d.funcs,
                type_exports: d.type_exports,
            })
        }
        ComponentType::Func(ft) => {
            let cx = comp.enter_type_scope(ty);
            Some(ConcreteType::Func(concretize_func_ty(ft, comp, &cx, &HashMap::new())))
        }
        ComponentType::Resource { .. } => Some(ConcreteType::Resource),
        _ => None,
    }
}

/// Build a map from resource-type-local-index → resource export name for an instance type.
///
/// Within an `InstanceType`, resource types appear as:
/// ```text
/// (export "request" (type (sub resource)))   -- creates type N (a resource)
/// (type (own N))                              -- creates type N+1 = own<resource>
/// ```
/// This function scans the declarations in order, tracking the sequential type index,
/// and returns a map of `N → "request"` so that when `concretize_defined_type` sees
/// `ComponentDefinedType::Own(N)`, it can return `ConcreteValType::NamedResource("request")`
/// instead of the anonymous `ConcreteValType::Resource`.
fn build_instance_resource_map<'a>(
    decls: &'a [InstanceTypeDeclaration<'a>],
) -> HashMap<u32, &'a str> {
    // resource_by_idx: type_idx → export name, for types declared as SubResource
    let mut resource_by_idx: HashMap<u32, &'a str> = HashMap::new();
    let mut type_count: u32 = 0;

    for decl in decls {
        match decl {
            // Export with SubResource creates a new type AND records its name.
            InstanceTypeDeclaration::Export {
                name,
                ty: ComponentTypeRef::Type(TypeBounds::SubResource),
            } => {
                resource_by_idx.insert(type_count, name.0);
                type_count += 1;
            }
            // Other type exports (Eq) also create a new type entry.
            InstanceTypeDeclaration::Export {
                ty: ComponentTypeRef::Type(_),
                ..
            } => {
                type_count += 1;
            }
            // Func exports do NOT create a new type entry.
            InstanceTypeDeclaration::Export {
                ty: ComponentTypeRef::Func(_),
                ..
            } => {}
            // All Type declarations create a new type entry.
            InstanceTypeDeclaration::Type(_) => {
                type_count += 1;
            }
            // Alias declarations (e.g. `(alias outer 1 N (type))`) create type entries.
            InstanceTypeDeclaration::Alias(_) => {
                type_count += 1;
            }
            // CoreType doesn't create component-level type entries.
            _ => {}
        }
    }

    resource_by_idx
}

/// Build a map from resource-type-index → export name for a **component** body.
///
/// Unlike [`build_instance_resource_map`] (which handles inline `InstanceType` declarations),
/// this function handles the "shim component" pattern produced by `wit-component`, where
/// resources are imported as `(type (sub resource))` and then re-exported with a name:
/// ```text
/// (import "import-type-request" (type (sub resource)))  -- type 0
/// ...
/// (export "request" (type 0))  -- re-exports type 0, creating alias type N
/// ```
/// Returns `{ N → "request" }` so that `concretize_defined_type(Own(N))` can look
/// up the resource name.
fn build_component_resource_map<'a>(
    comp: &'a Component<'a>,
    cx: &VisitCtx<'a>,
) -> HashMap<u32, &'a str> {
    let mut map: HashMap<u32, &'a str> = HashMap::new();
    for export in comp.exports.iter() {
        if export.kind != ComponentExternalKind::Type {
            continue;
        }
        let type_ref = IndexedRef {
            depth: Depth::default(),
            space: Space::CompType,
            index: export.index,
        };
        let is_res = resolved_is_resource(cx.resolve(&type_ref), comp, cx, 0);
        eprintln!(
            "[wirm debug] build_component_resource_map: export '{}' index={} is_resource={}",
            export.name.0, export.index, is_res
        );
        if is_res {
            map.insert(export.index, export.name.0);
        }
    }
    map
}

/// Returns `true` if `resolved` ultimately resolves to a sub-resource type.
fn resolved_is_resource<'a>(
    resolved: ResolvedItem<'a, '_>,
    comp: &'a Component<'a>,
    cx: &VisitCtx<'a>,
    depth: u32,
) -> bool {
    if depth > 16 {
        return false; // guard against infinite alias chains
    }
    match resolved {
        ResolvedItem::CompType(_, ComponentType::Resource { .. }) => true,
        ResolvedItem::Import(_, imp) => {
            matches!(imp.ty, ComponentTypeRef::Type(TypeBounds::SubResource))
        }
        ResolvedItem::Alias(_, alias) => {
            resolved_is_resource(cx.resolve(&alias.get_item_ref().ref_), comp, cx, depth + 1)
        }
        _ => false,
    }
}

/// Return value from [`concretize_instance_decls`]: function exports and named type exports.
struct ConcreteInstanceDecls<'a> {
    funcs: Vec<(&'a str, ConcreteFuncType<'a>)>,
    type_exports: Vec<(&'a str, ConcreteValType<'a>)>,
}

fn concretize_instance_decls<'a>(
    comp: &'a Component<'a>,
    decls: &'a [InstanceTypeDeclaration<'a>],
    cx: &VisitCtx<'a>,
) -> ConcreteInstanceDecls<'a> {
    // Build a map of own-type-local-idx → resource-name for named resource types.
    let resource_map = build_instance_resource_map(decls);

    let mut funcs = vec![];
    let mut type_exports = vec![];
    for decl in decls {
        if let InstanceTypeDeclaration::Export { name, ty, .. } = decl {
            if let Some(type_ref) = decl.get_type_refs().first() {
                let resolved = cx.resolve(&type_ref.ref_);
                if let Some(ft) = resolve_and_concretize_func(resolved, comp, cx, &resource_map) {
                    funcs.push((name.0, ft));
                } else {
                    // Not a function export — check if it's a type export.
                    match ty {
                        ComponentTypeRef::Type(TypeBounds::SubResource) => {
                            type_exports.push((name.0, ConcreteValType::NamedResource(name.0)));
                        }
                        ComponentTypeRef::Type(TypeBounds::Eq(_)) => {
                            // Re-resolve (first resolve was consumed by resolve_and_concretize_func).
                            let resolved2 = cx.resolve(&type_ref.ref_);
                            if let Some(cvt) = concretize_from_resolved_to_val(
                                resolved2, comp, cx, &resource_map,
                            ) {
                                type_exports.push((name.0, cvt));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    ConcreteInstanceDecls {
        funcs,
        type_exports,
    }
}

/// Try to concretize a resolved item as a value type (not a function).
fn concretize_from_resolved_to_val<'a>(
    resolved: ResolvedItem<'a, 'a>,
    comp: &'a Component<'a>,
    cx: &VisitCtx<'a>,
    resource_map: &HashMap<u32, &'a str>,
) -> Option<ConcreteValType<'a>> {
    match resolved {
        ResolvedItem::CompType(_, ComponentType::Defined(dt)) => {
            Some(concretize_defined_type(dt, comp, cx, resource_map))
        }
        ResolvedItem::CompType(_, ComponentType::Resource { .. }) => {
            Some(ConcreteValType::Resource)
        }
        // Follow import's type refs (handles eq-bound aliases like error-code).
        ResolvedItem::Import(_, imp) => {
            for tr in imp.get_type_refs() {
                let inner = comp.resolve(&tr.ref_);
                if let Some(cvt) = concretize_from_resolved_to_val(inner, comp, cx, resource_map) {
                    return Some(cvt);
                }
            }
            None
        }
        _ => None,
    }
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
///
/// `resource_map` maps `own<T>` local-type-indices to resource export names within
/// the current instance-type scope; pass an empty map when outside an instance type.
fn resolve_and_concretize_func<'a>(
    resolved: ResolvedItem<'a, 'a>,
    comp: &'a Component<'a>,
    cx: &VisitCtx<'a>,
    resource_map: &HashMap<u32, &'a str>,
) -> Option<ConcreteFuncType<'a>> {
    match resolved {
        ResolvedItem::CompType(_, ComponentType::Func(ft)) => {
            Some(concretize_func_ty(ft, comp, cx, resource_map))
        }
        ResolvedItem::Alias(_, alias @ ComponentAlias::Outer { .. }) => {
            resolve_and_concretize_func(cx.resolve(&alias.get_item_ref().ref_), comp, cx, resource_map)
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
        // The function is a direct import (e.g. `(import "f" (func (type $sig)))`).
        // This arises in shim components that take individual function imports rather
        // than a whole instance import.  Follow the import's declared type.
        // Pass the resource_map through so that own<T> types can be resolved to
        // named resources within the current component scope.
        ResolvedItem::Import(_, imp) => {
            let type_ref = imp.get_type_refs().into_iter().next()?;
            match comp.resolve(&type_ref.ref_) {
                ResolvedItem::CompType(_, ComponentType::Func(ft)) => {
                    Some(concretize_func_ty(ft, comp, cx, resource_map))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn concretize_func_ty<'a>(
    ft: &'a ComponentFuncType<'a>,
    comp: &'a Component<'a>,
    cx: &VisitCtx<'a>,
    resource_map: &HashMap<u32, &'a str>,
) -> ConcreteFuncType<'a> {
    ConcreteFuncType {
        is_async: ft.async_,
        params: ft
            .params
            .iter()
            .map(|(name, ty)| (*name, concretize_val_type(ty, comp, cx, resource_map)))
            .collect(),
        result: ft
            .result
            .as_ref()
            .map(|ty| concretize_val_type(ty, comp, cx, resource_map)),
    }
}

fn concretize_val_type<'a>(
    ty: &'a ComponentValType,
    comp: &'a Component<'a>,
    cx: &VisitCtx<'a>,
    resource_map: &HashMap<u32, &'a str>,
) -> ConcreteValType<'a> {
    match ty {
        ComponentValType::Primitive(p) => ConcreteValType::Primitive(*p),
        ComponentValType::Type(_) => {
            if let Some(type_ref) = ty.get_type_refs().first() {
                concretize_from_resolved(cx.resolve(&type_ref.ref_), comp, cx, resource_map)
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
    resource_map: &HashMap<u32, &'a str>,
) -> ConcreteValType<'a> {
    match resolved {
        ResolvedItem::CompType(_, ty) => concretize_comp_type_to_val(ty, comp, cx, resource_map),
        ResolvedItem::Alias(_, alias @ ComponentAlias::Outer { .. }) => {
            concretize_from_resolved(cx.resolve(&alias.get_item_ref().ref_), comp, cx, resource_map)
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
                // The instance is an import (e.g. `wasi:http/types@...`) rather than a
                // locally-instantiated component.  Look up the named type export from
                // the import's declared instance type — the same approach used by
                // `resolve_func_from_import_instance` for function aliases.
                return resolve_type_from_import_instance(comp, *instance_index, name);
            };
            match nested_comp.concretize_export(name) {
                Some(ConcreteType::Resource) | None => ConcreteValType::Resource,
                // TODO(beyond-wit): An `InstanceExport` alias used as a *value type*
                // should only ever resolve to a resource or defined type in the WIT
                // subset — functions and instances are not value types. If you hit
                // this with a non-WIT component, extend `ConcreteValType` and add a
                // proper case here.
                Some(ConcreteType::Instance { .. } | ConcreteType::Func(_)) => {
                    ConcreteValType::Resource
                }
            }
        }
        ResolvedItem::Import(_, import) => {
            if let Some(type_ref) = import.get_type_refs().into_iter().next() {
                concretize_from_resolved(cx.resolve(&type_ref.ref_), comp, cx, resource_map)
            } else {
                // TODO(beyond-wit): In WIT, an import used as a val type always carries
                // a type ref. Module imports have no type refs but can't appear as val
                // types. Audit this if concretizing non-WIT component imports.
                ConcreteValType::Resource
            }
        }
        ResolvedItem::InstTyDeclExport(_, decl) => {
            if let Some(type_ref) = decl.get_type_refs().into_iter().next() {
                concretize_from_resolved(cx.resolve(&type_ref.ref_), comp, cx, resource_map)
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
    resource_map: &HashMap<u32, &'a str>,
) -> ConcreteValType<'a> {
    match ty {
        ComponentType::Defined(def) => concretize_defined_type(def, comp, cx, resource_map),
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
    resource_map: &HashMap<u32, &'a str>,
) -> ConcreteValType<'a> {
    match ty {
        ComponentDefinedType::Primitive(p) => ConcreteValType::Primitive(*p),
        ComponentDefinedType::Record(fields) => ConcreteValType::Record(
            fields
                .iter()
                .map(|(name, ty)| (*name, Box::new(concretize_val_type(ty, comp, cx, resource_map))))
                .collect(),
        ),
        ComponentDefinedType::Variant(cases) => ConcreteValType::Variant(
            cases
                .iter()
                .map(|c| {
                    (
                        c.name,
                        c.ty.as_ref()
                            .map(|t| Box::new(concretize_val_type(t, comp, cx, resource_map))),
                    )
                })
                .collect(),
        ),
        ComponentDefinedType::List(ty) => {
            ConcreteValType::List(Box::new(concretize_val_type(ty, comp, cx, resource_map)))
        }
        ComponentDefinedType::Tuple(types) => ConcreteValType::Tuple(
            types
                .iter()
                .map(|t| concretize_val_type(t, comp, cx, resource_map))
                .collect(),
        ),
        ComponentDefinedType::Option(ty) => {
            ConcreteValType::Option(Box::new(concretize_val_type(ty, comp, cx, resource_map)))
        }
        ComponentDefinedType::Result { ok, err } => ConcreteValType::Result {
            ok: ok
                .as_ref()
                .map(|t| Box::new(concretize_val_type(t, comp, cx, resource_map))),
            err: err
                .as_ref()
                .map(|t| Box::new(concretize_val_type(t, comp, cx, resource_map))),
        },
        ComponentDefinedType::Flags(names) => ConcreteValType::Flags(names.to_vec()),
        ComponentDefinedType::Enum(names) => ConcreteValType::Enum(names.to_vec()),
        ComponentDefinedType::Map(key, val) => ConcreteValType::Map(
            Box::new(concretize_val_type(key, comp, cx, resource_map)),
            Box::new(concretize_val_type(val, comp, cx, resource_map)),
        ),
        ComponentDefinedType::FixedSizeList(elem, size) => {
            ConcreteValType::FixedSizeList(Box::new(concretize_val_type(elem, comp, cx, resource_map)), *size)
        }
        ComponentDefinedType::Own(res_idx) => {
            // Look up the resource name via the resource_map (keyed by resource-type-idx).
            // Direct lookup first.
            if let Some(&name) = resource_map.get(res_idx) {
                ConcreteValType::NamedResource(name)
            } else {
                // The own's target index might be an alias (outer or eq-bound) of
                // a resource that IS in the map, or a resource type in a parent scope.
                // Try multiple resolution strategies.
                let type_ref = IndexedRef {
                    depth: Depth::default(),
                    space: Space::CompType,
                    index: *res_idx,
                };
                let resolved = cx.resolve(&type_ref);
                // Try to follow import eq-bound aliases to find the resource name.
                let found_name = match resolved {
                    ResolvedItem::Import(_, imp) => {
                        imp.get_type_refs()
                            .iter()
                            .find_map(|tr| resource_map.get(&tr.ref_.index).copied())
                    }
                    _ => None,
                };
                if let Some(name) = found_name {
                    ConcreteValType::NamedResource(name)
                } else {
                    ConcreteValType::Resource
                }
            }
        }
        ComponentDefinedType::Borrow(_) => ConcreteValType::Resource,
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

    let resource_map = build_component_resource_map(comp, &cx);

    let mut funcs = vec![];
    for export in exports.iter() {
        if export.kind != ComponentExternalKind::Func {
            continue; // Skip non-function exports (nested instances, types, etc.)
        }
        let resolved = comp.resolve(&export.get_item_ref().ref_);
        if let Some(ft) = resolve_and_concretize_func(resolved, comp, &cx, &resource_map) {
            funcs.push((export.name.0, ft));
        }
    }

    Some(ConcreteType::Instance {
        funcs,
        type_exports: vec![],
    })
}

/// Concretize the "interface" of a real instantiated component by collecting all of its
/// function exports into a [`ConcreteType::Instance`].
///
/// This handles the pattern produced by `wit-component` where a shim component re-exports
/// individual functions (`"handle"`, etc.) rather than bundling them under a WIT interface
/// name.  When the outer component exports the whole shim instance under an interface name
/// (e.g. `"wasi:http/handler@..."`), the type of that interface is implicitly defined by the
/// shim's function exports.
fn concretize_comp_func_exports<'a>(comp: &'a Component<'a>) -> Option<ConcreteType<'a>> {
    let cx = {
        let mut inner = VisitCtxInner::new(comp);
        inner.push_component(comp);
        VisitCtx { inner }
    };

    // Build a resource map from the component's type exports so that resources used
    // as function parameters/results can be named (e.g. "request", "response").
    let resource_map = build_component_resource_map(comp, &cx);

    let mut funcs = vec![];
    let mut type_exports = vec![];
    for export in comp.exports.iter() {
        match export.kind {
            ComponentExternalKind::Func => {
                let resolved = comp.resolve(&export.get_item_ref().ref_);
                if let Some(ft) =
                    resolve_and_concretize_func(resolved, comp, &cx, &resource_map)
                {
                    funcs.push((export.name.0, ft));
                }
            }
            ComponentExternalKind::Type => {
                let type_ref = IndexedRef {
                    depth: Depth::default(),
                    space: Space::CompType,
                    index: export.index,
                };
                let resolved = cx.resolve(&type_ref);
                if resolved_is_resource(cx.resolve(&type_ref), comp, &cx, 0) {
                    // Use NamedResource so the vid matches function param resource vids.
                    type_exports.push((export.name.0, ConcreteValType::NamedResource(export.name.0)));
                } else if let Some(cvt) =
                    concretize_from_resolved_to_val(resolved, comp, &cx, &resource_map)
                {
                    type_exports.push((export.name.0, cvt));
                }
            }
            _ => {}
        }
    }

    Some(ConcreteType::Instance {
        funcs,
        type_exports,
    })
}

/// Resolve a **type** exported by an **import** instance (not a locally-instantiated component).
///
/// When `(alias export $import-inst "type-name" (type $t))` appears inside a component and
/// `$import-inst` is an import (e.g. `wasi:http/types@...`), [`resolve_instantiated_comp`]
/// returns `None`.  This function looks up the import's declared instance type, enters its
/// type-body scope, and concretizes the named type export from the declarations.
///
/// This is the value-type counterpart of [`resolve_func_from_import_instance`].
fn resolve_type_from_import_instance<'a>(
    comp: &'a Component<'a>,
    instance_index: u32,
    type_name: &str,
) -> ConcreteValType<'a> {
    let inst_ref = IndexedRef {
        depth: Depth::default(),
        space: Space::CompInst,
        index: instance_index,
    };
    let import = match comp.resolve(&inst_ref) {
        ResolvedItem::Import(_, imp) => imp,
        _ => return ConcreteValType::Resource,
    };
    let type_ref = match import.get_type_refs().into_iter().next() {
        Some(tr) => tr,
        None => return ConcreteValType::Resource,
    };
    let ty = match comp.resolve(&type_ref.ref_) {
        ResolvedItem::CompType(_, ty) => ty,
        _ => return ConcreteValType::Resource,
    };
    let decls = match ty {
        ComponentType::Instance(decls) => decls,
        _ => return ConcreteValType::Resource,
    };
    // Build a type-body scope so that outer-alias refs inside the decls resolve
    // against the component's own type space (same as `enter_type_scope`).
    let inner_cx = comp.enter_type_scope(ty);
    // Build the resource map from the instance type declarations so that
    // resource names (like "request", "response") are preserved.
    let resource_map = build_instance_resource_map(decls);
    for decl in decls {
        if let InstanceTypeDeclaration::Export { name, .. } = decl {
            if name.0 != type_name {
                continue;
            }
            if let Some(tr) = decl.get_type_refs().first() {
                let resolved = inner_cx.resolve(&tr.ref_);
                return concretize_from_resolved(resolved, comp, &inner_cx, &resource_map);
            }
        }
    }
    ConcreteValType::Resource
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
        ConcreteType::Instance { funcs, .. } => funcs
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
