//! Resolution-correctness tests for `walk_structural` and `walk_topological`.
//!
//! These tests exercise the resolution layer (`cx.resolve`) rather than event
//! generation.  They exist to catch:
//!
//!   - `comp_at` depth/scope-stack misalignment inside type scopes
//!     (`type_scope_nesting` accounting).
//!   - Driver bugs where `type_body_stack` is not pushed/popped correctly,
//!     causing "No index for assumed ID" panics inside type-body decl callbacks.
//!   - `type_scope_nesting` and `type_body_stack` failing to reset correctly
//!     after scope exit.
//!
//! **Layer 1** — paranoid "resolve everything" tests: run a visitor that calls
//! `scoped.resolve()` for every ref on every type-body decl.  Resolution bugs
//! surface as panics, catching regressions without requiring knowledge of
//! exact IDs.
//!
//! **Layer 2** — specific result assertions: small, targeted WAT components
//! where the resolved `ResolvedItem` variant is checked precisely.

use crate::ir::component::refs::{RefKind, ReferencedIndices};
use crate::ir::component::visitor::{
    walk_structural, walk_topological, ComponentVisitor, ItemKind, ResolvedItem, VisitCtx,
};
use crate::ir::types::CustomSection;
use crate::{Component, Module};
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance,
    ComponentStartFunction, ComponentType, ComponentTypeDeclaration, CoreType, Instance,
    InstanceTypeDeclaration, ModuleTypeDeclaration, SubType,
};
// ============================================================
// Paranoid visitor
// ============================================================

/// Visits every type-body declaration and resolves all its refs using the
/// `VisitCtx` the driver provides directly.  Any resolution bug
/// (comp_at overflow, wrong nesting count, missing type_body_stack push, …)
/// surfaces as a panic.
#[derive(Default)]
struct ParanoidVisitor {
    resolved_count: usize,
}
impl ParanoidVisitor {
    fn resolve_refs<T>(&mut self, cx: &VisitCtx, item: &T)
    where
        T: ReferencedIndices,
    {
        for r in item.referenced_indices() {
            let _ = cx.resolve(&r.ref_);
            self.resolved_count += 1;
        }
    }
}
impl<'a> ComponentVisitor<'a> for ParanoidVisitor {
    fn visit_module(&mut self, cx: &VisitCtx<'a>, _: u32, item: &Module<'a>) {
        self.resolve_refs(cx, item);
    }
    fn visit_comp_type(&mut self, cx: &VisitCtx<'a>, _id: u32, item: &ComponentType<'a>) {
        self.resolve_refs(cx, item);
    }
    fn visit_comp_type_decl(
        &mut self,
        cx: &VisitCtx<'a>,
        _decl_idx: usize,
        _id: u32,
        _parent: &ComponentType<'a>,
        item: &ComponentTypeDeclaration<'a>,
    ) {
        self.resolve_refs(cx, item);
    }

    fn visit_inst_type_decl(
        &mut self,
        cx: &VisitCtx<'a>,
        _decl_idx: usize,
        _id: u32,
        _parent: &ComponentType<'a>,
        decl: &InstanceTypeDeclaration<'a>,
    ) {
        self.resolve_refs(cx, decl);
    }
    // These four are intentionally no-ops. `ComponentType::Instance` and
    // `ComponentType::Component` carry no refs of their own — their
    // `ReferencedIndices` impl just iterates over every inner decl. Resolving
    // here would double-count every ref that `visit_inst_type_decl` /
    // `visit_comp_type_decl` already handles individually.
    fn enter_component_type_inst(
        &mut self,
        _cx: &VisitCtx<'a>,
        _id: u32,
        _item: &ComponentType<'a>,
    ) {
    }
    fn exit_component_type_inst(
        &mut self,
        _cx: &VisitCtx<'a>,
        _id: u32,
        _item: &ComponentType<'a>,
    ) {
    }
    fn enter_component_type_comp(
        &mut self,
        _cx: &VisitCtx<'a>,
        _id: u32,
        _item: &ComponentType<'a>,
    ) {
    }
    fn exit_component_type_comp(
        &mut self,
        _cx: &VisitCtx<'a>,
        _id: u32,
        _item: &ComponentType<'a>,
    ) {
    }
    fn visit_comp_instance(&mut self, cx: &VisitCtx<'a>, _id: u32, item: &ComponentInstance<'a>) {
        self.resolve_refs(cx, item);
    }
    fn visit_canon(
        &mut self,
        cx: &VisitCtx<'a>,
        _kind: ItemKind,
        _id: u32,
        item: &CanonicalFunction,
    ) {
        self.resolve_refs(cx, item);
    }
    fn visit_alias(
        &mut self,
        cx: &VisitCtx<'a>,
        _kind: ItemKind,
        _id: u32,
        item: &ComponentAlias<'a>,
    ) {
        self.resolve_refs(cx, item);
    }
    fn visit_comp_import(
        &mut self,
        cx: &VisitCtx<'a>,
        _kind: ItemKind,
        _id: u32,
        item: &ComponentImport<'a>,
    ) {
        self.resolve_refs(cx, item);
    }
    fn visit_comp_export(
        &mut self,
        cx: &VisitCtx<'a>,
        _kind: ItemKind,
        _id: u32,
        item: &ComponentExport<'a>,
    ) {
        self.resolve_refs(cx, item);
    }

