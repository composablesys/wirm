//! Tests for `Component::resolve` and `Component::get_type_of_exported_lift_func`.

use crate::ir::component::concrete::ConcreteType;
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

// ============================================================
// concretize_import — instance type body resolution
// ============================================================

/// `concretize_import` must resolve type refs inside an instance-type body against
/// the body's own declaration namespace, not the component's main type namespace.
///
/// Regression test: before the fix, `VisitCtxInner::resolve()` fell through to the
/// component's main type index space for body-relative refs when called from
/// `concretize_instance_decls`, causing an out-of-bounds panic whenever the
/// body-local type index exceeded the component's main type count.
#[test]
fn test_concretize_import_resolves_body_types() {
    // The component has only 1 type in its main namespace (the instance type at index 0).
    // The instance body defines two types (body-index 0 and 1) and exports a function
    // whose type is body-index 1. Without the fix, resolving body-index 1 against the
    // main namespace (len=1) panics with "index out of bounds".
    let b = bytes(
        r#"(component
      (type (instance
        (type $elem u32)
        (type $fn-type (func (param "x" 0)))
        (export "my-func" (func (type 1)))
      ))
      (import "my-iface" (instance (type 0)))
    )"#,
    );
    let comp = parsed(&b);
    let result = comp.concretize_import("my-iface");
    assert!(
        matches!(result, Some(ConcreteType::Instance { .. })),
        "expected Some(Instance), got {result:?}"
    );
}

// ============================================================
// concretize_export — patterns
// ============================================================

fn check_concretize_export(wat: &str) {
    let b = bytes(wat);
    let comp = parsed(&b);
    let result = comp.concretize_export("iface");
    let Some(ConcreteType::Instance { funcs, .. }) = result else {
        panic!("expected Some(Instance), got {result:?}");
    };
    assert_eq!(funcs.len(), 1);
    assert_eq!(funcs[0].0, "f");
}

/// Export that resolves to a synthetic `FromExports` instance.
///
/// Pattern:
///   (alias export $imp "f" (func $f))
///   (instance $out (export "f" (func $f)))   ;; FromExports
///   (export "iface" (instance $out))
#[test]
fn concretize_export_from_exports_instance() {
    check_concretize_export(
        r#"(component
      (import "iface" (instance $imp
        (export "f" (func (param "x" u32) (result u8)))
      ))
      (alias export $imp "f" (func $fn))
      (instance $out (export "f" (func $fn)))
      (export "iface" (instance $out))
    )"#,
    )
}

/// Export that resolves to a `CompInst::Instantiate` (the wit-component shim pattern).
///
/// The shim component exports individual functions rather than a whole WIT
/// instance.  The outer component exports the shim instance under the interface
/// name.  `concretize_export` must collect the shim's function exports.
#[test]
fn concretize_export_instantiated_component() {
    check_concretize_export(
        r#"(component
      (import "iface" (instance $imp
        (export "f" (func (param "x" u32) (result u8)))
      ))
      (component $shim
        (type $sig (func (param "x" u32) (result u8)))
        (import "import-func-f" (func (type $sig)))
        (export "f" (func 0))
      )
      (alias export $imp "f" (func $fn))
      (instance $shim-inst (instantiate $shim
        (with "import-func-f" (func $fn))
      ))
      (export "iface" (instance $shim-inst))
    )"#,
    );
}

/// Export that directly re-exposes an imported instance (pass-through middleware).
///
/// Pattern:
///   (import "iface" (instance $imp ...))
///   (export "iface" (instance $imp))
#[test]
fn concretize_export_import_reexport() {
    check_concretize_export(
        r#"(component
      (import "iface" (instance $imp
        (export "f" (func (param "x" u32) (result u8)))
      ))
      (export "iface" (instance $imp))
    )"#,
    );
}

