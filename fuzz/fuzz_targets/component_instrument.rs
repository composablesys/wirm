//! wasm-smith Component → wirm parse → graft a fresh-named arg onto an
//! existing core/component `Instantiate` pointing at a freshly-added IR
//! item built via one of many recipes → encode → validate.
//!
//! Stresses two axes simultaneously:
//!
//!   * **Topological reorder** — every kind's recipes append new items
//!     *after* the consumer in IR insertion order, so wirm must reorder
//!     them ahead during encode. Several recipes also build genuine
//!     multi-hop dep chains (typed-list nesting, FromExports + alias
//!     ladders, type→import pairs) so reorder must traverse more than
//!     one edge.
//!
//!   * **API coverage** — each kind has multiple sub-recipes that
//!     exercise different `Component` mutation methods (per-kind
//!     `add_import_*`, `add_alias_instance_export`, `add_alias_outer`,
//!     extra `ComponentType` / `ComponentDefinedType` variants, sub-
//!     components). The fuzzer picks a sub-recipe per side via the
//!     per-input sub-bytes. A recipe that needs an existing source
//!     (e.g. `alias_instance_export` of a Func) returns `None` when
//!     no source exists; the injection is skipped for that side.
//!
//! After injection both sides unconditionally run two orthogonal
//! phases: export the freshly-added item via the matching
//! `add_export_*` helper, and append a `add_custom_section`. Per-op
//! `nop` instrumentation lives in `module_instrument` and isn't
//! duplicated here.
//!
//! Either side independently degenerates to a plain encode-roundtrip
//! when the smith input has no `Instantiate` of that flavor.
//!
//! Correctness of the encoder's topological reordering is checked
//! transitively: if wirm misorders any new dep relative to its
//! consumer, the re-encoded binary will reference an undefined index
//! and the post-encode `wasmparser::Validator` call panics. We don't
//! structurally diff against an expected ordering — see
//! fuzz/DECISIONS.md.
//!
//! Design per fuzz/DECISIONS.md — parse / pre-validation failures
//! silent, encode / post-encode validation errors are bugs.

#![no_main]

use libfuzzer_sys::fuzz_target;
use wasm_smith::Component as SmithComponent;
use wasmparser::{
    CanonicalFunction, ComponentDefinedType, ComponentExportName, ComponentExternalKind,
    ComponentFuncType, ComponentImportName, ComponentInstance, ComponentInstantiationArg,
    ComponentOuterAliasKind, ComponentType, ComponentValType, CoreType, Export, ExternalKind,
    Import, Instance, InstantiationArg, InstantiationArgKind, MemoryType, ModuleTypeDeclaration,
    PrimitiveValType, TypeBounds, TypeRef, ValType, VariantCase,
};
use wirm::ir::id::{
    ComponentFunctionId, ComponentId, ComponentInstanceId, ComponentTypeId, CoreInstanceId,
    ModuleID, ValueID,
};
use wirm::ir::types::CustomSection;
use wirm::Component;

/// Forward-reference dep-chain depth used by every chained recipe.
/// Distinct from wasm-smith's own size/depth knob — that one bounds
/// the generated component, this one bounds the deps the fuzzer
/// grafts on top.
///
/// Read once per process from `WIRM_FUZZ_INJECTION_DEPTH` (default
/// 3), so the weekly cron can crank it up via the workflow `env:`
/// block without touching the code, and local runs can keep the
/// default.
fn injection_depth() -> usize {
    static CELL: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *CELL.get_or_init(|| {
        std::env::var("WIRM_FUZZ_INJECTION_DEPTH")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3)
    })
}

const NUM_CORE_RECIPES: u8 = 4;
const NUM_COMPONENT_KINDS: u8 = 6;

fuzz_target!(|input: (SmithComponent, u8, u8, u8, u8, u8, u8, u8, u8)| {
    let (
        smith,
        mod_main_a,
        mod_sub_a,
        comp_main_a,
        comp_sub_a,
        mod_main_b,
        mod_sub_b,
        comp_main_b,
        comp_sub_b,
    ) = input;
    let bytes = smith.to_bytes();

    if wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&bytes)
        .is_err()
    {
        return;
    }

    // Per-iteration arena of aux arg names for branching recipes,
    // one set per recipe round. Two rounds (A and B) so each
    // consumer gets two unrelated chains. Names are round-prefixed
    // so the two rounds can't collide within a single args list.
    let depth = injection_depth();
    let n_aux = depth.saturating_sub(1).max(1);
    let aux_names_owned_a: Vec<String> = (0..n_aux).map(|i| format!("wirm_a_aux_{i}")).collect();
    let aux_names_owned_b: Vec<String> = (0..n_aux).map(|i| format!("wirm_b_aux_{i}")).collect();
    let aux_names_a: Vec<&str> = aux_names_owned_a.iter().map(|s| s.as_str()).collect();
    let aux_names_b: Vec<&str> = aux_names_owned_b.iter().map(|s| s.as_str()).collect();

    let mut comp = match Component::parse(&bytes, false, false) {
        Ok(c) => c,
        Err(_) => return,
    };

    // Recurse the whole component tree and inject at every level.
    // Two recipe rounds (A and B) per consumer — each consumer
    // gets two unrelated chains feeding it. Returns the total
    // count of (core + component) consumers found, so we can
    // short-circuit when the input has no Instantiate anywhere.
    let consumers_found = inject_recursively(
        &mut comp,
        mod_main_a,
        mod_sub_a,
        comp_main_a,
        comp_sub_a,
        &aux_names_a,
        "wirm_inj_a",
        "a",
    );
    let _ = inject_recursively(
        &mut comp,
        mod_main_b,
        mod_sub_b,
        comp_main_b,
        comp_sub_b,
        &aux_names_b,
        "wirm_inj_b",
        "b",
    );

    if consumers_found == 0 {
        return;
    }

    // Always exercise add_start_section — orthogonal to the
    // injection logic. Build a producer (() -> u32) and a consumer
    // ((u32) -> ()), with the producer's ValueID flowing as the
    // consumer's arg. Tests the start → ValueID → start path the
    // encoder uses to resolve `Space::CompVal` refs through start
    // results, which an empty `() -> ()` start wouldn't exercise.
    // Values are linear; producing without consuming would fail
    // validation, hence the pair.
    let start_ty_producer = comp.add_component_type(ComponentType::Func(ComponentFuncType {
        async_: false,
        params: Vec::new().into_boxed_slice(),
        result: Some(ComponentValType::Primitive(PrimitiveValType::U32)),
    }));
    let start_func_producer = comp.add_import_component_func(
        ComponentImportName("wirm_start_p"),
        *start_ty_producer,
    );
    let produced = comp.add_start_section(start_func_producer, Vec::new(), 1);
    if let Some(&vid) = produced.first() {
        let start_ty_consumer = comp.add_component_type(ComponentType::Func(ComponentFuncType {
            async_: false,
            params: vec![("v", ComponentValType::Primitive(PrimitiveValType::U32))]
                .into_boxed_slice(),
            result: None,
        }));
        let start_func_consumer = comp.add_import_component_func(
            ComponentImportName("wirm_start_c"),
            *start_ty_consumer,
        );
        let _ = comp.add_start_section(start_func_consumer, vec![vid], 0);
    }

    // In-place mutation of existing `FromExports` instances: at
    // every scope, append one extra export to each existing
    // `FromExports`. The new export duplicates the first
    // entry's target under name `wirm_g_dup`, so it doesn't
    // change validity (consumers care about specific named
    // exports; an extra unused name is harmless), but the
    // encoder's export-list walk sees an extended list.
    mutate_from_exports_per_scope(&mut comp);

    // Duplicate-and-redirect mutation: for each existing
    // `FromExports` instance in the tree (both core and
    // component-side), build an alias-wrapped duplicate and
    // redirect any consumer args from the original to the
    // duplicate. The wrapper is validation-equivalent because each
    // export aliases the original under the same name + kind, so
    // consumers see the same shape. Smith-generated input shapes
    // drive most of the variety here.
    duplicate_and_redirect_per_scope(&mut comp);

    // Always exercise add_custom_section at every scope (top-level
    // and all nested sub-components, including those freshly added
    // by recipes). Exercises the per-component custom-section
    // emission path in the encoder rather than just the top-level
    // one.
    add_custom_section_per_scope(&mut comp);

    let encoded = comp
        .encode()
        .expect("instrumented component failed to re-encode");

    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&encoded)
        .expect("instrumented component failed wasmparser validation");
});