    fn enter_core_rec_group(&mut self, cx: &VisitCtx<'a>, _count: usize, item: &CoreType<'a>) {
        self.resolve_refs(cx, item);
    }
    fn visit_core_subtype(&mut self, cx: &VisitCtx<'a>, _id: u32, item: &SubType) {
        self.resolve_refs(cx, item);
    }

    // No-op for the same reason as the comp body callbacks above: `CoreType::Module`
    // has no refs beyond its decl list, which `visit_module_type_decl` already covers.
    fn enter_core_module_type(&mut self, _cx: &VisitCtx<'a>, _id: u32, _item: &CoreType<'a>) {}
    fn visit_module_type_decl(
        &mut self,
        cx: &VisitCtx<'a>,
        _decl_idx: usize,
        _id: u32,
        _parent: &CoreType<'a>,
        decl: &ModuleTypeDeclaration<'a>,
    ) {
        self.resolve_refs(cx, decl);
    }
    fn exit_core_module_type(&mut self, cx: &VisitCtx<'a>, _id: u32, item: &CoreType<'a>) {
        self.resolve_refs(cx, item);
    }
    fn visit_core_instance(&mut self, cx: &VisitCtx<'a>, _id: u32, item: &Instance<'a>) {
        self.resolve_refs(cx, item);
    }
    fn visit_custom_section(&mut self, cx: &VisitCtx<'a>, item: &CustomSection<'a>) {
        self.resolve_refs(cx, item);
    }
    fn visit_start_section(&mut self, cx: &VisitCtx<'a>, item: &ComponentStartFunction) {
        self.resolve_refs(cx, item);
    }
}

fn run_on_bytes(bytes: Vec<u8>) -> usize {
    let comp = Component::parse(&bytes, false, false, false).expect("component parse failed");

    let mut structural = ParanoidVisitor::default();
    walk_structural(&comp, &mut structural);

    let mut topological = ParanoidVisitor::default();
    walk_topological(&comp, &mut topological);

    assert_eq!(
        structural.resolved_count, topological.resolved_count,
        "structural and topological walks resolved a different number of refs"
    );
    structural.resolved_count
}

/// Run the paranoid visitor under both walkers and assert they resolve the
/// same number of refs (a consistency check on top of the no-panic check).
/// Returns the resolved ref count.
fn run_paranoid(wat: &str) -> usize {
    run_on_bytes(wat::parse_str(wat).expect("WAT parse failed"))
}

fn run_paranoid_file(path: &str) {
    let bytes = wat::parse_file(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    run_on_bytes(bytes);
}

// ============================================================
// Layer 2 helpers
// ============================================================

/// Parse `wat` and run `walk_structural` with `visitor`.
fn walk_wat<V: for<'a> ComponentVisitor<'a>>(wat: &str, visitor: &mut V) {
    let bytes = wat::parse_str(wat).expect("WAT parse failed");
    let comp = Component::parse(&bytes, false, false, false).expect("component parse failed");
    walk_structural(&comp, visitor);
}

/// Resolve every ref in `refs` using `cx`, assert each satisfies `pred`, return the count.
///
/// Called from within a `ComponentVisitor<'a>` impl where `'a` is already concrete,
/// so no higher-ranked lifetime bound is needed here.
fn check_refs<'a>(
    cx: &VisitCtx<'a>,
    refs: Vec<RefKind>,
    pred: impl Fn(&ResolvedItem<'a, 'a>) -> bool,
    msg: &'static str,
) -> usize {
    let mut n = 0;
    for r in refs {
        let resolved = cx.resolve(&r.ref_);
        assert!(pred(&resolved), "{msg}: got {resolved:?}");
        n += 1;
    }
    n
}

// ============================================================
// Layer 1 — regression net over existing fixture files
// ============================================================

/// Runs the paranoid visitor over all handwritten fixture components to catch
/// regressions.
#[test]
fn test_scoped_resolution_no_panic_fixture_files() {
    let fixtures = [
        "./tests/test_inputs/handwritten/components/add.wat",
        "./tests/test_inputs/handwritten/components/mul_mod.wat",
        "./tests/test_inputs/dfinity/components/exports.wat",
        "./tests/test_inputs/dfinity/components/func.wat",
        "./tests/test_inputs/dfinity/components/func_locals.wat",
        "./tests/test_inputs/spin/hello_world.wat",
    ];
    for path in fixtures {
        run_paranoid_file(path);
    }
}

// ============================================================
// Layer 1 — targeted WATs for specific bug scenarios
// ============================================================