/// All three export patterns produce structurally identical `ConcreteType`s and
/// therefore equal fingerprints when compared via cviz.  This test verifies the
/// wirm side: that the function signature extracted from each pattern is identical.
#[test]
fn concretize_export_all_patterns_same_signature() {
    fn single_func_sig(wat: &str) -> crate::ir::component::concrete::ConcreteFuncType<'static> {
        // Round-trip through owned bytes to satisfy the `'static` bound.
        let bytes = wat::parse_str(wat).expect("WAT parse failed");
        let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        let comp = Box::leak(Box::new(Component::parse(bytes, false, false).unwrap()));
        let Some(ConcreteType::Instance { funcs: mut funcs, .. }) = comp.concretize_export("iface") else {
            panic!("expected Instance");
        };
        funcs.remove(0).1
    }

    let from_exports = single_func_sig(
        r#"(component
          (import "iface" (instance $imp (export "f" (func (param "x" u32)))))
          (alias export $imp "f" (func $fn))
          (instance $out (export "f" (func $fn)))
          (export "iface" (instance $out))
        )"#,
    );
    let import_reexport = single_func_sig(
        r#"(component
          (import "iface" (instance $imp (export "f" (func (param "x" u32)))))
          (export "iface" (instance $imp))
        )"#,
    );
    let shim = single_func_sig(
        r#"(component
          (import "iface" (instance $imp (export "f" (func (param "x" u32)))))
          (component $shim
            (type $sig (func (param "x" u32)))
            (import "import-func-f" (func (type $sig)))
            (export "f" (func 0))
          )
          (alias export $imp "f" (func $fn))
          (instance $shim-inst (instantiate $shim (with "import-func-f" (func $fn))))
          (export "iface" (instance $shim-inst))
        )"#,
    );

    // All three should carry a single u32 param and no result.
    for (label, sig) in [
        ("from-exports", &from_exports),
        ("import-reexport", &import_reexport),
        ("shim", &shim),
    ] {
        assert_eq!(
            sig.params.len(),
            1,
            "{label}: expected 1 param, got {}",
            sig.params.len()
        );
        assert!(
            matches!(
                sig.params[0],
                (
                    "x",
                    crate::ir::component::concrete::ConcreteValType::Primitive(
                        wasmparser::PrimitiveValType::U32
                    )
                )
            ),
            "{label}: expected (\"x\", Primitive(U32)), got {:?}",
            sig.params[0]
        );
        assert!(sig.result.is_none(), "{label}: expected no result");
    }
}

// ============================================================
// concretize_import — val-type coverage
// ============================================================

use crate::ir::component::concrete::ConcreteValType;

fn first_param_type(wat: &str) -> ConcreteValType<'_> {
    // We need the bytes to live long enough; use a local helper that returns owned.
    // Instead, build the bytes externally and let the caller deal with lifetimes.
    // This is a helper for tests that need to inspect a concrete val type.
    let bytes = bytes(wat);
    // Safety: we box-leak to get 'static for simplicity in tests.
    let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    let comp = Box::leak(Box::new(Component::parse(bytes, false, false).unwrap()));
    let Some(ConcreteType::Instance { funcs, .. }) = comp.concretize_import("iface") else {
        panic!("expected Instance");
    };
    funcs
        .into_iter()
        .next()
        .unwrap()
        .1
        .params
        .into_iter()
        .next()
        .unwrap()
        .1
}

#[test]
fn concretize_import_record_param() {
    let ty = first_param_type(
        r#"(component
      (type (instance
        (type $rec (record (field "a" u32) (field "b" string)))
        (type $fn (func (param "r" 0)))
        (export "f" (func (type 1)))
      ))
      (import "iface" (instance (type 0)))
    )"#,
    );
    assert!(
        matches!(ty, ConcreteValType::Record(_)),
        "expected Record, got {ty:?}"
    );
    let ConcreteValType::Record(fields) = ty else {
        unreachable!()
    };
    assert_eq!(fields.len(), 2);
    assert!(
        matches!(
            *fields[0].1,
            ConcreteValType::Primitive(wasmparser::PrimitiveValType::U32)
        ),
        "field 'a'"
    );
    assert!(
        matches!(
            *fields[1].1,
            ConcreteValType::Primitive(wasmparser::PrimitiveValType::String)
        ),
        "field 'b'"
    );
}

#[test]
fn concretize_import_variant_param() {
    let ty = first_param_type(
        r#"(component
      (type (instance
        (type $var (variant (case "a" u32) (case "b")))
        (type $fn (func (param "v" 0)))
        (export "f" (func (type 1)))
      ))
      (import "iface" (instance (type 0)))
    )"#,
    );
    assert!(
        matches!(ty, ConcreteValType::Variant(_)),
        "expected Variant, got {ty:?}"
    );
    let ConcreteValType::Variant(cases) = ty else {
        unreachable!()
    };
    assert_eq!(cases.len(), 2);
    assert!(matches!(cases[0], ("a", Some(_))));
    assert!(matches!(cases[1], ("b", None)));
}

