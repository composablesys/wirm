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
        let Some(ConcreteType::Instance { mut funcs, .. }) = comp.concretize_export("iface") else {
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

    let Some(ConcreteType::Instance {
        funcs: sb_funcs, ..
    }) = sb.concretize_export("my:iface@1.0")
    else {
        panic!("server_b: expected Some(Instance)");
    };
    let Some(ConcreteType::Instance {
        funcs: ma_funcs, ..
    }) = ma.concretize_export("my:iface@1.0")
    else {
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

    let Some(ConcreteType::Instance {
        funcs: sb_funcs, ..
    }) = sb.concretize_export("my:iface@1.0")
    else {
        panic!("server_b: expected Some(Instance)");
    };
    let Some(ConcreteType::Instance {
        funcs: ma_funcs, ..
    }) = ma.concretize_export("my:iface@1.0")
    else {
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

// Section count invariant: each binary section payload must map to exactly
// one `comp.sections` entry, so `cx.curr_section_idx()` stays aligned with
// wasmparser-based consumers. Uses `wasm_encoder` directly because
// `wat::parse_str` merges consecutive same-kind sections.

/// Build a component binary with `n` separate single-item alias sections,
/// mirroring the layout wac-compose produces for split components.
fn build_binary_with_separate_alias_sections(n: usize) -> Vec<u8> {
    use wasm_encoder::{
        Alias, Component, ComponentAliasSection, ComponentExportKind, ComponentImportSection,
        ComponentTypeRef, ComponentTypeSection, InstanceType, TypeBounds,
    };

    // Resource names need to outlive the alias entries we hand to
    // wasm_encoder, so build them up front.
    let names: Vec<String> = (0..n).map(|i| format!("r{i}")).collect();

    let mut comp = Component::new();

    // Type section: a types-instance type that exports `n` resources.
    {
        let mut types = ComponentTypeSection::new();
        let mut inst = InstanceType::new();
        for name in &names {
            inst.export(name, ComponentTypeRef::Type(TypeBounds::SubResource));
        }
        types.instance(&inst);
        comp.section(&types);
    }

    // Import section: import the types instance.
    {
        let mut imports = ComponentImportSection::new();
        imports.import("wasi:http/types", ComponentTypeRef::Instance(0));
        comp.section(&imports);
    }

    // `n` separate alias sections, each with a single
    // instance-export-of-type alias. This is the key shape: each
    // alias is its own binary section.
    for name in &names {
        let mut aliases = ComponentAliasSection::new();
        aliases.alias(Alias::InstanceExport {
            instance: 0,
            kind: ComponentExportKind::Type,
            name,
        });
        comp.section(&aliases);
    }

    // Trailing type section: a final empty instance type (the
    // "handler" placeholder) so the test can verify the section
    // count after the alias run.
    {
        let mut types = ComponentTypeSection::new();
        types.instance(&InstanceType::new());
        comp.section(&types);
    }

    // Trailing import section: a handler-style import that points at
    // the final instance type. Type 0 is the types-instance type,
    // types 1..=n are the alias-produced resource types, and type
    // n+1 is the trailing instance type defined just above.
    {
        let mut imports = ComponentImportSection::new();
        imports.import(
            "wasi:http/handler",
            ComponentTypeRef::Instance((n + 1) as u32),
        );
        comp.section(&imports);
    }

    comp.finish()
}

/// Count component-section payloads of each kind by walking the
/// binary independently with wasmparser.
fn count_binary_sections_by_kind(bytes: &[u8]) -> std::collections::HashMap<&'static str, usize> {
    let mut counts: std::collections::HashMap<&'static str, usize> = Default::default();
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        let key = match payload.expect("parse") {
            wasmparser::Payload::ComponentTypeSection(_) => "type",
            wasmparser::Payload::ComponentImportSection(_) => "import",
            wasmparser::Payload::ComponentAliasSection(_) => "alias",
            wasmparser::Payload::ComponentInstanceSection(_) => "comp_instance",
            wasmparser::Payload::ComponentExportSection(_) => "comp_export",
            wasmparser::Payload::ComponentCanonicalSection(_) => "canon",
            wasmparser::Payload::CoreTypeSection(_) => "core_type",
            wasmparser::Payload::InstanceSection(_) => "core_instance",
            wasmparser::Payload::ModuleSection { .. } => "module",
            wasmparser::Payload::ComponentSection { .. } => "subcomp",
            wasmparser::Payload::ComponentStartSection { .. } => "start",
            wasmparser::Payload::CustomSection(_) => "custom",
            _ => continue,
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

/// Count `comp.sections` entries by kind in a parsed wirm component.
fn count_wirm_sections_by_kind(
    comp: &Component<'_>,
) -> std::collections::HashMap<&'static str, usize> {
    use crate::ir::component::section::ComponentSection;
    let mut counts: std::collections::HashMap<&'static str, usize> = Default::default();
    for (_, section) in comp.sections.iter() {
        let key = match section {
            ComponentSection::ComponentType => "type",
            ComponentSection::ComponentImport => "import",
            ComponentSection::Alias => "alias",
            ComponentSection::ComponentInstance => "comp_instance",
            ComponentSection::ComponentExport => "comp_export",
            ComponentSection::Canon => "canon",
            ComponentSection::CoreType => "core_type",
            ComponentSection::CoreInstance => "core_instance",
            ComponentSection::Module => "module",
            ComponentSection::Component => "subcomp",
            ComponentSection::ComponentStartSection => "start",
            ComponentSection::CustomSection => "custom",
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

/// Parse `bytes` with wirm and assert that for every section kind the
/// number of `comp.sections` entries equals the number of binary
/// payloads of that kind. Returns the parsed component for any
/// follow-up assertions the caller wants to make.
fn assert_section_count_invariant<'a>(bytes: &'a [u8]) -> Component<'a> {
    let comp = Component::parse(bytes, false, false).expect("wirm parse");
    let binary_counts = count_binary_sections_by_kind(bytes);
    let wirm_counts = count_wirm_sections_by_kind(&comp);
    assert_eq!(
        binary_counts, wirm_counts,
        "wirm's comp.sections counts must match the binary's payload counts \
         exactly — each binary section payload should produce one wirm entry"
    );
    comp
}

/// Each binary section payload must produce exactly one entry in
/// `comp.sections`. The cross-call fold bug folded N consecutive
/// same-kind binary sections into a single wirm entry, which made
/// any wasmparser-based consumer (e.g. splicer's section reencoder)
/// disagree with wirm about section ordinals.
///
/// `wasm_encoder` is used directly here because `wat::parse_str`
/// always merges consecutive same-kind decls into one binary section
/// — only direct binary construction can produce the "N separate
/// single-item alias sections" layout that triggered the bug.
#[test]
fn section_count_invariant_separate_alias_sections() {
    for n in [1usize, 3, 7] {
        let bytes = build_binary_with_separate_alias_sections(n);
        assert_section_count_invariant(&bytes);
        // Spot-check: each of the n alias decls is its own binary
        // section, so the binary has n alias payloads — and so does
        // wirm. The bug folded all of them into one entry.
        assert_eq!(
            count_binary_sections_by_kind(&bytes).get("alias"),
            Some(&n),
            "expected {n} separate alias section payloads"
        );
    }
}

/// A component type section that mixes leaf types and
/// instance/component types must still produce exactly one wirm
/// entry. Earlier versions of `add_to_sections` spread the items
/// one-per-entry whenever the section contained any
/// subscope-bearing type (Instance/Component), which broke the
/// "one binary payload = one wirm entry" invariant for the very
/// common case of "two records and an instance type".
///
/// The visitor's scope accounting is per-item (via
/// `maybe_enter_scope` / `maybe_exit_scope` in the driver) and
/// doesn't depend on section grouping, so the spread was
/// unnecessary.
#[test]
fn section_count_invariant_mixed_leaf_and_instance_types() {
    // Three top-level types in one binary section: two leaf
    // records and one instance type. wat::parse_str merges them
    // into a single ComponentTypeSection payload with count=3.
    let b = bytes(
        r#"(component
            (type (record (field "x" string)))
            (type (record (field "y" 0)))
            (type (instance
                (alias outer 1 1 (type))
                (export "thing" (type (eq 0))))))"#,
    );

    let comp = assert_section_count_invariant(&b);

    // Single binary type section, single wirm entry — even though
    // type 2 is an Instance type with a nested scope.
    assert_eq!(count_binary_sections_by_kind(&b).get("type"), Some(&1));

    // The merged entry should have count=3 (all three types).
    use crate::ir::component::section::ComponentSection;
    let type_entry = comp
        .sections
        .iter()
        .find(|(_, s)| matches!(s, ComponentSection::ComponentType))
        .expect("type entry");
    assert_eq!(
        type_entry.0, 3,
        "merged type section should have count=3 even when one item has a subscope"
    );
}

/// Dual of the above: `wat::parse_str` merges consecutive same-kind
/// decls into one binary section, so 3 alias decls produce ONE alias
/// section with 3 items. wirm should still report ONE entry — but
/// with `count == 3` (the in-payload grouping logic).
#[test]
fn section_count_invariant_merged_alias_section_from_wat() {
    let b = bytes(
        r#"(component
            (type (instance
                (export "r0" (type (sub resource)))
                (export "r1" (type (sub resource)))
                (export "r2" (type (sub resource)))))
            (import "wasi:http/types" (instance (type 0)))
            (alias export 0 "r0" (type))
            (alias export 0 "r1" (type))
            (alias export 0 "r2" (type)))"#,
    );

    let comp = assert_section_count_invariant(&b);

    // Single alias section in the binary, single entry in wirm.
    assert_eq!(count_binary_sections_by_kind(&b).get("alias"), Some(&1));

    // And the merged entry's count should be 3 (in-payload grouping).
    use crate::ir::component::section::ComponentSection;
    let alias_entry = comp
        .sections
        .iter()
        .find(|(_, s)| matches!(s, ComponentSection::Alias))
        .expect("alias entry");
    assert_eq!(alias_entry.0, 3, "merged alias section should have count=3");
}

/// An empty section (e.g. a `ComponentImportSection` with count=0)
/// must still produce one `comp.sections` entry — otherwise
/// `curr_section_idx()` silently skips the binary section and any
/// wasmparser-based consumer sees off-by-one mismatches. wasm-smith
/// can generate empty sections; `wat::parse_str` will not, so this is
/// constructed directly with `wasm_encoder`.
#[test]
fn section_count_invariant_empty_import_section() {
    use wasm_encoder::{
        Component, ComponentImportSection, ComponentTypeRef, ComponentTypeSection, TypeBounds,
    };

    let mut comp = Component::new();

    // 1: empty import section. ComponentImportSection::new() with no
    // imports added encodes as a payload with count=0.
    comp.section(&ComponentImportSection::new());

    // 2: a non-empty component type section so we can verify the
    // following entry's section_idx is 1.
    let mut types = ComponentTypeSection::new();
    types
        .defined_type()
        .primitive(wasm_encoder::PrimitiveValType::U32);
    comp.section(&types);

    // 3: an import referencing type 0, so the binary ends up validating.
    let mut imports = ComponentImportSection::new();
    imports.import("t", ComponentTypeRef::Type(TypeBounds::Eq(0)));
    comp.section(&imports);

    let bytes = comp.finish();
    let comp = assert_section_count_invariant(&bytes);

    use crate::ir::component::section::ComponentSection;
    assert_eq!(comp.sections.len(), 3, "empty + type + import = 3 entries");
    assert_eq!(
        comp.sections[0],
        (0, ComponentSection::ComponentImport),
        "empty import section must still produce a (0, ComponentImport) entry"
    );
    assert!(matches!(
        comp.sections[1].1,
        ComponentSection::ComponentType
    ));
    assert!(matches!(
        comp.sections[2].1,
        ComponentSection::ComponentImport
    ));
}