/// Walk the component tree depth-first, injecting at every level.
/// `primary_name` is the arg name for this round's injection (e.g.
/// "wirm_inj_a") and `round_tag` is a short label embedded in
/// kind-specific export names so two rounds against the same
/// consumer can't collide on either arg names or export names.
fn inject_recursively<'a>(
    comp: &mut Component<'a>,
    mod_main: u8,
    mod_sub: u8,
    comp_main: u8,
    comp_sub: u8,
    aux_names: &[&'a str],
    primary_name: &'a str,
    round_tag: &'a str,
) -> usize {
    let preexisting_subcomponents = comp.components.len();
    let core_inst_positions: Vec<usize> = comp
        .instances
        .iter()
        .enumerate()
        .filter_map(|(idx, i)| matches!(i, Instance::Instantiate { .. }).then_some(idx))
        .collect();
    let comp_inst_positions: Vec<usize> = comp
        .component_instance
        .iter()
        .enumerate()
        .filter_map(|(idx, i)| matches!(i, ComponentInstance::Instantiate { .. }).then_some(idx))
        .collect();
    let mut count = core_inst_positions.len() + comp_inst_positions.len();

    for pos in core_inst_positions {
        try_inject_core_arg(comp, pos, mod_main, mod_sub, primary_name);
    }
    for pos in comp_inst_positions {
        try_inject_component_arg(
            comp,
            pos,
            comp_main,
            comp_sub,
            aux_names,
            primary_name,
            round_tag,
        );
    }

    for i in 0..preexisting_subcomponents {
        let sub = &mut comp.components[i];
        count += inject_recursively(
            sub,
            mod_main,
            mod_sub,
            comp_main,
            comp_sub,
            aux_names,
            primary_name,
            round_tag,
        );
    }
    count
}

/// Walk the tree and, at every scope, for each existing
/// `FromExports` (core or component), append one extra export
/// entry under a fresh name (`wirm_g_dup`) that duplicates the
/// first existing entry's `(kind, index)`. Tests the encoder's
/// in-place export-list mutation path — the existing instance
/// stays at its original index but its body is mutated.
fn mutate_from_exports_per_scope<'a>(comp: &mut Component<'a>) {
    let n = comp.instances.len();
    for i in 0..n {
        if let Instance::FromExports(exports) = &mut comp.instances[i] {
            if let Some(first) = exports.first() {
                let extra = Export {
                    name: "wirm_g_dup",
                    kind: first.kind,
                    index: first.index,
                };
                let mut next = exports.to_vec();
                next.push(extra);
                *exports = next.into_boxed_slice();
            }
        }
    }
    let m = comp.component_instance.len();
    for i in 0..m {
        if let ComponentInstance::FromExports(exports) = &mut comp.component_instance[i] {
            if let Some(first) = exports.first() {
                let extra = wasmparser::ComponentExport {
                    name: ComponentExportName("wirm_g_dup"),
                    kind: first.kind,
                    index: first.index,
                    ty: None,
                };
                let mut next = exports.to_vec();
                next.push(extra);
                *exports = next.into_boxed_slice();
            }
        }
    }
    let n_sub = comp.components.len();
    for i in 0..n_sub {
        mutate_from_exports_per_scope(&mut comp.components[i]);
    }
}

/// Walk the component tree and, at every scope, for each existing
/// `FromExports` instance (core and component-side both), build an
/// alias-wrapped duplicate and redirect any consumer's args
/// referring to the original toward the duplicate. The wrapper
/// preserves validity because each new export aliases the original
/// under the same name + kind. Acts on smith-generated `FromExports`
/// shapes, so the mutations vary with the input rather than being
/// driven by a static recipe.
fn duplicate_and_redirect_per_scope<'a>(comp: &mut Component<'a>) {
    duplicate_and_redirect_core(comp);
    duplicate_and_redirect_component(comp);
    let n = comp.components.len();
    for i in 0..n {
        duplicate_and_redirect_per_scope(&mut comp.components[i]);
    }
}