/// Instance type body with a type and an export that references it.
///
/// Previously panicked with "No index for assumed ID" because `cx.resolve()`
/// was called without entering the type scope first.
#[test]
fn test_scoped_resolution_instance_type_body() {
    let count = run_paranoid(
        r#"(component
          (type (instance
            (type $t u32)
            (export "n" (type (eq $t)))
          ))
        )"#,
    );
    assert_eq!(count, 1);
}

/// Instance type body with a compound type (list) that references a sibling
/// type in the same scope.
#[test]
fn test_scoped_resolution_instance_type_compound_ref() {
    let count = run_paranoid(
        r#"(component
          (type (instance
            (type $item u8)
            (type $list (list $item))
            (export "data" (type (eq $list)))
          ))
        )"#,
    );
    assert_eq!(count, 2);
}

/// Component type body (not instance type) with a type and an export ref.
/// Exercises `enter_comp_ty_scope` for `ComponentType::Component`.
#[test]
fn test_scoped_resolution_component_type_body() {
    let count = run_paranoid(
        r#"(component
          (type (component
            (type $t u32)
            (export "value" (type (eq $t)))
          ))
        )"#,
    );
    assert_eq!(count, 1);
}

/// `CoreType::Module` body with a func type declaration and an import that
/// references it.  Exercises `enter_core_ty_scope` and
/// `resolve_maybe_from_subvec` for `ModuleTypeDeclaration`.
#[test]
fn test_scoped_resolution_core_module_type_body() {
    let count = run_paranoid(
        r#"(component
          (core type (module
            (type (func (param i32) (result i64)))
            (import "env" "compute" (func (type 0)))
            (export "result" (func (type 0)))
          ))
        )"#,
    );
    assert_eq!(count, 2);
}

/// Outer alias (depth > 0) inside an instance type body.
///
/// This is the exact pattern that triggered the `comp_at` subtraction
/// overflow before `type_scope_nesting` was introduced.  With one type-scope
/// level on the scope stack but zero on the component stack, resolving a
/// depth=1 ref into `component_stack[len - 1 - 1]` used to underflow.
#[test]
fn test_scoped_resolution_outer_alias_in_instance_type() {
    let count = run_paranoid(
        r#"(component
          (type $outer u32)
          (type (instance
            (alias outer 1 0 (type))
            (export "n" (type (eq 0)))
          ))
        )"#,
    );
    assert_eq!(count, 2);
}

/// Two consecutive instance type scopes.
///
/// Verifies that `type_scope_nesting` resets to zero after the first scope
/// exits, so the second scope's resolution is not affected by the first.
#[test]
fn test_scoped_resolution_nesting_resets_between_scopes() {
    assert_eq!(
        run_paranoid(
            r#"(component
          (type (instance
            (type $a u32)
            (export "a" (type (eq $a)))
          ))
          (type (instance
            (type $b string)
            (export "b" (type (eq $b)))
          ))
        )"#,
        ),
        2
    )
}

/// Instance type body that itself contains an inner instance type.
///
/// The outer export references the inner instance type, which is itself a
/// complete type body.  Verifies that nested scoped contexts are handled
/// correctly.
#[test]
fn test_scoped_resolution_nested_instance_types() {
    let count = run_paranoid(
        r#"(component
          (type (instance
            (type $inner (instance
              (type $t u8)
              (export "v" (type (eq $t)))
            ))
            (export "inner" (type (eq $inner)))
          ))
        )"#,
    );
    assert_eq!(count, 2);
}

// ============================================================
// Layer 2 — specific resolved-item variant assertions
// ============================================================

/// An export's type ref inside an instance type body (pointing at a sibling
/// type declaration in the same scope) must resolve to `ResolvedItem::CompType`.
#[test]
fn test_resolve_result_instance_type_export_ref_is_comp_type() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_inst_type_decl(
            &mut self,
            cx: &VisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &InstanceTypeDeclaration<'a>,
        ) {
            if !matches!(decl, InstanceTypeDeclaration::Export { .. }) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(..)),
                "export type ref in instance body should resolve to CompType",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component (type (instance (type $t u32) (export "n" (type (eq $t))))))"#,
        &mut v,
    );
    assert_eq!(
        v.checked, 1,
        "expected exactly 1 export type ref to be resolved"
    );
}

/// An export's type ref inside a `ComponentType::Component` body must also
/// resolve to `ResolvedItem::CompType`.
#[test]
fn test_resolve_result_component_type_export_ref_is_comp_type() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_comp_type_decl(
            &mut self,
            cx: &VisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &ComponentTypeDeclaration<'a>,
        ) {
            if !matches!(decl, ComponentTypeDeclaration::Export { .. }) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(..)),
                "export type ref in component body should resolve to CompType",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component (type (component (type $t u32) (export "value" (type (eq $t))))))"#,
        &mut v,
    );
    assert_eq!(
        v.checked, 1,
        "expected exactly 1 export type ref to be resolved"
    );
}

