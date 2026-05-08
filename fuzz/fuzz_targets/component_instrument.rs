//! wasm-smith Component → wirm parse → graft a fresh-named arg onto an
//! existing core/component `Instantiate` pointing at a freshly-added IR
//! item → encode → validate.
//!
//! Stresses wirm's topological encode logic for component-level forward
//! references. Find the first core `Instance::Instantiate` and the first
//! component `ComponentInstance::Instantiate` in the IR, pick a recipe
//! per side via the per-input recipe bytes, build a fresh dep chain, and
//! append a new instantiation arg pointing at the freshly-added item.
//! The dep is added *after* the consumer in IR insertion order, so wirm
//! must reorder. Existing roundtrip targets don't trigger this because
//! wasm-smith never emits a binary where a consumer textually precedes
//! its dep.
//!
//! Either side independently degenerates to a plain encode-roundtrip
//! when the smith input has no `Instantiate` of that flavor (no input
//! is wasted). Per-op `nop` instrumentation lives in `module_instrument`
//! and isn't duplicated here — keeping this target focused on the
//! reorder bug class.
//!
//! Correctness of the encoder's topological reordering is checked
//! transitively: if wirm misorders the new dep relative to its consumer,
//! the re-encoded binary will reference an undefined index and the
//! post-encode `wasmparser::Validator` call panics. We don't structurally
//! diff against an expected ordering — see fuzz/DECISIONS.md.
//!
//! Design per fuzz/DECISIONS.md — parse / pre-validation failures silent,
//! encode / post-encode validation errors are bugs.

#![no_main]

use libfuzzer_sys::fuzz_target;
use wasm_smith::Component as SmithComponent;
use wasmparser::{
    ComponentDefinedType, ComponentExternalKind, ComponentFuncType, ComponentImportName,
    ComponentInstance, ComponentInstantiationArg, ComponentType, CoreType, Export, ExternalKind,
    Instance, InstantiationArg, InstantiationArgKind, PrimitiveValType,
};
use wirm::ir::id::{
    ComponentFunctionId, ComponentId, ComponentInstanceId, ComponentTypeId, CoreInstanceId,
    ModuleID,
};
use wirm::{Component, Module};

/// Forward-reference dep-chain depth used by every recipe. Distinct from
/// wasm-smith's own size/depth knob — that one bounds the generated
/// component, this one bounds the deps the fuzzer grafts on top.
///
/// Read once per process from `WIRM_FUZZ_INJECTION_DEPTH` (default 3),
/// so the weekly cron can crank it up via the workflow `env:` block
/// without touching the code, and local runs can keep the default.
fn injection_depth() -> usize {
    static CELL: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *CELL.get_or_init(|| {
        std::env::var("WIRM_FUZZ_INJECTION_DEPTH")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3)
    })
}

const NUM_MODULE_RECIPES: u8 = 3;
const NUM_COMPONENT_RECIPES: u8 = 6;

fuzz_target!(|input: (SmithComponent, u8, u8)| {
    let (smith, mod_byte, comp_byte) = input;
    let bytes = smith.to_bytes();

    if wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&bytes)
        .is_err()
    {
        return;
    }

    let mut comp = match Component::parse(&bytes, false, false) {
        Ok(c) => c,
        Err(_) => return,
    };

    let core_inst_pos = comp
        .instances
        .iter()
        .position(|i| matches!(i, Instance::Instantiate { .. }));
    let comp_inst_pos = comp
        .component_instance
        .iter()
        .position(|i| matches!(i, ComponentInstance::Instantiate { .. }));

    // Skip when neither side has an Instantiate to graft onto — the
    // unmodified parse → encode → validate path is already covered by
    // `component_roundtrip`, so re-running it here just smears coverage
    // across two targets and slows libfuzzer's convergence on
    // Instantiate-rich shapes.
    if core_inst_pos.is_none() && comp_inst_pos.is_none() {
        return;
    }

    if let Some(pos) = core_inst_pos {
        inject_core_arg(&mut comp, pos, mod_byte % NUM_MODULE_RECIPES);
    }
    if let Some(pos) = comp_inst_pos {
        inject_component_arg(&mut comp, pos, comp_byte % NUM_COMPONENT_RECIPES);
    }

    let encoded = comp
        .encode()
        .expect("instrumented component failed to re-encode");

    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&encoded)
        .expect("instrumented component failed wasmparser validation");
});