fn duplicate_and_redirect_core<'a>(comp: &mut Component<'a>) {
    let original_count = comp.instances.len();
    let depth = injection_depth();
    let mut redirects: Vec<(u32, u32)> = Vec::new();
    for src_idx in 0..original_count {
        let exports = match &comp.instances[src_idx] {
            Instance::FromExports(es) if !es.is_empty() => es.to_vec(),
            _ => continue,
        };
        // Build a depth-N chain of FromExports duplicates, each
        // aliasing the previous level (chain[0] aliases the
        // original). The encoder must walk every level via the
        // new aliases — turning smith's single-level FromExports
        // into a multi-hop chain.
        let mut current_idx = src_idx as u32;
        for _ in 0..depth.max(1) {
            let mut new_exports = Vec::with_capacity(exports.len());
            for e in &exports {
                let alias =
                    comp.add_alias_core_instance_export(e.kind, current_idx, e.name);
                let raw_idx = match e.kind {
                    ExternalKind::Func | ExternalKind::FuncExact => *alias.unwrap_core_func(),
                    ExternalKind::Table => *alias.unwrap_core_table(),
                    ExternalKind::Memory => *alias.unwrap_core_memory(),
                    ExternalKind::Global => *alias.unwrap_core_global(),
                    ExternalKind::Tag => *alias.unwrap_core_tag(),
                };
                new_exports.push(Export {
                    name: e.name,
                    kind: e.kind,
                    index: raw_idx,
                });
            }
            let dup = comp
                .add_core_instance(Instance::FromExports(new_exports.into_boxed_slice()));
            current_idx = *dup;
        }
        redirects.push((src_idx as u32, current_idx));
    }
    for c in 0..comp.instances.len() {
        if let Instance::Instantiate { args, .. } = &mut comp.instances[c] {
            let mut next = args.to_vec();
            for arg in next.iter_mut() {
                if let Some(&(_, dup)) =
                    redirects.iter().find(|(orig, _)| *orig == arg.index)
                {
                    arg.index = dup;
                }
            }
            *args = next.into_boxed_slice();
        }
    }
}

fn duplicate_and_redirect_component<'a>(comp: &mut Component<'a>) {
    let original_count = comp.component_instance.len();
    let depth = injection_depth();
    let mut redirects: Vec<(u32, u32)> = Vec::new();
    for src_idx in 0..original_count {
        let exports = match &comp.component_instance[src_idx] {
            ComponentInstance::FromExports(es) if !es.is_empty() => es.to_vec(),
            _ => continue,
        };
        let mut current_idx = src_idx as u32;
        for _ in 0..depth.max(1) {
            let mut new_exports = Vec::with_capacity(exports.len());
            for e in &exports {
                let alias = comp.add_alias_instance_export(e.kind, current_idx, e.name.0);
                let raw_idx = match e.kind {
                    ComponentExternalKind::Module => *alias.unwrap_core_module(),
                    ComponentExternalKind::Component => *alias.unwrap_component(),
                    ComponentExternalKind::Func => *alias.unwrap_component_func(),
                    ComponentExternalKind::Type => *alias.unwrap_component_type(),
                    ComponentExternalKind::Value => *alias.unwrap_component_value(),
                    ComponentExternalKind::Instance => *alias.unwrap_component_instance(),
                };
                new_exports.push(wasmparser::ComponentExport {
                    name: e.name,
                    kind: e.kind,
                    index: raw_idx,
                    ty: None,
                });
            }
            let dup = comp.add_component_instance(ComponentInstance::FromExports(
                new_exports.into_boxed_slice(),
            ));
            current_idx = *dup;
        }
        redirects.push((src_idx as u32, current_idx));
    }
    for c in 0..comp.component_instance.len() {
        if let ComponentInstance::Instantiate { args, .. } = &mut comp.component_instance[c]
        {
            let mut next = args.to_vec();
            for arg in next.iter_mut() {
                if arg.kind == ComponentExternalKind::Instance {
                    if let Some(&(_, dup)) =
                        redirects.iter().find(|(orig, _)| *orig == arg.index)
                    {
                        arg.index = dup;
                    }
                }
            }
            *args = next.into_boxed_slice();
        }
    }
}

/// Add a custom section to this component and every nested sub-
/// component, including any sub-components freshly added by the
/// injection phase. The encoder emits custom sections per-component,
/// so seeding one at every scope exercises the per-component
/// emission path rather than only the top-level one.
fn add_custom_section_per_scope<'a>(comp: &mut Component<'a>) {
    comp.add_custom_section(CustomSection::new("wirm_inj_cs", b"wirm".to_vec()));
    let n = comp.components.len();
    for i in 0..n {
        add_custom_section_per_scope(&mut comp.components[i]);
    }
}

// ── module-side ─────────────────────────────────────────────────────

/// Build a fresh core-instance via the chosen recipe, then append a
/// new `InstantiationArg` to the target core `Instance::Instantiate`
/// pointing at it. Core-instantiation arg kinds are always `Instance`
/// per wasmparser, so only the recipe varies, not the kind.
fn try_inject_core_arg<'a>(
    comp: &mut Component<'a>,
    target_idx: usize,
    recipe: u8,
    _sub: u8,
    primary_name: &'a str,
) {
    let new_inst = match recipe % NUM_CORE_RECIPES {
        0 => recipe_core_empty(comp),
        1 => recipe_core_alias_export(comp),
        2 => recipe_core_instantiate_chain(comp, injection_depth()),
        3 => recipe_core_alias_chain(comp, injection_depth()),
        _ => unreachable!(),
    };
    let new_arg = InstantiationArg {
        name: primary_name,
        kind: InstantiationArgKind::Instance,
        index: *new_inst,
    };
    if let Instance::Instantiate { args, .. } = &mut comp.instances[target_idx] {
        let mut next = args.to_vec();
        next.push(new_arg);
        *args = next.into_boxed_slice();
    }
}

/// Default `MemoryType` used by the synthesized source and chain
/// modules — 32-bit, non-shared, initial 0 pages, no maximum.
const SYNTH_MEMORY_TYPE: MemoryType = MemoryType {
    memory64: false,
    shared: false,
    initial: 0,
    maximum: None,
    page_size_log2: None,
};

/// Find or synthesize a `(core_instance_index, export_kind, export_name)`
/// triple that points at an existing aliasable export. Used by the
/// alias-based core recipes so they fire reliably even on smith inputs
/// that have no `FromExports` core instance of their own.
///
/// Synthesis path: declare a `CoreType::Module` that exports a memory,
/// import a module of that type, instantiate it, alias the declared
/// memory export, and wrap the alias in a fresh `FromExports`. The
/// returned `&'a str` is `'static`, satisfying `'a` for any parsed
/// component.
fn ensure_core_alias_source<'a>(comp: &mut Component<'a>) -> (u32, ExternalKind, &'a str) {
    if let Some(found) = comp.instances.iter().enumerate().find_map(|(idx, inst)| {
        let Instance::FromExports(exports) = inst else {
            return None;
        };
        exports.first().map(|e| (idx as u32, e.kind, e.name))
    }) {
        return found;
    }

    let module_type = comp.add_core_type(CoreType::Module(
        vec![ModuleTypeDeclaration::Export {
            name: "m",
            ty: TypeRef::Memory(SYNTH_MEMORY_TYPE),
        }]
        .into_boxed_slice(),
    ));
    let imported = comp.add_import_core_module(ComponentImportName("wirm_src_mod"), *module_type);
    let inst = comp.add_core_instance(Instance::Instantiate {
        module_index: *imported,
        args: Vec::new().into_boxed_slice(),
    });
    let alias = comp.add_alias_core_instance_export(ExternalKind::Memory, *inst, "m");
    let mem_idx = *alias.unwrap_core_memory();
    let from_exports = comp.add_core_instance(Instance::FromExports(
        vec![Export {
            name: "wirm_src_e",
            kind: ExternalKind::Memory,
            index: mem_idx,
        }]
        .into_boxed_slice(),
    ));
    (*from_exports, ExternalKind::Memory, "wirm_src_e")
}