/// A func import's type ref inside a `CoreType::Module` body (pointing at a
/// sibling `Type` declaration in the same scope) must resolve to
/// `ResolvedItem::ModuleTyDecl`.
#[test]
fn test_resolve_result_core_module_import_ref_is_module_ty_decl() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_module_type_decl(
            &mut self,
            cx: &VisitCtx<'a>,
            _: usize,
            _: u32,
            _: &CoreType<'a>,
            decl: &ModuleTypeDeclaration<'a>,
        ) {
            if !matches!(decl, ModuleTypeDeclaration::Import(..)) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::ModuleTyDecl(..)),
                "import type ref in module type body should resolve to ModuleTyDecl",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component (core type (module
          (type (func (param i32) (result i64)))
          (import "env" "compute" (func (type 0)))
        )))"#,
        &mut v,
    );
    assert_eq!(
        v.checked, 1,
        "expected exactly 1 import type ref to be resolved"
    );
}

// ============================================================
// Layer 3 — exact resolved index assertions
// ============================================================

/// The only type in an instance type scope is at index 0; the export's ref
/// must resolve to `CompType(0, _)`.
#[test]
fn test_resolve_index_single_type_in_instance_scope() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_inst_type_decl(
            &mut self,
            cx: &VisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &InstanceTypeDeclaration<'a>,
        ) {
            if !matches!(decl, InstanceTypeDeclaration::Export { .. }) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(0, _)),
                "sole type in instance scope must resolve to index 0",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component (type (instance (type $t u32) (export "n" (type (eq $t))))))"#,
        &mut v,
    );
    assert_eq!(v.checked, 1);
}

/// When two types exist in an instance scope, an export referencing the
/// second must resolve to `CompType(1, _)`.
#[test]
fn test_resolve_index_second_type_in_instance_scope() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_inst_type_decl(
            &mut self,
            cx: &VisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &InstanceTypeDeclaration<'a>,
        ) {
            if !matches!(decl, InstanceTypeDeclaration::Export { .. }) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(1, _)),
                "export referencing second type must resolve to index 1",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component (type (instance
          (type $a u8)        (;; index 0 ;)
          (type $b string)    (;; index 1 ;)
          (export "x" (type (eq $b)))
        )))"#,
        &mut v,
    );
    assert_eq!(v.checked, 1);
}

/// An outer alias pointing at the *second* type in the enclosing component
/// scope must resolve to `CompType(1, _)` — verifying that `comp_at` walks
/// to the right outer scope AND picks the right index within it.
#[test]
fn test_resolve_index_outer_alias_to_second_outer_type() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_inst_type_decl(
            &mut self,
            cx: &VisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &InstanceTypeDeclaration<'a>,
        ) {
            if !matches!(decl, InstanceTypeDeclaration::Alias(..)) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(1, _)),
                "alias to second outer type must resolve to index 1",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component
          (type $first u8)   (;; outer index 0 ;)
          (type $second u32) (;; outer index 1 ;)
          (type (instance
            (alias outer 1 1 (type)) (;; depth=1, index=1 → $second ;)
            (export "n" (type (eq 0)))
          ))
        )"#,
        &mut v,
    );
    assert_eq!(v.checked, 1);
}

/// In a `CoreType::Module` body with two func types, an import referencing
/// the second func type must resolve to `ModuleTyDecl(1, _)`.
#[test]
fn test_resolve_index_module_type_import_refs_second_func_type() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_module_type_decl(
            &mut self,
            cx: &VisitCtx<'a>,
            _: usize,
            _: u32,
            _: &CoreType<'a>,
            decl: &ModuleTypeDeclaration<'a>,
        ) {
            if !matches!(decl, ModuleTypeDeclaration::Import(..)) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::ModuleTyDecl(1, _)),
                "import referencing second func type must resolve to index 1",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component (core type (module
          (type (func (param i32)))            (;; index 0 ;)
          (type (func (param i32) (result i64))) (;; index 1 ;)
          (import "env" "f" (func (type 1)))
        )))"#,
        &mut v,
    );
    assert_eq!(v.checked, 1);
}

/// An outer alias inside an instance type body resolves via the outer
/// component scope, not the type scope.  The resolved item must match the
/// type declared in the enclosing component.
#[test]
fn test_resolve_result_outer_alias_in_instance_type_resolves_to_comp_type() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_inst_type_decl(
            &mut self,
            cx: &VisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &InstanceTypeDeclaration<'a>,
        ) {
            if !matches!(decl, InstanceTypeDeclaration::Alias(..)) {
                return;
            }
            self.checked += check_refs(cx, decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(..)),
                "outer alias in instance type should resolve to CompType in the outer component scope");
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component
          (type $outer u32)
          (type (instance (alias outer 1 0 (type)) (export "n" (type (eq 0)))))
        )"#,
        &mut v,
    );
    assert_eq!(
        v.checked, 1,
        "expected exactly 1 outer alias ref to be resolved"
    );
}