// ── module-side ─────────────────────────────────────────────────────

/// Build a fresh core-instance via the chosen recipe, then append a new
/// `InstantiationArg` to the target core `Instance::Instantiate` pointing
/// at it. Core-instantiation arg kinds are always `Instance` per
/// wasmparser, so only the recipe varies, not the kind.
fn inject_core_arg<'a>(comp: &mut Component<'a>, target_idx: usize, recipe: u8) {
    let new_inst = match recipe {
        0 => recipe_core_empty(comp),
        1 => recipe_core_alias_export(comp).unwrap_or_else(|| recipe_core_empty(comp)),
        2 => recipe_core_chain(comp, injection_depth()),
        _ => unreachable!(),
    };
    let new_arg = InstantiationArg {
        name: "wirm_inj",
        kind: InstantiationArgKind::Instance,
        index: *new_inst,
    };
    if let Instance::Instantiate { args, .. } = &mut comp.instances[target_idx] {
        let mut next = args.to_vec();
        next.push(new_arg);
        *args = next.into_boxed_slice();
    }
}

/// Recipe 0: empty `FromExports` core instance. Single forward-ref hop;
/// `WIRM_FUZZ_INJECTION_DEPTH` doesn't apply.
fn recipe_core_empty<'a>(comp: &mut Component<'a>) -> CoreInstanceId {
    comp.add_core_instance(Instance::FromExports(Vec::new().into_boxed_slice()))
}

/// Recipe 1: `FromExports` carrying one alias of an existing core
/// instance's export. Falls back to the empty recipe when no eligible
/// source exists (smith may not produce any `FromExports` core
/// instance). Single hop; could be extended by aliasing more exports
/// across multiple existing instances.
fn recipe_core_alias_export<'a>(comp: &mut Component<'a>) -> Option<CoreInstanceId> {
    // The aliased name must match an actual export of the source instance,
    // so we lift it directly from a `FromExports` we already see.
    let (src_inst_idx, kind, name) = comp.instances.iter().enumerate().find_map(|(idx, inst)| {
        let Instance::FromExports(exports) = inst else {
            return None;
        };
        exports.first().map(|e| (idx, e.kind, e.name))
    })?;

    let alias_id = comp.add_alias_core_instance_export(kind, src_inst_idx as u32, name);
    let raw_idx = match kind {
        ExternalKind::Func | ExternalKind::FuncExact => *alias_id.unwrap_core_func(),
        ExternalKind::Table => *alias_id.unwrap_core_table(),
        ExternalKind::Memory => *alias_id.unwrap_core_memory(),
        ExternalKind::Global => *alias_id.unwrap_core_global(),
        ExternalKind::Tag => *alias_id.unwrap_core_tag(),
    };
    let exports = vec![Export {
        name: "wirm_e",
        kind,
        index: raw_idx,
    }];
    Some(comp.add_core_instance(Instance::FromExports(exports.into_boxed_slice())))
}

/// Recipe 2: add a fresh empty module, then a core instance instantiating
/// it with no args (module has no imports). Two hops at depth 1. Deeper
/// chains require type-import plumbing on each module so the next
/// instance can satisfy its imports — TODO.
fn recipe_core_chain<'a>(comp: &mut Component<'a>, _depth: usize) -> CoreInstanceId {
    let new_module = comp.add_module(Module::default());
    comp.add_core_instance(Instance::Instantiate {
        module_index: *new_module,
        args: Vec::new().into_boxed_slice(),
    })
}

// ── component-side ──────────────────────────────────────────────────