/// Recipe 0: empty `FromExports` core instance. Single forward-ref
/// hop; `WIRM_FUZZ_INJECTION_DEPTH` doesn't apply.
fn recipe_core_empty<'a>(comp: &mut Component<'a>) -> CoreInstanceId {
    comp.add_core_instance(Instance::FromExports(Vec::new().into_boxed_slice()))
}

/// Recipe 1: `FromExports` carrying one alias of an existing core
/// instance's export. Uses [`ensure_core_alias_source`] so it always
/// fires.
fn recipe_core_alias_export<'a>(comp: &mut Component<'a>) -> CoreInstanceId {
    let (src_inst_idx, kind, name) = ensure_core_alias_source(comp);
    let alias_id = comp.add_alias_core_instance_export(kind, src_inst_idx, name);
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
    comp.add_core_instance(Instance::FromExports(exports.into_boxed_slice()))
}

/// Recipe 2: real chain of core `Instantiate` instances. The first
/// instance comes from a source module that exports memory `m`; each
/// subsequent instance instantiates a "chain" module that imports
/// memory `m` from arg `src` and re-exports it, fed by the previous
/// instance via an `InstantiationArg` named `src`. Each new instance
/// depends on the previous via that arg.
fn recipe_core_instantiate_chain<'a>(comp: &mut Component<'a>, depth: usize) -> CoreInstanceId {
    let src_module_type = comp.add_core_type(CoreType::Module(
        vec![ModuleTypeDeclaration::Export {
            name: "m",
            ty: TypeRef::Memory(SYNTH_MEMORY_TYPE),
        }]
        .into_boxed_slice(),
    ));
    let src_module =
        comp.add_import_core_module(ComponentImportName("wirm_chain_src"), *src_module_type);
    let chain_module_type = comp.add_core_type(CoreType::Module(
        vec![
            ModuleTypeDeclaration::Import(Import {
                module: "src",
                name: "m",
                ty: TypeRef::Memory(SYNTH_MEMORY_TYPE),
            }),
            ModuleTypeDeclaration::Export {
                name: "m",
                ty: TypeRef::Memory(SYNTH_MEMORY_TYPE),
            },
        ]
        .into_boxed_slice(),
    ));
    let chain_module =
        comp.add_import_core_module(ComponentImportName("wirm_chain_mod"), *chain_module_type);

    let mut current = comp.add_core_instance(Instance::Instantiate {
        module_index: *src_module,
        args: Vec::new().into_boxed_slice(),
    });
    for _ in 0..depth.max(1) {
        current = comp.add_core_instance(Instance::Instantiate {
            module_index: *chain_module,
            args: vec![InstantiationArg {
                name: "src",
                kind: InstantiationArgKind::Instance,
                index: *current,
            }]
            .into_boxed_slice(),
        });
    }
    current
}

/// Recipe 3: chain of `FromExports` core instances where each
/// subsequent instance aliases an export of the previous. Uses
/// [`ensure_core_alias_source`] so it always fires.
fn recipe_core_alias_chain<'a>(comp: &mut Component<'a>, depth: usize) -> CoreInstanceId {
    let (mut current_inst_idx, current_kind, mut current_name) = ensure_core_alias_source(comp);
    let mut last_inst_id = None;
    for _ in 0..depth.max(1) {
        let alias_id =
            comp.add_alias_core_instance_export(current_kind, current_inst_idx, current_name);
        let raw_idx = match current_kind {
            ExternalKind::Func | ExternalKind::FuncExact => *alias_id.unwrap_core_func(),
            ExternalKind::Table => *alias_id.unwrap_core_table(),
            ExternalKind::Memory => *alias_id.unwrap_core_memory(),
            ExternalKind::Global => *alias_id.unwrap_core_global(),
            ExternalKind::Tag => *alias_id.unwrap_core_tag(),
        };
        let exports = vec![Export {
            name: "wirm_e",
            kind: current_kind,
            index: raw_idx,
        }];
        let inst = comp.add_core_instance(Instance::FromExports(exports.into_boxed_slice()));
        current_inst_idx = *inst;
        current_name = "wirm_e";
        last_inst_id = Some(inst);
    }
    last_inst_id.expect("depth >= 1")
}

// ── component-side ──────────────────────────────────────────────────

/// One main recipe per `ComponentExternalKind` variant, with several
/// sub-recipes per kind selected by `sub`. Each builds a fresh IR
/// item of the right kind, then appends a `ComponentInstantiationArg`
/// of that kind onto the target component instantiation. Returning
/// without injecting is fine — recipes that need pre-existing items
/// fall through that path when none exist.
fn try_inject_component_arg<'a>(
    comp: &mut Component<'a>,
    target_idx: usize,
    recipe: u8,
    sub: u8,
    aux_names: &[&'a str],
    primary_name: &'a str,
    round_tag: &'a str,
) {
    let depth = injection_depth();
    let item = match recipe % NUM_COMPONENT_KINDS {
        0 => dispatch_kind_module(comp, target_idx, sub, depth, aux_names),
        1 => recipe_kind_component(comp, sub, depth).map(|id| {
            (
                ComponentExternalKind::Component,
                *id,
                Injected::Component(id),
            )
        }),
        2 => recipe_kind_func(comp, sub, depth)
            .map(|id| (ComponentExternalKind::Func, *id, Injected::Func(id))),
        3 => recipe_kind_type(comp, sub, depth)
            .map(|id| (ComponentExternalKind::Type, *id, Injected::Type(id))),
        4 => recipe_kind_value(comp, sub, depth)
            .map(|id| (ComponentExternalKind::Value, *id, Injected::Value(id))),
        5 => recipe_kind_instance(comp, sub, depth)
            .map(|id| (ComponentExternalKind::Instance, *id, Injected::Instance(id))),
        _ => unreachable!(),
    };

    let Some((kind, index, injected)) = item else {
        return;
    };

    append_component_arg(comp, target_idx, primary_name, kind, index);

    // Orthogonal phase: re-export the freshly-added item under a
    // round-tagged + kind-specific name. Round tag prevents the two
    // recipe rounds from colliding when they pick the same kind.
    export_injected(comp, injected, round_tag);
}