// ============================================================
// Layer 1 — nested component-type / core-type body scenarios
// ============================================================

/// A `ComponentType::Component` body that contains a nested instance type
/// declaration with only local refs (no outer aliases).
///
/// Exercises `enter_comp_ty_scope` for both the outer component type and
/// the inner instance type.  The 2 refs are:
///   - instance export `(eq $item)` in the inner body
///   - comp-type export `(eq 1)` referencing the nested instance type
#[test]
fn test_scoped_resolution_comp_type_contains_nested_instance_type() {
    assert_eq!(
        run_paranoid(
            r#"(component
              (type (component
                (type $ct u32)
                (type (instance
                  (type $item u8)
                  (export "x" (type (eq $item)))
                ))
                (export "inner" (type (eq 1)))
              ))
            )"#,
        ),
        2,
    );
}

/// A `ComponentType::Component` body that contains a nested component type
/// declaration with its own export ref.
///
/// Two consecutive `ComponentTypeComponent` scope entries — verifies that
/// `type_scope_nesting` increments and resets correctly across both.
/// The 2 refs are:
///   - inner comp-type export `(eq $inner)`
///   - outer comp-type export `(eq 1)` referencing the nested comp type
#[test]
fn test_scoped_resolution_comp_type_contains_nested_comp_type() {
    assert_eq!(
        run_paranoid(
            r#"(component
              (type (component
                (type $ct u32)
                (type (component
                  (type $inner u32)
                  (export "n" (type (eq $inner)))
                ))
                (export "nested" (type (eq 1)))
              ))
            )"#,
        ),
        2,
    );
}

/// A `ComponentType::Component` body that contains a `CoreType::Module`
/// declaration whose import and export each ref a sibling func type.
///
/// Exercises the three-way nesting:
/// component → `ComponentTypeComponent` → `CoreTypeModule`.
/// The 2 refs are the two `(type 0)` uses inside the module type body.
#[test]
fn test_scoped_resolution_comp_type_contains_core_module_type() {
    assert_eq!(
        run_paranoid(
            r#"(component
              (type (component
                (core type (module
                  (type (func (param i32) (result i64)))
                  (import "env" "f" (func (type 0)))
                  (export "g" (func (type 0)))
                ))
              ))
            )"#,
        ),
        2,
    );
}

/// Outer alias (depth=1) declared directly inside a `ComponentType::Component`
/// body, pointing at a type in the enclosing component scope.
///
/// With `type_scope_nesting=1` at this point, depth=1 yields `comp_depth=0`,
/// resolving against the actual enclosing component.  2 refs: the alias target
/// and the export `(eq 0)` that consumes it.
#[test]
fn test_scoped_resolution_outer_alias_in_comp_type_body() {
    assert_eq!(
        run_paranoid(
            r#"(component
              (type $outer u32)
              (type (component
                (alias outer 1 0 (type))
                (export "n" (type (eq 0)))
              ))
            )"#,
        ),
        2,
    );
}

/// Outer alias at depth=1 from inside a nested instance type that is itself
/// inside a `ComponentType::Component` body.
///
/// `type_scope_nesting` reaches 2.  `saturating_sub(1, 2)` = 0 so
/// `comp_depth=0`, preventing the underflow the `type_scope_nesting`
/// mechanism was introduced to guard against.  2 refs: alias + export.
#[test]
fn test_scoped_resolution_outer_depth1_from_inst_inside_comp_type() {
    assert_eq!(
        run_paranoid(
            r#"(component
              (type (component
                (type $ct u32)
                (type (instance
                  (alias outer 1 0 (type))
                  (export "n" (type (eq 0)))
                ))
              ))
            )"#,
        ),
        2,
    );
}

/// Outer alias at depth=2 from inside a nested instance type that is itself
/// inside a `ComponentType::Component` body, targeting a type in the actual
/// enclosing component.
///
/// `type_scope_nesting=2`, depth=2 → `comp_depth=0` → actual component.
/// `scope_at_depth(2)` walks two levels up the scope_stack and lands on the
/// actual component scope, so both index lookup and item retrieval are correct.
/// 2 refs: alias + export.
#[test]
fn test_scoped_resolution_outer_depth2_from_inst_inside_comp_type() {
    assert_eq!(
        run_paranoid(
            r#"(component
              (type $outer u32)
              (type (component
                (type $ct u32)
                (type (instance
                  (alias outer 2 0 (type))
                  (export "n" (type (eq 0)))
                ))
              ))
            )"#,
        ),
        2,
    );
}

