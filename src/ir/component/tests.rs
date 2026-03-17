//! Tests for `Component::resolve` and `Component::get_type_of_exported_lift_func`.

use crate::ir::component::refs::GetItemRef;
use crate::ir::component::visitor::ResolvedItem;
use crate::Component;

// ============================================================
// Helpers
// ============================================================

fn bytes(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("WAT parse failed")
}

fn parsed(b: &[u8]) -> Component<'_> {
    Component::parse(b, false, false).unwrap()
}

/// Resolve the ref carried by `comp.exports[export_idx]` against `comp`'s own index space.
fn resolve_export<'a>(comp: &'a Component<'a>, export_idx: usize) -> ResolvedItem<'a, 'a> {
    let ref_ = comp.exports[export_idx].get_item_ref();
    comp.resolve(&ref_.ref_)
}

// ============================================================
// Component::resolve — basic cases
// ============================================================

/// A type declared directly in a component resolves to `CompType` at the correct index.
#[test]
fn test_resolve_type_ref_from_export() {
    let b = bytes(
        r#"(component
      (type $a u32)    (;; index 0 ;)
      (type $b u8)     (;; index 1 ;)
      (export "a" (type $a))
      (export "b" (type $b))
    )"#,
    );
    let comp = parsed(&b);

    assert!(matches!(
        resolve_export(&comp, 0),
        ResolvedItem::CompType(0, _)
    ));
    assert!(matches!(
        resolve_export(&comp, 1),
        ResolvedItem::CompType(1, _)
    ));
}

/// A type that enters the index space via an import resolves to `Import`.
#[test]
fn test_resolve_imported_type_ref() {
    // The import occupies type index 0; re-exporting it makes a ref we can resolve.
    let b = bytes(
        r#"(component
      (import "t" (type (sub resource)))
      (export "t-out" (type 0))
    )"#,
    );
    let comp = parsed(&b);

    assert!(matches!(
        resolve_export(&comp, 0),
        ResolvedItem::Import(0, _)
    ));
}

/// A type that enters the index space via an outer alias resolves to `Alias`.
#[test]
fn test_resolve_alias_ref() {
    // The inner component aliases type 0 from the outer component and re-exports it.
    let b = bytes(
        r#"(component
      (type $outer u32)
      (component $inner
        (alias outer 1 0 (type))  (;; aliases outer type 0 → inner type 0 ;)
        (export "t" (type 0))
      )
    )"#,
    );
    let outer = parsed(&b);
    let inner = &outer.components[0];

    assert!(matches!(
        resolve_export(inner, 0),
        ResolvedItem::Alias(0, _)
    ));
}

// ============================================================
// Component::resolve — cross-scope (the key new capability)
// ============================================================

/// Resolving refs on a *nested* component uses that component's own index space,
/// not the outer component's space.  This is the cross-scope resolution case that
/// was previously impossible without a walk.
#[test]
fn test_resolve_on_inner_component() {
    let b = bytes(
        r#"(component
      (component $inner
        (type $a u32)   (;; inner type 0 ;)
        (type $b u8)    (;; inner type 1 ;)
        (export "a" (type $a))
        (export "b" (type $b))
      )
    )"#,
    );
    let outer = parsed(&b);
    let inner = &outer.components[0];

    assert!(matches!(
        resolve_export(inner, 0),
        ResolvedItem::CompType(0, _)
    ));
    assert!(matches!(
        resolve_export(inner, 1),
        ResolvedItem::CompType(1, _)
    ));
}

/// Two nested components each have their own independent type index spaces.
/// Resolving on either should only consult that component's own space.
#[test]
fn test_resolve_on_two_independent_inner_components() {
    let b = bytes(
        r#"(component
      (component $first
        (type $x u32)   (;; first's type 0 ;)
        (export "x" (type $x))
      )
      (component $second
        (type $p u8)    (;; second's type 0 ;)
        (type $q u16)   (;; second's type 1 ;)
        (export "p" (type $p))
        (export "q" (type $q))
      )
    )"#,
    );
    let outer = parsed(&b);
    let first = &outer.components[0];
    let second = &outer.components[1];

    assert!(matches!(
        resolve_export(first, 0),
        ResolvedItem::CompType(0, _)
    ));
    assert!(matches!(
        resolve_export(second, 0),
        ResolvedItem::CompType(0, _)
    ));
    assert!(matches!(
        resolve_export(second, 1),
        ResolvedItem::CompType(1, _)
    ));
}

// ============================================================
// get_type_of_exported_lift_func
// ============================================================

/// `get_type_of_exported_lift_func` returns the correct `ComponentType::Func`
/// for a lifted canonical function.
#[test]
fn test_get_type_of_exported_lift_func() {
    use crate::ir::id::ComponentExportId;

    let b = bytes(
        r#"(component
      (core module $m
        (func (export "add") (param i32 i32) (result i32)
          local.get 0
          local.get 1
          i32.add
        )
      )
      (core instance $mi (instantiate $m))
      (type $add-t (func (param "a" u32) (param "b" u32) (result u32)))
      (func $add (type $add-t) (canon lift (core func $mi "add")))
      (export "add" (func $add))
    )"#,
    );
    let comp = parsed(&b);

    let ty = comp.get_type_of_exported_lift_func(ComponentExportId(0));
    assert!(
        ty.is_some(),
        "should find the type of the exported lift func"
    );
    assert!(
        matches!(ty.unwrap(), wasmparser::ComponentType::Func(_)),
        "resolved type should be ComponentType::Func"
    );
}