/// Append a single `ComponentInstantiationArg` to the consumer at
/// `target_idx` in `comp.component_instance`. Shared by the regular
/// primary-arg append and the branching recipes' extra-arg appends.
fn append_component_arg<'a>(
    comp: &mut Component<'a>,
    target_idx: usize,
    name: &'a str,
    kind: ComponentExternalKind,
    index: u32,
) {
    let arg = ComponentInstantiationArg { name, kind, index };
    if let ComponentInstance::Instantiate { args, .. } = &mut comp.component_instance[target_idx]
    {
        let mut next = args.to_vec();
        next.push(arg);
        *args = next.into_boxed_slice();
    }
}

/// Polymorphic handle on a freshly-injected component-side item, so
/// the orthogonal export phase can route to the matching
/// `add_export_*` helper without re-deriving the kind.
enum Injected {
    Module(ModuleID),
    Component(ComponentId),
    Func(ComponentFunctionId),
    Type(ComponentTypeId),
    Value(ValueID),
    Instance(ComponentInstanceId),
}

/// Static export-name sets per round. Each entry is indexed by
/// kind: 0=Module, 1=Component, 2=Func, 3=Type, 4=Value, 5=Instance.
/// Using `&'static str` keeps lifetimes simple and avoids any
/// per-iteration allocation/leak for export names.
const EXPORTS_A: [&str; 6] = [
    "wirm_a_m", "wirm_a_c", "wirm_a_f", "wirm_a_t", "wirm_a_v", "wirm_a_i",
];
const EXPORTS_B: [&str; 6] = [
    "wirm_b_m", "wirm_b_c", "wirm_b_f", "wirm_b_t", "wirm_b_v", "wirm_b_i",
];

fn export_injected<'a>(comp: &mut Component<'a>, injected: Injected, round_tag: &str) {
    let names = if round_tag == "a" {
        &EXPORTS_A
    } else {
        &EXPORTS_B
    };
    match injected {
        Injected::Module(id) => {
            comp.add_export_core_module(ComponentExportName(names[0]), id, None);
        }
        Injected::Component(id) => {
            comp.add_export_component(ComponentExportName(names[1]), id, None);
        }
        Injected::Func(id) => {
            comp.add_export_component_func(ComponentExportName(names[2]), id, None);
        }
        Injected::Type(id) => {
            comp.add_export_component_type(ComponentExportName(names[3]), id, None);
        }
        Injected::Value(id) => {
            comp.add_export_component_value(ComponentExportName(names[4]), id, None);
        }
        Injected::Instance(id) => {
            comp.add_export_component_instance(ComponentExportName(names[5]), id, None);
        }
    }
}

// ── kind 0: Module ──────────────────────────────────────────────────

const NUM_KIND_MODULE_SUBS: u8 = 2;

/// Module-kind dispatch: takes `target_idx` and `aux_names` so the
/// branching sub-recipe can append extra args directly. Returns the
/// (kind, primary index, injected handle) tuple consumed by the
/// caller's regular primary-arg append + export phases.
fn dispatch_kind_module<'a>(
    comp: &mut Component<'a>,
    target_idx: usize,
    sub: u8,
    depth: usize,
    aux_names: &[&'a str],
) -> Option<(ComponentExternalKind, u32, Injected)> {
    let id = match sub % NUM_KIND_MODULE_SUBS {
        0 => recipe_module_import(comp),
        1 => recipe_module_branching(comp, target_idx, depth, aux_names),
        _ => unreachable!(),
    };
    Some((ComponentExternalKind::Module, *id, Injected::Module(id)))
}

/// Sub 0: empty `CoreType::Module` then an `import` of that type.
/// Two-hop dep chain; the import is what the new arg points at.
fn recipe_module_import<'a>(comp: &mut Component<'a>) -> ModuleID {
    let core_type = comp.add_core_type(CoreType::Module(Vec::new().into_boxed_slice()));
    comp.add_import_core_module(ComponentImportName("wirm_m"), *core_type)
}

/// Sub 1: branching. Build `depth` independent (core_type, import)
/// pairs and append all but the last as separate args to the
/// consumer under aux names; return the last so the caller appends
/// it as the primary `wirm_inj` arg. Tests one consumer pulling in
/// multiple new dep chains at once — module types are opaque from
/// the component POV so we can't chain them transitively, but we
/// can still stress the encoder's reorder with N concurrent
/// forward refs from a single consumer.
fn recipe_module_branching<'a>(
    comp: &mut Component<'a>,
    target_idx: usize,
    depth: usize,
    aux_names: &[&'a str],
) -> ModuleID {
    let n = depth.max(1);
    let mut last = None;
    for i in 0..n {
        let core_type = comp.add_core_type(CoreType::Module(Vec::new().into_boxed_slice()));
        let m = comp.add_import_core_module(ComponentImportName("wirm_m"), *core_type);
        if i + 1 < n {
            // Append as an extra arg under an aux name. `aux_names`
            // is sized depth-1 by the caller, so indexing 0..n-1 is
            // in-bounds for n <= depth.
            let name = aux_names.get(i).copied().unwrap_or("wirm_aux_overflow");
            append_component_arg(comp, target_idx, name, ComponentExternalKind::Module, *m);
        }
        last = Some(m);
    }
    last.expect("n >= 1")
}

// ── kind 1: Component ───────────────────────────────────────────────

const NUM_KIND_COMPONENT_SUBS: u8 = 2;

fn recipe_kind_component<'a>(
    comp: &mut Component<'a>,
    sub: u8,
    depth: usize,
) -> Option<ComponentId> {
    match sub % NUM_KIND_COMPONENT_SUBS {
        0 => Some(recipe_component_nested(comp, depth)),
        1 => Some(recipe_component_import(comp)),
        _ => unreachable!(),
    }
}