#[test]
fn concretize_import_list_param() {
    let ty = first_param_type(
        r#"(component
      (type (instance
        (type $lst (list u8))
        (type $fn (func (param "l" 0)))
        (export "f" (func (type 1)))
      ))
      (import "iface" (instance (type 0)))
    )"#,
    );
    assert!(
        matches!(ty, ConcreteValType::List(_)),
        "expected List, got {ty:?}"
    );
}

#[test]
fn concretize_import_tuple_param() {
    let ty = first_param_type(
        r#"(component
      (type (instance
        (type $tup (tuple u32 string))
        (type $fn (func (param "t" 0)))
        (export "f" (func (type 1)))
      ))
      (import "iface" (instance (type 0)))
    )"#,
    );
    assert!(
        matches!(ty, ConcreteValType::Tuple(_)),
        "expected Tuple, got {ty:?}"
    );
    let ConcreteValType::Tuple(elems) = ty else {
        unreachable!()
    };
    assert_eq!(elems.len(), 2);
}

#[test]
fn concretize_import_option_param() {
    let ty = first_param_type(
        r#"(component
      (type (instance
        (type $opt (option string))
        (type $fn (func (param "o" 0)))
        (export "f" (func (type 1)))
      ))
      (import "iface" (instance (type 0)))
    )"#,
    );
    assert!(
        matches!(ty, ConcreteValType::Option(_)),
        "expected Option, got {ty:?}"
    );
}

#[test]
fn concretize_import_result_param() {
    let ty = first_param_type(
        r#"(component
      (type (instance
        (type $res (result u32 (error string)))
        (type $fn (func (param "r" 0)))
        (export "f" (func (type 1)))
      ))
      (import "iface" (instance (type 0)))
    )"#,
    );
    assert!(
        matches!(ty, ConcreteValType::Result { .. }),
        "expected Result, got {ty:?}"
    );
    let ConcreteValType::Result { ok, err } = ty else {
        unreachable!()
    };
    assert!(ok.is_some(), "expected ok type");
    assert!(err.is_some(), "expected err type");
}

#[test]
fn concretize_import_flags_param() {
    let ty = first_param_type(
        r#"(component
      (type (instance
        (type $flg (flags "read" "write" "exec"))
        (type $fn (func (param "f" 0)))
        (export "f" (func (type 1)))
      ))
      (import "iface" (instance (type 0)))
    )"#,
    );
    assert!(
        matches!(ty, ConcreteValType::Flags(_)),
        "expected Flags, got {ty:?}"
    );
    let ConcreteValType::Flags(names) = ty else {
        unreachable!()
    };
    assert_eq!(names, vec!["read", "write", "exec"]);
}

#[test]
fn concretize_import_enum_param() {
    let ty = first_param_type(
        r#"(component
      (type (instance
        (type $enm (enum "low" "medium" "high"))
        (type $fn (func (param "e" 0)))
        (export "f" (func (type 1)))
      ))
      (import "iface" (instance (type 0)))
    )"#,
    );
    assert!(
        matches!(ty, ConcreteValType::Enum(_)),
        "expected Enum, got {ty:?}"
    );
    let ConcreteValType::Enum(variants) = ty else {
        unreachable!()
    };
    assert_eq!(variants, vec!["low", "medium", "high"]);
}

// ============================================================
// resolve_type_from_import_instance — InstanceExport to imported instance
// ============================================================