/// Verifies that `type_scope_nesting` resets correctly when a
/// `ComponentType::Component` scope is immediately followed by a
/// `ComponentType::Instance` scope at the same nesting level.
///
/// The second scope's resolution must not inherit residual nesting state
/// from the first.  2 refs total: one export ref in each type body.
#[test]
fn test_scoped_resolution_nesting_resets_comp_type_then_inst_type() {
    assert_eq!(
        run_paranoid(
            r#"(component
              (type (component
                (type $a u32)
                (export "a" (type (eq $a)))
              ))
              (type (instance
                (type $b string)
                (export "b" (type (eq $b)))
              ))
            )"#,
        ),
        2,
    );
}

// ============================================================
// Layer 2 — resolved-item variant assertions (nested types)
// ============================================================

/// An outer alias inside a `ComponentType::Component` body must resolve to
/// `ResolvedItem::CompType`, not `ResolvedItem::Import`.
///
/// Padding strategy: a type import occupies CompType index 0 in the enclosing
/// component scope.  If the resolution returns the wrong index (0), the
/// assertion catches it because `Import` ≠ `CompType`.  The alias correctly
/// targets index 1 (the defined type).
///
/// This is the component-type body analogue of
/// `test_resolve_result_outer_alias_in_instance_type_resolves_to_comp_type`.
#[test]
fn test_resolve_result_outer_alias_in_comp_type_body_is_comp_type() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_comp_type_decl(
            &mut self,
            cx: &VisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &ComponentTypeDeclaration<'a>,
        ) {
            if !matches!(decl, ComponentTypeDeclaration::Alias(..)) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(..)),
                "outer alias in comp-type body must resolve to CompType, not Import",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component
          (import "ty" (type (sub resource)))   (;; CompType index 0: Import — wrong index lands here ;)
          (type $target u32)         (;; CompType index 1: Main → correct target ;)
          (type (component
            (alias outer 1 1 (type)) (;; depth=1, index=1 → $target ;)
            (export "n" (type (eq 0)))
          ))
        )"#,
        &mut v,
    );
    assert_eq!(v.checked, 1, "expected exactly 1 outer alias ref resolved");
}

/// An outer alias at depth=2 from inside a doubly-nested type scope (instance
/// type inside component type) must resolve to `ResolvedItem::CompType`, not
/// `ResolvedItem::Import`.
///
/// Padding strategy: a type import at CompType index 0 in the enclosing
/// component scope catches a wrong-index error.  Additionally, a type import
/// at index 1 inside the intermediate component-type body catches a
/// wrong-depth error: if depth=1 is used instead of depth=2 the lookup lands
/// in the comp-type scope where index 1 is also an `Import`, returning
/// `ResolvedItem::Import` rather than `CompType`.
#[test]
fn test_resolve_result_outer_depth2_from_doubly_nested_is_comp_type() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_inst_type_decl(
            &mut self,
            cx: &VisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &InstanceTypeDeclaration<'a>,
        ) {
            if !matches!(decl, InstanceTypeDeclaration::Alias(..)) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(..)),
                "depth-2 outer alias must resolve to CompType, not Import",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component
          (import "outer-ty" (type (sub resource)))   (;; component CompType index 0: Import (wrong index) ;)
          (type $outer u32)              (;; component CompType index 1: CompType ← target ;)
          (type (component
            (import "inner-ty0" (type (sub resource))) (;; comp-type CompType index 0: Import (wrong index) ;)
            (import "inner-ty1" (type (sub resource))) (;; comp-type CompType index 1: Import (wrong depth) ;)
            (type $ct u32)
            (type (instance
              (alias outer 2 1 (type))   (;; depth=2, index=1 → $outer → CompType ;)
              (export "n" (type (eq 0)))
            ))
          ))
        )"#,
        &mut v,
    );
    assert_eq!(
        v.checked, 1,
        "expected exactly 1 depth-2 outer alias ref resolved"
    );
}

// ============================================================
// Layer 3 — exact resolved index (nested types)
// ============================================================

/// An outer alias in a `ComponentType::Component` body pointing at the
/// *second* type in the enclosing component scope must resolve to
/// `CompType(1, _)`.
///
/// This is the component-type body analogue of
/// `test_resolve_index_outer_alias_to_second_outer_type`.
#[test]
fn test_resolve_index_outer_in_comp_type_body_to_second_outer_type() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_comp_type_decl(
            &mut self,
            cx: &VisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &ComponentTypeDeclaration<'a>,
        ) {
            if !matches!(decl, ComponentTypeDeclaration::Alias(..)) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(1, _)),
                "outer alias to second outer type must resolve to index 1",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component
          (type $first u8)    (;; outer index 0 ;)
          (type $second u32)  (;; outer index 1 ;)
          (type (component
            (alias outer 1 1 (type)) (;; depth=1, index=1 → $second ;)
            (export "n" (type (eq 0)))
          ))
        )"#,
        &mut v,
    );
    assert_eq!(v.checked, 1);
}