/// Sub 0: nest empty sub-components to `depth`. At depth 1 just an
/// empty sub-component; deeper levels add an inner primitive type so
/// the outer component genuinely depends on it.
fn recipe_component_nested<'a>(comp: &mut Component<'a>, depth: usize) -> ComponentId {
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

/// Sub 1: empty `ComponentType::Component` then an import of that
/// type. Two-hop dep chain.
fn recipe_component_import<'a>(comp: &mut Component<'a>) -> ComponentId {
    let ty = comp.add_component_type(ComponentType::Component(Vec::new().into_boxed_slice()));
    comp.add_import_component(ComponentImportName("wirm_c"), *ty)
}

// ── kind 2: Func ────────────────────────────────────────────────────

const NUM_KIND_FUNC_SUBS: u8 = 3;

fn recipe_kind_func<'a>(
    comp: &mut Component<'a>,
    sub: u8,
    depth: usize,
) -> Option<ComponentFunctionId> {
    match sub % NUM_KIND_FUNC_SUBS {
        0 => Some(recipe_func_import(comp)),
        1 => Some(recipe_func_alias_instance_export(comp)),
        2 => Some(recipe_func_import_chain(comp, depth)),
        _ => unreachable!(),
    }
}

/// Sub 0: empty `ComponentType::Func` then an import of that type.
fn recipe_func_import<'a>(comp: &mut Component<'a>) -> ComponentFunctionId {
    let func_ty = comp.add_component_type(ComponentType::Func(empty_func_type()));
    comp.add_import_component_func(ComponentImportName("wirm_f"), *func_ty)
}

/// Sub 1: build a fresh comp-instance with a single Func export, then
/// `alias_instance_export(Func, ...)` it back out. Four-hop chain
/// (func type → import → FromExports instance → alias).
fn recipe_func_alias_instance_export<'a>(comp: &mut Component<'a>) -> ComponentFunctionId {
    let func_ty = comp.add_component_type(ComponentType::Func(empty_func_type()));
    let imported_func = comp.add_import_component_func(ComponentImportName("wirm_f"), *func_ty);
    let inst = comp.add_component_instance(ComponentInstance::FromExports(
        vec![wasmparser::ComponentExport {
            name: ComponentExportName("f"),
            kind: ComponentExternalKind::Func,
            index: *imported_func,
            ty: None,
        }]
        .into_boxed_slice(),
    ));
    let alias = comp.add_alias_instance_export(ComponentExternalKind::Func, *inst, "f");
    alias.unwrap_component_func()
}

/// Sub 2: real chain through `Defined::List` nesting. Each iteration
/// adds a `List(Type(prev_defined))` defined type, then a `Func` type
/// whose result is that defined type, then a func import of that
/// func type. The final import transitively depends on the whole
/// defined-type chain, so the encoder must reorder `~3*depth` items.
fn recipe_func_import_chain<'a>(
    comp: &mut Component<'a>,
    depth: usize,
) -> ComponentFunctionId {
    let mut latest_defined = comp.add_component_type(ComponentType::Defined(
        ComponentDefinedType::Primitive(PrimitiveValType::U32),
    ));
    let mut last = None;
    for _ in 0..depth.max(1) {
        latest_defined = comp.add_component_type(ComponentType::Defined(
            ComponentDefinedType::List(ComponentValType::Type(*latest_defined)),
        ));
        let func_ty = comp.add_component_type(ComponentType::Func(ComponentFuncType {
            async_: false,
            params: Vec::new().into_boxed_slice(),
            result: Some(ComponentValType::Type(*latest_defined)),
        }));
        last = Some(comp.add_import_component_func(ComponentImportName("wirm_f"), *func_ty));
    }
    last.expect("depth >= 1")
}

fn empty_func_type<'a>() -> ComponentFuncType<'a> {
    ComponentFuncType {
        async_: false,
        params: Vec::new().into_boxed_slice(),
        result: None,
    }
}

// ── kind 3: Type ────────────────────────────────────────────────────

const NUM_KIND_TYPE_SUBS: u8 = 7;

fn recipe_kind_type<'a>(
    comp: &mut Component<'a>,
    sub: u8,
    depth: usize,
) -> Option<ComponentTypeId> {
    match sub % NUM_KIND_TYPE_SUBS {
        0 => Some(recipe_type_primitive(comp)),
        1 => Some(recipe_type_import_bounds(comp)),
        2 => Some(recipe_type_list_chain(comp, depth)),
        3 => Some(recipe_type_defined_variants(comp, sub)),
        4 => Some(recipe_type_diamond(comp, depth)),
        5 => Some(recipe_type_via_outer_alias(comp)),
        6 => Some(recipe_type_resource(comp, sub)),
        _ => unreachable!(),
    }
}

/// Sub 5: exercise `add_alias_outer`. Add a primitive type at parent
/// scope; build a sub-component that outer-aliases it (depth = 1) and
/// re-exports it as a type; instantiate the sub-component from parent;
/// alias the new instance's type export back into the parent's
/// CompType space. The consumer's arg points at that final alias.
///
/// This is the only path the fuzz target exercises that produces a
/// `RefKind` with `depth > 0` (the outer alias's reference back into
/// the parent scope). Bugs in `comp_at(depth)` resolution or the
/// scope stack management would surface here.
fn recipe_type_via_outer_alias<'a>(comp: &mut Component<'a>) -> ComponentTypeId {
    let parent_ty = comp.add_component_type(ComponentType::Defined(
        ComponentDefinedType::Primitive(PrimitiveValType::U32),
    ));
    let parent_ty_idx = *parent_ty;
    let sub_comp = comp.add_component(|inner| {
        let alias = inner.add_alias_outer(ComponentOuterAliasKind::Type, 1, parent_ty_idx);
        let aliased_ty = alias.unwrap_component_type();
        inner.add_export_component_type(ComponentExportName("t"), aliased_ty, None);
    });
    let inst = comp.add_component_instance(ComponentInstance::Instantiate {
        component_index: *sub_comp,
        args: Vec::new().into_boxed_slice(),
    });
    let exported = comp.add_alias_instance_export(ComponentExternalKind::Type, *inst, "t");
    exported.unwrap_component_type()
}

/// Sub 0: a fresh primitive defined type. Single hop; doesn't chain.
fn recipe_type_primitive<'a>(comp: &mut Component<'a>) -> ComponentTypeId {
    comp.add_component_type(ComponentType::Defined(ComponentDefinedType::Primitive(
        PrimitiveValType::U32,
    )))
}

/// Sub 1: `add_import_component_type` with `SubResource` bounds.
fn recipe_type_import_bounds<'a>(comp: &mut Component<'a>) -> ComponentTypeId {
    comp.add_import_component_type(ComponentImportName("wirm_t"), TypeBounds::SubResource)
}