/// A server-style component (exports interface via shim, no matching import)
/// and a middleware-style component (imports the same interface and re-exports it)
/// must produce the same `ConcreteFuncType` signature for the shared function.
///
/// This is the regression test for the two-fingerprint bug: previously the
/// import-declaration path collapsed variant types to `Resource` while the
/// shim path resolved them correctly, causing a fingerprint mismatch.
#[test]
fn server_and_middleware_concretize_to_same_func_type() {
    // Server pattern: exports "my:iface@1.0" via an instantiated shim component.
    // The shim takes the function as an import and re-exports it.  No import
    // of "my:iface@1.0" exists in the outer component, so concretize_export
    // falls back to concretize_comp_func_exports(shim).
    //
    // Inline WAT requires a separate (type ...) declaration for the func
    // signature when the result type is not a primitive.
    let server_b = bytes(
        r#"(component
          (component $shim
            (type $ev (variant (case "a") (case "b" u32)))
            (type $fn-type (func (param "x" u32) (result 0)))
            (import "handle" (func $h (type $fn-type)))
            (export "handle" (func $h))
          )
          (import "handle" (func $h
            (param "x" u32) (result (variant (case "a") (case "b" u32)))))
          (instance $si (instantiate $shim (with "handle" (func $h))))
          (export "my:iface@1.0" (instance $si))
        )"#,
    );
    // Middleware / import-reexport pattern: imports the interface and re-exports it.
    // concretize_export calls concretize_import, which enters the instance type body.
    // The function's result type (bare index 0) is the inline variant at type[0].
    //
    // Note: the `(export "ev" (type (eq 0)))` type-export alias occupies type[1] in the
    // binary, so the func type ($fn) ends up at type[2].  `(func (type $fn))` avoids the
    // ambiguity — the wat crate resolves the name to the correct binary index.
    let middleware_a = bytes(
        r#"(component
          (import "my:iface@1.0" (instance $iface
            (type (variant (case "a") (case "b" u32)))
            (export "ev" (type (eq 0)))
            (type $fn (func (param "x" u32) (result 0)))
            (export "handle" (func (type $fn)))
          ))
          (export "my:iface@1.0" (instance $iface))
        )"#,
    );

    let server_b_s: &'static [u8] = Box::leak(server_b.into_boxed_slice());
    let middleware_a_s: &'static [u8] = Box::leak(middleware_a.into_boxed_slice());
    let sb = Box::leak(Box::new(
        Component::parse(server_b_s, false, false).unwrap(),
    ));
    let ma = Box::leak(Box::new(
        Component::parse(middleware_a_s, false, false).unwrap(),
    ));

    let Some(ConcreteType::Instance { funcs: sb_funcs, .. }) = sb.concretize_export("my:iface@1.0") else {
        panic!("server_b: expected Some(Instance)");
    };
    let Some(ConcreteType::Instance { funcs: ma_funcs, .. }) = ma.concretize_export("my:iface@1.0") else {
        panic!("middleware_a: expected Some(Instance)");
    };

    assert_eq!(sb_funcs.len(), 1, "server_b should export 1 function");
    assert_eq!(ma_funcs.len(), 1, "middleware_a should export 1 function");
    assert_eq!(sb_funcs[0].0, "handle");
    assert_eq!(ma_funcs[0].0, "handle");

    // The result types must be structurally equal — before the fix, one path
    // would return Resource while the other returned Variant.
    let sb_result = sb_funcs[0].1.result.as_ref();
    let ma_result = ma_funcs[0].1.result.as_ref();
    assert!(
        matches!(sb_result, Some(ConcreteValType::Variant(_))),
        "server_b result should be Variant, got {sb_result:?}"
    );
    assert!(
        matches!(ma_result, Some(ConcreteValType::Variant(_))),
        "middleware_a result should be Variant, got {ma_result:?}"
    );
}