/// An outer alias at depth=2 from inside a doubly-nested type scope pointing
/// at the *second* type in the enclosing component scope must resolve to
/// `CompType(1, _)`.
///
/// `type_scope_nesting=2` and depth=2 → `comp_depth=0`; `scope_at_depth(2)`
/// lands on the actual component scope where index 1 is `$second`.
#[test]
fn test_resolve_index_outer_depth2_from_doubly_nested_to_second_outer() {
    struct V {
        checked: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_inst_type_decl(
            &mut self,
            cx: &VisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &InstanceTypeDeclaration<'a>,
        ) {
            if !matches!(decl, InstanceTypeDeclaration::Alias(..)) {
                return;
            }
            self.checked += check_refs(
                cx,
                decl.referenced_indices(),
                |r| matches!(r, ResolvedItem::CompType(1, _)),
                "depth-2 outer alias to second outer type must resolve to index 1",
            );
        }
    }
    let mut v = V { checked: 0 };
    walk_wat(
        r#"(component
          (type $first u8)    (;; outer index 0 ;)
          (type $second u32)  (;; outer index 1 ;)
          (type (component
            (type $ct u32)
            (type (instance
              (alias outer 2 1 (type)) (;; depth=2, index=1 → $second ;)
              (export "n" (type (eq 0)))
            ))
          ))
        )"#,
        &mut v,
    );
    assert_eq!(v.checked, 1);
}

// ============================================================
// type_body_stack depth and pop-correctness tests
//
// These tests specifically target the scenario where multiple type bodies are
// pushed onto the stack (nested scopes), and verify that:
//
//   1. Resolution inside the innermost body uses the innermost frame.
//   2. After an inner scope exits, the enclosing scope's frame is active again.
//   3. Two sibling scopes (at the same level) each see only their own frame.
//   4. A triple-nested stack (inst in inst in outer comp-type) resolves at
//      the right depth throughout.
// ============================================================

/// Instance type nested directly inside another instance type — two levels of
/// type_body_stack.
///
/// The inner body has a single type at index 0.  An export referencing it must
/// resolve to `CompType(0, _)` while the inner scope is active.  The outer
/// body also has its own type at index 0; after the inner scope pops, the outer
/// scope's frame must be the active one again.
///
/// Ref count: inner export type ref (1) + outer export type ref (1) = 2.
#[test]
fn test_type_body_stack_inst_nested_in_inst_inner_resolves_independently() {
    assert_eq!(
        run_paranoid(
            r#"(component
              (type (instance
                (type $outer_t u32)        (;; outer body, index 0 ;)
                (type (instance
                  (type $inner_t u8)       (;; inner body, index 0 ;)
                  (export "iv" (type (eq $inner_t)))
                ))
                (export "ov" (type (eq $outer_t)))
              ))
            )"#,
        ),
        2,
    );
}

/// After a deep inner instance scope exits, a sibling export in the outer
/// instance body must still resolve correctly against the outer body's frame.
///
/// This is the "pop-correctness" guard: if `pop_type_body` is not called on
/// exit, or if it pops the wrong frame, the outer export ref panics or
/// resolves to the wrong item.
///
/// Ref count: inner export (1) + outer export (1) = 2.
#[test]
fn test_type_body_stack_outer_scope_resolves_after_inner_exits() {
    struct V {
        inst_export_count: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn visit_inst_type_decl(
            &mut self,
            cx: &VisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &InstanceTypeDeclaration<'a>,
        ) {
            if !matches!(decl, InstanceTypeDeclaration::Export { .. }) {
                return;
            }
            // Every export here must resolve — if the stack is wrong this panics.
            for r in decl.referenced_indices() {
                let resolved = cx.resolve(&r.ref_);
                assert!(
                    matches!(resolved, ResolvedItem::CompType(..)),
                    "export ref must resolve to CompType, got {resolved:?}"
                );
                self.inst_export_count += 1;
            }
        }
    }
    let mut v = V {
        inst_export_count: 0,
    };
    walk_wat(
        r#"(component
          (type (instance
            (type $a u32)          (;; outer body index 0 ;)
            (type (instance
              (type $b u8)         (;; inner body index 0 ;)
              (export "b" (type (eq $b)))
            ))
            (export "a" (type (eq $a)))  (;; must use outer frame, not inner ;)
          ))
        )"#,
        &mut v,
    );
    // Two exports: one in the inner body, one in the outer body.
    assert_eq!(v.inst_export_count, 2);
}

/// Two sibling instance types at the same nesting level — each resolved against
/// its own body frame, not the other's.
///
/// The paranoid walker already covers no-panic; this test adds a ref count
/// check confirming that both scopes fire callbacks independently.
///
/// Ref count: export in first body (1) + export in second body (1) = 2.
#[test]
fn test_type_body_stack_sibling_scopes_isolated() {
    assert_eq!(
        run_paranoid(
            r#"(component
              (type (instance
                (type $x u8)
                (export "x" (type (eq $x)))
              ))
              (type (instance
                (type $y string)
                (export "y" (type (eq $y)))
              ))
            )"#,
        ),
        2,
    );
}