/// Sub 2: depth-N chain of `Defined::List(Type(prev))`. Each level
/// genuinely depends on the previous via index — the encoder must
/// order them all ahead of the consumer.
fn recipe_type_list_chain<'a>(comp: &mut Component<'a>, depth: usize) -> ComponentTypeId {
    let mut current = comp.add_component_type(ComponentType::Defined(
        ComponentDefinedType::Primitive(PrimitiveValType::U32),
    ));
    for _ in 0..depth.max(1) {
        current = comp.add_component_type(ComponentType::Defined(ComponentDefinedType::List(
            ComponentValType::Type(*current),
        )));
    }
    current
}

/// Sub 4: diamond. Build a shared primitive deep dep, then two
/// independent `Defined::List` chains of length `depth` that both
/// terminate at the shared primitive, then wrap both chain heads in
/// a `Defined::Tuple`. The tuple's two field types both reach the
/// shared primitive via disjoint paths, so the encoder's
/// `seen`-tracked topological walk must dedupe the shared node when
/// it's visited from the second chain. A bug in the dedup logic
/// would either re-emit the shared item or skip it entirely.
fn recipe_type_diamond<'a>(comp: &mut Component<'a>, depth: usize) -> ComponentTypeId {
    let shared = comp.add_component_type(ComponentType::Defined(
        ComponentDefinedType::Primitive(PrimitiveValType::U32),
    ));
    let left = list_chain_to(comp, *shared, depth);
    let right = list_chain_to(comp, *shared, depth);
    comp.add_component_type(ComponentType::Defined(ComponentDefinedType::Tuple(
        vec![
            ComponentValType::Type(*left),
            ComponentValType::Type(*right),
        ]
        .into_boxed_slice(),
    )))
}

/// Helper for `recipe_type_diamond`: append `depth` `Defined::List`
/// types, each wrapping `Type(prev)`, starting from `start_idx`.
/// Returns the head of the chain (the last list added).
fn list_chain_to<'a>(comp: &mut Component<'a>, start_idx: u32, depth: usize) -> ComponentTypeId {
    let mut current_idx = start_idx;
    let mut last = None;
    for _ in 0..depth.max(1) {
        let next = comp.add_component_type(ComponentType::Defined(ComponentDefinedType::List(
            ComponentValType::Type(current_idx),
        )));
        current_idx = *next;
        last = Some(next);
    }
    last.expect("depth >= 1")
}

/// Sub 3: rotate among many `ComponentDefinedType` variants
/// (Record / Tuple / Option / Result / Variant / Flags / Enum /
/// Map / FixedLengthList / Future / Stream), each referencing a
/// fresh Primitive defined type where applicable. Spreads coverage
/// across the defined-type enum without dedicated recipes for each.
fn recipe_type_defined_variants<'a>(comp: &mut Component<'a>, sub: u8) -> ComponentTypeId {
    let prim = comp.add_component_type(ComponentType::Defined(
        ComponentDefinedType::Primitive(PrimitiveValType::U32),
    ));
    let val = ComponentValType::Type(*prim);
    let key_str = ComponentValType::Primitive(PrimitiveValType::String);
    let defined = match sub % 11 {
        0 => ComponentDefinedType::Record(vec![("x", val)].into_boxed_slice()),
        1 => ComponentDefinedType::Tuple(vec![val].into_boxed_slice()),
        2 => ComponentDefinedType::Option(val),
        3 => ComponentDefinedType::Result {
            ok: Some(val),
            err: None,
        },
        4 => ComponentDefinedType::Variant(
            vec![VariantCase {
                name: "v",
                ty: Some(val),
            }]
            .into_boxed_slice(),
        ),
        5 => ComponentDefinedType::Flags(vec!["a", "b"].into_boxed_slice()),
        6 => ComponentDefinedType::Enum(vec!["x", "y"].into_boxed_slice()),
        7 => ComponentDefinedType::Map(key_str, val),
        8 => ComponentDefinedType::FixedLengthList(val, 2),
        9 => ComponentDefinedType::Future(Some(val)),
        10 => ComponentDefinedType::Stream(Some(val)),
        _ => unreachable!(),
    };
    comp.add_component_type(ComponentType::Defined(defined))
}

/// Sub 6: define two `Resource` types. The first has no
/// destructor; canon `ResourceNew/Drop/Rep` are created for it and
/// wrapped in a `FromExports` core instance so the encoder reaches
/// them via dep traversal. The second resource uses the first's
/// canon `ResourceDrop` core func as its destructor — both have
/// `[i32] -> []` signature, so validation accepts it. This adds a
/// `Resource → CoreFunc(canon) → Resource` ref edge the encoder
/// must resolve, and exercises the resource-with-dtor code path
/// that an `dtor: None` resource doesn't.
fn recipe_type_resource<'a>(comp: &mut Component<'a>, sub: u8) -> ComponentTypeId {
    let resource_a = comp.add_component_type(ComponentType::Resource {
        rep: ValType::I32,
        dtor: None,
    });
    let canon_new = comp.add_canon_func(CanonicalFunction::ResourceNew {
        resource: *resource_a,
    });
    let canon_drop = comp.add_canon_func(CanonicalFunction::ResourceDrop {
        resource: *resource_a,
    });
    let canon_rep = comp.add_canon_func(CanonicalFunction::ResourceRep {
        resource: *resource_a,
    });
    let canon_new_idx = *canon_new.unwrap_core();
    let canon_drop_idx = *canon_drop.unwrap_core();
    let canon_rep_idx = *canon_rep.unwrap_core();
    let _wrapper = comp.add_core_instance(Instance::FromExports(
        vec![
            Export {
                name: "new",
                kind: ExternalKind::Func,
                index: canon_new_idx,
            },
            Export {
                name: "drop",
                kind: ExternalKind::Func,
                index: canon_drop_idx,
            },
            Export {
                name: "rep",
                kind: ExternalKind::Func,
                index: canon_rep_idx,
            },
        ]
        .into_boxed_slice(),
    ));
    // Resource B: uses A's canon ResourceDrop as its dtor. Signature
    // matches (`[i32] -> []`); the encoder must resolve the dtor
    // ref through CoreFunc space back to the canon.
    let resource_b = comp.add_component_type(ComponentType::Resource {
        rep: ValType::I32,
        dtor: Some(canon_drop_idx),
    });
    // Pick A or B based on the sub byte so both resources see Own
    // and Borrow paths across iterations.
    let target = if (sub / 2) % 2 == 0 {
        resource_a
    } else {
        resource_b
    };
    let defined = if sub % 2 == 0 {
        ComponentDefinedType::Own(*target)
    } else {
        ComponentDefinedType::Borrow(*target)
    };
    comp.add_component_type(ComponentType::Defined(defined))
}