/// One recipe per `ComponentExternalKind` variant; pick by the
/// component-recipe byte. Each builds a fresh IR item of the right kind,
/// then appends a `ComponentInstantiationArg` of that kind onto the
/// target component instantiation.
fn inject_component_arg<'a>(comp: &mut Component<'a>, target_idx: usize, recipe: u8) {
    let (kind, index) = match recipe {
        0 => (ComponentExternalKind::Module, *recipe_comp_module(comp)),
        1 => (
            ComponentExternalKind::Component,
            *recipe_comp_component(comp, injection_depth()),
        ),
        2 => (ComponentExternalKind::Func, *recipe_comp_func(comp)),
        3 => (ComponentExternalKind::Type, *recipe_comp_type(comp)),
        // TODO Kind 4 (Value): no clean fresh-value source today — a value
        // can only enter the index space via import / export / alias /
        // start-result. Fuzzing this kind likely means importing a
        // value, which is a single-hop forward ref. Skipped for now;
        // wire up when we want that coverage.
        4 => return,
        5 => (
            ComponentExternalKind::Instance,
            *recipe_comp_instance(comp, injection_depth()),
        ),
        _ => unreachable!(),
    };
    let arg = ComponentInstantiationArg {
        name: "wirm_inj",
        kind,
        index,
    };
    if let ComponentInstance::Instantiate { args, .. } =
        &mut comp.component_instance[target_idx]
    {
        let mut next = args.to_vec();
        next.push(arg);
        *args = next.into_boxed_slice();
    }
}

/// Kind 0 (Module): empty `CoreType::Module` then an `import` of that
/// type. Two-hop dep chain; the import is what the new arg points at.
/// Deeper chains would require outer-alias plumbing — TODO.
fn recipe_comp_module<'a>(comp: &mut Component<'a>) -> ModuleID {
    let core_type = comp.add_core_type(CoreType::Module(Vec::new().into_boxed_slice()));
    comp.add_import_core_module(ComponentImportName("wirm_m"), *core_type)
}

/// Kind 1 (Component): nest empty sub-components to `depth`. At depth 1
/// just an empty sub-component; deeper levels add an inner primitive
/// type so the outer component genuinely depends on it.
fn recipe_comp_component<'a>(comp: &mut Component<'a>, depth: usize) -> ComponentId {
    fn build(c: &mut Component<'_>, levels_remaining: usize) -> ComponentId {
        if levels_remaining <= 1 {
            return c.add_component(|inner| {
                inner.add_component_type(ComponentType::Defined(
                    ComponentDefinedType::Primitive(PrimitiveValType::U32),
                ));
            });
        }
        c.add_component(|inner| {
            build(inner, levels_remaining - 1);
        })
    }
    build(comp, depth.max(1))
}

/// Kind 2 (Func): empty `ComponentType::Func` then an import of that
/// type. Deeper chains need component-type-decl plumbing — TODO.
fn recipe_comp_func<'a>(comp: &mut Component<'a>) -> ComponentFunctionId {
    let func_ty = comp.add_component_type(ComponentType::Func(ComponentFuncType {
        async_: false,
        params: Vec::new().into_boxed_slice(),
        result: None,
    }));
    comp.add_import_component_func(ComponentImportName("wirm_f"), *func_ty)
}

/// Kind 3 (Type): a fresh primitive defined type. Single hop; doesn't
/// chain.
fn recipe_comp_type<'a>(comp: &mut Component<'a>) -> ComponentTypeId {
    comp.add_component_type(ComponentType::Defined(ComponentDefinedType::Primitive(
        PrimitiveValType::U32,
    )))
}

/// Kind 5 (Instance): at depth 1 an empty `FromExports`. At depth ≥ 2
/// build a sub-component to depth-1 first and `Instantiate` it, so the
/// instance pulls in the whole sub-component chain.
fn recipe_comp_instance<'a>(comp: &mut Component<'a>, depth: usize) -> ComponentInstanceId {
    if depth <= 1 {
        return comp.add_component_instance(ComponentInstance::FromExports(
            Vec::new().into_boxed_slice(),
        ));
    }
    let inner = recipe_comp_component(comp, depth - 1);
    comp.add_component_instance(ComponentInstance::Instantiate {
        component_index: *inner,
        args: Vec::new().into_boxed_slice(),
    })
}