/// Triple-nested stack: component type → instance type inside it →
/// another instance type inside that.
///
/// At peak nesting, type_body_stack has 3 frames.  Verifies that each
/// frame is pushed and popped correctly so refs at every level resolve to
/// the right item.
///
/// Ref count: innermost export (1) + middle export (1) = 2.
/// (The outer component-type has no export in this WAT.)
#[test]
fn test_type_body_stack_triple_depth() {
    assert_eq!(
        run_paranoid(
            r#"(component
              (type (component
                (type (instance
                  (type (instance
                    (type $leaf u8)
                    (export "leaf" (type (eq $leaf)))
                  ))
                  (type $mid u32)
                  (export "mid" (type (eq $mid)))
                ))
              ))
            )"#,
        ),
        2,
    );
}

/// Verifies that a core module type body nested inside a component type body
/// has its own isolated frame on type_body_stack.
///
/// The import inside the module type body references a func type declared in the
/// same module body (index 0).  If the outer comp-type frame were still active
/// instead of the module frame, the lookup would go to the wrong subvec.
///
/// Ref count: module import type ref (1) = 1.
#[test]
fn test_type_body_stack_module_type_inside_comp_type() {
    assert_eq!(
        run_paranoid(
            r#"(component
              (type (component
                (core type (module
                  (type (func (param i32)))
                  (import "m" "f" (func (type 0)))
                ))
              ))
            )"#,
        ),
        1,
    );
}

/// After exiting a core module type scope, the enclosing instance type scope's
/// frame must be reinstated on type_body_stack.
///
/// The instance body has a type at index 0 and exports it after the nested
/// module type.  If pop_type_body is missing for the module type, or if the
/// instance body's frame was never pushed, the export ref panics.
///
/// Ref count: module import ref (1) + instance export ref (1) = 2.
#[test]
fn test_type_body_stack_inst_scope_restored_after_module_type_exit() {
    assert_eq!(
        run_paranoid(
            r#"(component
              (type (instance
                (type $t u32)               (;; inst body index 0 ;)
                (core type (module
                  (type (func (param i32)))
                  (import "m" "f" (func (type 0)))
                ))
                (export "t" (type (eq $t))) (;; must resolve against inst frame ;)
              ))
            )"#,
        ),
        2,
    );
}

/// Exact-index check for the triple-depth case.
///
/// Layout:
///   - inner instance body: `$leaf` at index 0 → export refs index 0 → `CompType(0, _)`
///   - middle instance body: inner instance type at index 0, `$mid` at index 1
///     → export refs index 1 → `CompType(1, _)`
///
/// If the wrong frame were active for the middle export (e.g. the inner body's
/// frame lingering after pop), the lookup would find a different item and the
/// assertion would fire.
#[test]
fn test_type_body_stack_triple_depth_exact_index() {
    struct V {
        inner_checked: usize,
        middle_checked: usize,
        /// Tracks how deep the inst-type visitor is (shallow=middle, deep=inner).
        depth: usize,
    }
    impl<'a> ComponentVisitor<'a> for V {
        fn enter_component_type_inst(&mut self, _: &VisitCtx<'a>, _: u32, _: &ComponentType<'a>) {
            self.depth += 1;
        }
        fn exit_component_type_inst(&mut self, _: &VisitCtx<'a>, _: u32, _: &ComponentType<'a>) {
            self.depth -= 1;
        }
        fn visit_inst_type_decl(
            &mut self,
            cx: &VisitCtx<'a>,
            _: usize,
            _: u32,
            _: &ComponentType<'a>,
            decl: &InstanceTypeDeclaration<'a>,
        ) {
            if !matches!(decl, InstanceTypeDeclaration::Export { .. }) {
                return;
            }
            if self.depth >= 2 {
                // innermost body: $leaf is at index 0
                self.inner_checked += check_refs(
                    cx,
                    decl.referenced_indices(),
                    |r| matches!(r, ResolvedItem::CompType(0, _)),
                    "inner export ref must resolve to CompType(0, _)",
                );
            } else {
                // middle body: inner instance type is at index 0, $mid at index 1
                self.middle_checked += check_refs(
                    cx,
                    decl.referenced_indices(),
                    |r| matches!(r, ResolvedItem::CompType(1, _)),
                    "middle export ref must resolve to CompType(1, _)",
                );
            }
        }
    }
    let mut v = V {
        inner_checked: 0,
        middle_checked: 0,
        depth: 0,
    };
    walk_wat(
        r#"(component
          (type (component
            (type (instance
              (type (instance
                (type $leaf u8)
                (export "leaf" (type (eq $leaf)))
              ))
              (type $mid u32)
              (export "mid" (type (eq $mid)))
            ))
          ))
        )"#,
        &mut v,
    );
    assert_eq!(
        v.inner_checked, 1,
        "inner body export should have been checked once"
    );
    assert_eq!(
        v.middle_checked, 1,
        "middle body export should have been checked once"
    );
}