// ── kind 4: Value ───────────────────────────────────────────────────

const NUM_KIND_VALUE_SUBS: u8 = 2;

fn recipe_kind_value<'a>(
    comp: &mut Component<'a>,
    sub: u8,
    depth: usize,
) -> Option<ValueID> {
    match sub % NUM_KIND_VALUE_SUBS {
        0 => Some(recipe_value_primitive(comp)),
        1 => Some(recipe_value_via_list_chain(comp, depth)),
        _ => unreachable!(),
    }
}

/// Sub 0: import a value of primitive type. Single-hop forward ref.
fn recipe_value_primitive<'a>(comp: &mut Component<'a>) -> ValueID {
    comp.add_import_component_value(
        ComponentImportName("wirm_v"),
        ComponentValType::Primitive(PrimitiveValType::U32),
    )
}

/// Sub 1: build a depth-N `Defined::List` chain, then import a value
/// of `Type(last)`. Genuine N+1 hop dep chain ending in the value.
fn recipe_value_via_list_chain<'a>(comp: &mut Component<'a>, depth: usize) -> ValueID {
    let mut current = comp.add_component_type(ComponentType::Defined(
        ComponentDefinedType::Primitive(PrimitiveValType::U32),
    ));
    for _ in 0..depth.max(1) {
        current = comp.add_component_type(ComponentType::Defined(ComponentDefinedType::List(
            ComponentValType::Type(*current),
        )));
    }
    comp.add_import_component_value(
        ComponentImportName("wirm_v"),
        ComponentValType::Type(*current),
    )
}

// ── kind 5: Instance ────────────────────────────────────────────────

const NUM_KIND_INSTANCE_SUBS: u8 = 5;

fn recipe_kind_instance<'a>(
    comp: &mut Component<'a>,
    sub: u8,
    depth: usize,
) -> Option<ComponentInstanceId> {
    match sub % NUM_KIND_INSTANCE_SUBS {
        0 => Some(recipe_instance_empty(comp)),
        1 => Some(recipe_instance_via_subcomponent(comp, depth)),
        2 => Some(recipe_instance_import(comp)),
        3 => Some(recipe_instance_alias_chain(comp, depth)),
        4 => Some(recipe_instance_cross_kind_chain(comp, depth)),
        _ => unreachable!(),
    }
}

/// Sub 4: cross-kind glue chain. Build a deep `Defined::List` chain
/// on the type side, wrap its tail in a `Func` type's result, import
/// a func of that type, and expose it via a `FromExports` instance.
/// Consumer's arg points at the instance, so the encoder's walk
/// crosses `CompInst → CompFunc → CompType → CompType...` in a
/// single chain. Hits more arms of `collect_deps`' space dispatch
/// per single forward-ref edge than any same-kind chain.
fn recipe_instance_cross_kind_chain<'a>(
    comp: &mut Component<'a>,
    depth: usize,
) -> ComponentInstanceId {
    let mut latest_defined = comp.add_component_type(ComponentType::Defined(
        ComponentDefinedType::Primitive(PrimitiveValType::U32),
    ));
    for _ in 0..depth.max(1) {
        latest_defined = comp.add_component_type(ComponentType::Defined(
            ComponentDefinedType::List(ComponentValType::Type(*latest_defined)),
        ));
    }
    let func_ty = comp.add_component_type(ComponentType::Func(ComponentFuncType {
        async_: false,
        params: Vec::new().into_boxed_slice(),
        result: Some(ComponentValType::Type(*latest_defined)),
    }));
    let imported_func = comp.add_import_component_func(ComponentImportName("wirm_f"), *func_ty);
    comp.add_component_instance(ComponentInstance::FromExports(
        vec![wasmparser::ComponentExport {
            name: ComponentExportName("f"),
            kind: ComponentExternalKind::Func,
            index: *imported_func,
            ty: None,
        }]
        .into_boxed_slice(),
    ))
}

/// Sub 0: empty `FromExports` component instance.
fn recipe_instance_empty<'a>(comp: &mut Component<'a>) -> ComponentInstanceId {
    comp.add_component_instance(ComponentInstance::FromExports(
        Vec::new().into_boxed_slice(),
    ))
}

/// Sub 1: build a depth-N nested sub-component, then `Instantiate`
/// it. Pulls the whole sub-component chain into the dep set.
fn recipe_instance_via_subcomponent<'a>(
    comp: &mut Component<'a>,
    depth: usize,
) -> ComponentInstanceId {
    if depth <= 1 {
        return comp.add_component_instance(ComponentInstance::FromExports(
            Vec::new().into_boxed_slice(),
        ));
    }
    let inner = recipe_component_nested(comp, depth - 1);
    comp.add_component_instance(ComponentInstance::Instantiate {
        component_index: *inner,
        args: Vec::new().into_boxed_slice(),
    })
}

/// Sub 2: empty `ComponentType::Instance` then an import of that
/// type. Two-hop dep chain.
fn recipe_instance_import<'a>(comp: &mut Component<'a>) -> ComponentInstanceId {
    let ty = comp.add_component_type(ComponentType::Instance(Vec::new().into_boxed_slice()));
    comp.add_import_component_instance(ComponentImportName("wirm_i"), *ty)
}

/// Sub 3: chain of `FromExports` component instances where each
/// aliases a Func export of the previous. Real depth-N chain via
/// instance-export aliases.
fn recipe_instance_alias_chain<'a>(
    comp: &mut Component<'a>,
    depth: usize,
) -> ComponentInstanceId {
    let func_ty = comp.add_component_type(ComponentType::Func(empty_func_type()));
    let mut current_func = *comp.add_import_component_func(ComponentImportName("wirm_f"), *func_ty);
    let mut last_inst = comp.add_component_instance(ComponentInstance::FromExports(
        vec![wasmparser::ComponentExport {
            name: ComponentExportName("f"),
            kind: ComponentExternalKind::Func,
            index: current_func,
            ty: None,
        }]
        .into_boxed_slice(),
    ));
    for _ in 0..depth.max(1) {
        let alias = comp.add_alias_instance_export(ComponentExternalKind::Func, *last_inst, "f");
        current_func = *alias.unwrap_component_func();
        last_inst = comp.add_component_instance(ComponentInstance::FromExports(
            vec![wasmparser::ComponentExport {
                name: ComponentExportName("f"),
                kind: ComponentExternalKind::Func,
                index: current_func,
                ty: None,
            }]
            .into_boxed_slice(),
        ));
    }
    last_inst
}