/// Same regression test as `server_and_middleware_concretize_to_same_func_type` but
/// `middleware_a` uses an **explicit** top-level type declaration (instead of an inline
/// type body inside the import) to rule out any scope-registration bug specific to
/// inline instance types in imports.
///
/// If this test passes while `server_and_middleware_concretize_to_same_func_type` fails,
/// the root cause is that inline instance types inside import declarations are not
/// getting their type-body scope registered during the structural traversal.
#[test]
fn server_and_middleware_same_func_type_explicit_type_decl() {
    // Server: exports "my:iface@1.0" via an instantiated shim — same WAT as the original.
    let server_b = bytes(
        r#"(component
          (component $shim
            (type $ev (variant (case "a") (case "b" u32)))
            (type $fn-type (func (param "x" u32) (result 0)))
            (import "handle" (func $h (type $fn-type)))
            (export "handle" (func $h))
          )
          (import "handle" (func $h
            (param "x" u32) (result (variant (case "a") (case "b" u32)))))
          (instance $si (instantiate $shim (with "handle" (func $h))))
          (export "my:iface@1.0" (instance $si))
        )"#,
    );
    // Middleware: imports and re-exports "my:iface@1.0" using an EXPLICIT type declaration.
    // The instance type is declared as a top-level `(type ...)` and then referenced from
    // the import via `(instance (type 0))`.  This guarantees the type-body scope is
    // registered during the structural traversal.
    let middleware_a = bytes(
        r#"(component
          (type $iface-type (instance
            (type $var (variant (case "a") (case "b" u32)))
            (type $fn (func (param "x" u32) (result 0)))
            (export "handle" (func (type 1)))
          ))
          (import "my:iface@1.0" (instance $iface (type $iface-type)))
          (export "my:iface@1.0" (instance $iface))
        )"#,
    );

    let server_b_s: &'static [u8] = Box::leak(server_b.into_boxed_slice());
    let middleware_a_s: &'static [u8] = Box::leak(middleware_a.into_boxed_slice());
    let sb = Box::leak(Box::new(
        Component::parse(server_b_s, false, false).unwrap(),
    ));
    let ma = Box::leak(Box::new(
        Component::parse(middleware_a_s, false, false).unwrap(),
    ));

    let Some(ConcreteType::Instance { funcs: sb_funcs, .. }) = sb.concretize_export("my:iface@1.0") else {
        panic!("server_b: expected Some(Instance)");
    };
    let Some(ConcreteType::Instance { funcs: ma_funcs, .. }) = ma.concretize_export("my:iface@1.0") else {
        panic!("middleware_a (explicit type decl): expected Some(Instance)");
    };

    assert_eq!(sb_funcs.len(), 1, "server_b should export 1 function");
    assert_eq!(
        ma_funcs.len(),
        1,
        "middleware_a (explicit type decl) should export 1 function"
    );
    assert_eq!(sb_funcs[0].0, "handle");
    assert_eq!(ma_funcs[0].0, "handle");

    let sb_result = sb_funcs[0].1.result.as_ref();
    let ma_result = ma_funcs[0].1.result.as_ref();
    assert!(
        matches!(sb_result, Some(ConcreteValType::Variant(_))),
        "server_b result should be Variant, got {sb_result:?}"
    );
    assert!(
        matches!(ma_result, Some(ConcreteValType::Variant(_))),
        "middleware_a (explicit type decl) result should be Variant, got {ma_result:?}"
    );
}

/// Tests `resolve_type_from_import_instance` via a **direct function import** rather than
/// an instance type body.  This avoids the `alias outer` WAT syntax limitations while still
/// exercising the same `InstanceExport → import instance` resolution path.
///
/// Without the fix, `concretize_from_resolved` returns `ConcreteValType::Resource` when it
/// encounters `ComponentAlias::InstanceExport` pointing to an imported (not locally-
/// instantiated) instance.  With the fix it delegates to `resolve_type_from_import_instance`.
#[test]
fn concretize_func_param_via_alias_to_imported_instance_type_direct() {
    // A component that:
    //   1. Imports an instance "$types" containing a variant type "my-variant"
    //   2. Aliases "my-variant" to $mv at the outer component level
    //   3. Imports a function whose param type IS $mv (an InstanceExport alias)
    //
    // concretize_import("handle") must return Variant, not Resource.
    // Note: inline type definitions are not valid inside instance-type export declarations
    // in WAT.  The type must be declared first and then re-exported via (type (eq N)).
    let b = bytes(
        r#"(component
          (import "types" (instance $types
            (type (variant (case "a") (case "b" u32)))
            (export "my-variant" (type (eq 0)))
          ))
          (alias export $types "my-variant" (type $mv))
          (type $fn-type (func (param "x" $mv)))
          (import "handle" (func (type $fn-type)))
        )"#,
    );
    let b_s: &'static [u8] = Box::leak(b.into_boxed_slice());
    let comp = Box::leak(Box::new(Component::parse(b_s, false, false).unwrap()));

    let Some(ConcreteType::Func(ft)) = comp.concretize_import("handle") else {
        panic!("expected ConcreteType::Func for 'handle' import");
    };
    assert_eq!(ft.params.len(), 1, "expected 1 param");
    assert!(
        matches!(ft.params[0].1, ConcreteValType::Variant(_)),
        "param type should be Variant (resolve_type_from_import_instance), got {:?}",
        ft.params[0].1
    );
    let ConcreteValType::Variant(cases) = &ft.params[0].1 else {
        unreachable!()
    };
    assert_eq!(cases.len(), 2);
    assert_eq!(cases[0].0, "a");
    assert!(cases[0].1.is_none());
    assert_eq!(cases[1].0, "b");
    assert!(matches!(
        cases[1].1.as_deref(),
        Some(ConcreteValType::Primitive(
            wasmparser::PrimitiveValType::U32
        ))
    ));
}
