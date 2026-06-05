//! wasm-smith Component → wirm parse → record events from both walkers via a
//! `ComponentVisitor` → assert the two recordings agree → bound-check every
//! `section_idx` against wasmparser's view of the binary.
//!
//! The fuzz-scale analog of `tests::check_event_validity` in
//! `src/ir/component/visitor/tests.rs`. That test uses the internal
//! `VisitEvent` enum directly; we don't, to keep wirm's public API surface
//! clean. Instead the `Recorder` below records a parallel `Evt` per visitor
//! callback using only the `ComponentVisitor` trait.
//!
//! Design per fuzz/DECISIONS.md — parse/pre-validation failures silent, any
//! post-parse divergence or panic is a bug.

#![no_main]

use libfuzzer_sys::fuzz_target;
use wasm_smith::Component as SmithComponent;
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance,
    ComponentStartFunction, ComponentType, ComponentTypeDeclaration, CoreType,
    Instance as WpInstance, InstanceTypeDeclaration, ModuleTypeDeclaration, Parser, Payload,
    SubType,
};
use wirm::ir::component::visitor::{
    walk_structural, walk_topological, ComponentVisitor, ItemKind, VisitCtx,
};
use wirm::ir::types::CustomSection;

// --- Recording visitor --------------------------------------------------

/// Stable, `PartialEq`-comparable tag for each visitor callback.
/// Captures only the fields that don't depend on IR pointer identity —
/// ids, kinds, section_idx, decl_idx. References to IR nodes are dropped
/// because both walks operate on the same `&Component`, so pointer
/// equality wouldn't add any information the other fields don't already
/// carry.
#[derive(Debug, PartialEq)]
enum Evt {
    EnterRootComp,
    ExitRootComp,
    EnterComp { id: u32, sec: Option<usize> },
    ExitComp { id: u32, sec: Option<usize> },
    Module { id: u32, sec: Option<usize> },
    CompType { id: u32, sec: Option<usize> },
    EnterCompTypeInst { id: u32, sec: Option<usize> },
    ExitCompTypeInst { id: u32, sec: Option<usize> },
    EnterCompTypeComp { id: u32, sec: Option<usize> },
    ExitCompTypeComp { id: u32, sec: Option<usize> },
    CompTypeDecl { decl_idx: usize, id: u32, sec: Option<usize> },
    InstTypeDecl { decl_idx: usize, id: u32, sec: Option<usize> },
    CompInstance { id: u32, sec: Option<usize> },
    Canon { kind: ItemKind, id: u32, sec: Option<usize> },
    Alias { kind: ItemKind, id: u32, sec: Option<usize> },
    CompImport { kind: ItemKind, id: u32, sec: Option<usize> },
    CompExport { kind: ItemKind, id: u32, sec: Option<usize> },
    EnterCoreRecGroup { count: usize, sec: Option<usize> },
    ExitCoreRecGroup { sec: Option<usize> },
    CoreSubtype { id: u32, sec: Option<usize> },
    EnterCoreModuleType { id: u32, sec: Option<usize> },
    ExitCoreModuleType { id: u32, sec: Option<usize> },
    ModuleTypeDecl { decl_idx: usize, id: u32, sec: Option<usize> },
    CoreInstance { id: u32, sec: Option<usize> },
    CustomSection { sec: Option<usize> },
    StartSection { sec: Option<usize> },
}

struct Recorder {
    events: Vec<Evt>,
}

impl<'a> ComponentVisitor<'a> for Recorder {
    fn enter_root_component(&mut self, _cx: &VisitCtx<'a>, _c: &wirm::Component<'a>) {
        self.events.push(Evt::EnterRootComp);
    }
    fn exit_root_component(&mut self, _cx: &VisitCtx<'a>, _c: &wirm::Component<'a>) {
        self.events.push(Evt::ExitRootComp);
    }
    fn enter_component(&mut self, cx: &VisitCtx<'a>, id: u32, _c: &wirm::Component<'a>) {
        self.events.push(Evt::EnterComp { id, sec: cx.curr_section_idx() });
    }
    fn exit_component(&mut self, cx: &VisitCtx<'a>, id: u32, _c: &wirm::Component<'a>) {
        self.events.push(Evt::ExitComp { id, sec: cx.curr_section_idx() });
    }
    fn visit_module(&mut self, cx: &VisitCtx<'a>, id: u32, _m: &wirm::Module<'a>) {
        self.events.push(Evt::Module { id, sec: cx.curr_section_idx() });
    }
    fn visit_comp_type(&mut self, cx: &VisitCtx<'a>, id: u32, _t: &ComponentType<'a>) {
        self.events.push(Evt::CompType { id, sec: cx.curr_section_idx() });
    }
    fn enter_component_type_inst(&mut self, cx: &VisitCtx<'a>, id: u32, _t: &ComponentType<'a>) {
        self.events.push(Evt::EnterCompTypeInst { id, sec: cx.curr_section_idx() });
    }
    fn exit_component_type_inst(&mut self, cx: &VisitCtx<'a>, id: u32, _t: &ComponentType<'a>) {
        self.events.push(Evt::ExitCompTypeInst { id, sec: cx.curr_section_idx() });
    }
    fn enter_component_type_comp(&mut self, cx: &VisitCtx<'a>, id: u32, _t: &ComponentType<'a>) {
        self.events.push(Evt::EnterCompTypeComp { id, sec: cx.curr_section_idx() });
    }
    fn exit_component_type_comp(&mut self, cx: &VisitCtx<'a>, id: u32, _t: &ComponentType<'a>) {
        self.events.push(Evt::ExitCompTypeComp { id, sec: cx.curr_section_idx() });
    }
    fn visit_comp_type_decl(
        &mut self,
        cx: &VisitCtx<'a>,
        decl_idx: usize,
        id: u32,
        _p: &ComponentType<'a>,
        _d: &ComponentTypeDeclaration<'a>,
    ) {
        self.events.push(Evt::CompTypeDecl { decl_idx, id, sec: cx.curr_section_idx() });
    }
    fn visit_inst_type_decl(
        &mut self,
        cx: &VisitCtx<'a>,
        decl_idx: usize,
        id: u32,
        _p: &ComponentType<'a>,
        _d: &InstanceTypeDeclaration<'a>,
    ) {
        self.events.push(Evt::InstTypeDecl { decl_idx, id, sec: cx.curr_section_idx() });
    }
    fn visit_comp_instance(&mut self, cx: &VisitCtx<'a>, id: u32, _i: &ComponentInstance<'a>) {
        self.events.push(Evt::CompInstance { id, sec: cx.curr_section_idx() });
    }
    fn visit_canon(&mut self, cx: &VisitCtx<'a>, kind: ItemKind, id: u32, _c: &CanonicalFunction) {
        self.events.push(Evt::Canon { kind, id, sec: cx.curr_section_idx() });
    }
    fn visit_alias(&mut self, cx: &VisitCtx<'a>, kind: ItemKind, id: u32, _a: &ComponentAlias<'a>) {
        self.events.push(Evt::Alias { kind, id, sec: cx.curr_section_idx() });
    }
    fn visit_comp_import(
        &mut self,
        cx: &VisitCtx<'a>,
        kind: ItemKind,
        id: u32,
        _i: &ComponentImport<'a>,
    ) {
        self.events.push(Evt::CompImport { kind, id, sec: cx.curr_section_idx() });
    }
    fn visit_comp_export(
        &mut self,
        cx: &VisitCtx<'a>,
        kind: ItemKind,
        id: u32,
        _e: &ComponentExport<'a>,
    ) {
        self.events.push(Evt::CompExport { kind, id, sec: cx.curr_section_idx() });
    }
    fn enter_core_rec_group(&mut self, cx: &VisitCtx<'a>, count: usize, _t: &CoreType<'a>) {
        self.events.push(Evt::EnterCoreRecGroup { count, sec: cx.curr_section_idx() });
    }
    fn visit_core_subtype(&mut self, cx: &VisitCtx<'a>, id: u32, _s: &SubType) {
        self.events.push(Evt::CoreSubtype { id, sec: cx.curr_section_idx() });
    }
    fn exit_core_rec_group(&mut self, cx: &VisitCtx<'a>) {
        self.events.push(Evt::ExitCoreRecGroup { sec: cx.curr_section_idx() });
    }
    fn enter_core_module_type(&mut self, cx: &VisitCtx<'a>, id: u32, _t: &CoreType<'a>) {
        self.events.push(Evt::EnterCoreModuleType { id, sec: cx.curr_section_idx() });
    }
    fn visit_module_type_decl(
        &mut self,
        cx: &VisitCtx<'a>,
        decl_idx: usize,
        id: u32,
        _p: &CoreType<'a>,
        _d: &ModuleTypeDeclaration<'a>,
    ) {
        self.events.push(Evt::ModuleTypeDecl { decl_idx, id, sec: cx.curr_section_idx() });
    }
    fn exit_core_module_type(&mut self, cx: &VisitCtx<'a>, id: u32, _t: &CoreType<'a>) {
        self.events.push(Evt::ExitCoreModuleType { id, sec: cx.curr_section_idx() });
    }
    fn visit_core_instance(&mut self, cx: &VisitCtx<'a>, id: u32, _i: &WpInstance<'a>) {
        self.events.push(Evt::CoreInstance { id, sec: cx.curr_section_idx() });
    }
    fn visit_custom_section(&mut self, cx: &VisitCtx<'a>, _s: &CustomSection<'a>) {
        self.events.push(Evt::CustomSection { sec: cx.curr_section_idx() });
    }
    fn visit_start_section(&mut self, cx: &VisitCtx<'a>, _s: &ComponentStartFunction) {
        self.events.push(Evt::StartSection { sec: cx.curr_section_idx() });
    }
}

// --- wasmparser section cross-check (root only) ------------------------
//
// For the root component we verify, per walker event: the event's
// `section_idx` (= position in wirm's `comp.sections`) indexes into a
// wasmparser-derived section list whose entry at that position has the
// matching kind. This pins down not just count/ordering but that each
// item is attributed to the right binary section.
//
// The mapping is 1:1 because wirm adds exactly one entry to `comp.sections`
// per depth-1 wasmparser payload (no cross-call folding in `add_to_sections`,
// and `get_sections_for_{core,comp}_ty` collapse the whole payload to one
// kind — so the granularity lines up). The two exceptions are mirrored
// here: `Payload::Version`/`End` produce no entry, and ComponentName
// custom sections are consumed for the name-map without adding to
// `comp.sections`.
//
// Per-subcomponent equivalents would need a parallel traversal through
// each `ComponentSection`'s sub-parser. Doable, but the root check
// already catches the class of bugs we're protecting against.

/// Section kinds we can attribute an event to. One-to-one with the
/// `ComponentSection` variants wirm stores in `comp.sections`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    CoreModule,
    Component,
    CoreType,
    ComponentType,
    CoreInstance,
    ComponentInstance,
    Alias,
    Canon,
    ComponentImport,
    ComponentExport,
    ComponentStart,
    Custom,
}

/// Ordered root-level section kinds as wasmparser sees them, aligned to
/// wirm's `comp.sections` indexing rules (one entry per depth-1 payload,
/// ComponentName custom sections excluded).
fn wasmparser_root_kinds(bytes: &[u8]) -> Vec<Kind> {
    let mut kinds = Vec::new();
    let mut depth: usize = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        let Ok(payload) = payload else { break };
        match &payload {
            Payload::Version { .. } => {
                depth += 1;
                continue;
            }
            Payload::End(_) => {
                if depth == 1 {
                    return kinds;
                }
                depth -= 1;
                continue;
            }
            _ => {}
        }
        if depth != 1 {
            continue;
        }
        let kind = match &payload {
            Payload::ModuleSection { .. } => Kind::CoreModule,
            Payload::ComponentSection { .. } => Kind::Component,
            Payload::CoreTypeSection(_) => Kind::CoreType,
            Payload::ComponentTypeSection(_) => Kind::ComponentType,
            Payload::InstanceSection(_) => Kind::CoreInstance,
            Payload::ComponentInstanceSection(_) => Kind::ComponentInstance,
            Payload::ComponentAliasSection(_) => Kind::Alias,
            Payload::ComponentCanonicalSection(_) => Kind::Canon,
            Payload::ComponentImportSection(_) => Kind::ComponentImport,
            Payload::ComponentExportSection(_) => Kind::ComponentExport,
            Payload::ComponentStartSection { .. } => Kind::ComponentStart,
            Payload::CustomSection(r) => match r.as_known() {
                // wirm consumes this into its name map, doesn't add to comp.sections.
                wasmparser::KnownCustom::ComponentName(_) => continue,
                _ => Kind::Custom,
            },
            _ => continue,
        };
        kinds.push(kind);
    }
    kinds
}

/// Kind mapping used *only* when the event is at nested-body depth 0
/// in the root component (see `check_root_sections_vs_wasmparser`).
/// Events that can only appear strictly nested (`*TypeDecl`) return
/// `None` because their kind is never checked.
fn kind_of_event(evt: &Evt) -> Option<Kind> {
    Some(match evt {
        Evt::Module { .. } => Kind::CoreModule,
        Evt::EnterComp { .. } | Evt::ExitComp { .. } => Kind::Component,
        Evt::CompInstance { .. } => Kind::ComponentInstance,
        Evt::CoreInstance { .. } => Kind::CoreInstance,
        Evt::Canon { .. } => Kind::Canon,
        Evt::Alias { .. } => Kind::Alias,
        Evt::CompImport { .. } => Kind::ComponentImport,
        Evt::CompExport { .. } => Kind::ComponentExport,
        Evt::CustomSection { .. } => Kind::Custom,
        Evt::StartSection { .. } => Kind::ComponentStart,
        Evt::CompType { .. }
        | Evt::EnterCompTypeInst { .. }
        | Evt::ExitCompTypeInst { .. }
        | Evt::EnterCompTypeComp { .. }
        | Evt::ExitCompTypeComp { .. } => Kind::ComponentType,
        Evt::EnterCoreRecGroup { .. }
        | Evt::ExitCoreRecGroup { .. }
        | Evt::CoreSubtype { .. }
        | Evt::EnterCoreModuleType { .. }
        | Evt::ExitCoreModuleType { .. } => Kind::CoreType,
        // These only appear strictly inside a nested body — kind is
        // never checked at nested-depth 0.
        Evt::CompTypeDecl { .. }
        | Evt::InstTypeDecl { .. }
        | Evt::ModuleTypeDecl { .. } => return None,
        Evt::EnterRootComp | Evt::ExitRootComp => return None,
    })
}

/// For each walker event at root-component depth:
///   * `section_idx` is in range against wasmparser's root section list,
///   * `section_idx` is monotonically non-decreasing,
///   * *when the event is at nested-body depth 0* (i.e. not inside a
///     `ComponentType.Instance|Component` or `CoreModuleType` body),
///     the kind wasmparser records at that index matches the event's
///     kind.
///
/// The nested-body carve-out is required because walker events emitted
/// inside a ComponentType body keep the outer section's `sec` value —
/// so e.g. an `EnterCoreRecGroup` inside an `Instance` type body points
/// to the `ComponentType` section, not a `CoreType` section.
fn check_root_sections_vs_wasmparser(events: &[Evt], expected: &[Kind]) {
    let mut comp_depth: usize = 0; // inside subcomponent(s)
    let mut nested: usize = 0; // inside CompType body or CoreModuleType body
    let mut last_sec: Option<usize> = None;
    for evt in events {
        match evt {
            Evt::EnterRootComp | Evt::ExitRootComp => continue,
            Evt::EnterComp { sec, .. } | Evt::ExitComp { sec, .. } => {
                if comp_depth == 0 && nested == 0 {
                    check_one(evt, *sec, expected, &mut last_sec, nested);
                }
                if matches!(evt, Evt::EnterComp { .. }) {
                    comp_depth += 1;
                } else {
                    comp_depth = comp_depth.saturating_sub(1);
                }
            }
            _ => {
                if comp_depth != 0 {
                    continue;
                }
                let opens = matches!(
                    evt,
                    Evt::EnterCompTypeInst { .. }
                        | Evt::EnterCompTypeComp { .. }
                        | Evt::EnterCoreModuleType { .. }
                );
                let closes = matches!(
                    evt,
                    Evt::ExitCompTypeInst { .. }
                        | Evt::ExitCompTypeComp { .. }
                        | Evt::ExitCoreModuleType { .. }
                );
                // Exits are attributed to the OUTER section (same `sec`
                // as their matching Enter). Decrement before the kind
                // check so an Exit at nested-depth 0 does get checked.
                if closes {
                    nested = nested.saturating_sub(1);
                }
                check_one(evt, section_idx_of(evt), expected, &mut last_sec, nested);
                if opens {
                    nested += 1;
                }
            }
        }
    }
}

fn check_one(
    evt: &Evt,
    sec: Option<usize>,
    expected: &[Kind],
    last_sec: &mut Option<usize>,
    nested: usize,
) {
    let Some(s) = sec else { return };
    assert!(
        s < expected.len(),
        "root section_idx {s} out of range (wasmparser says {} root sections): {evt:?}",
        expected.len()
    );
    if let Some(prev) = *last_sec {
        assert!(
            s >= prev,
            "root section_idx went backward: {prev} -> {s}: {evt:?}"
        );
    }
    *last_sec = Some(s);
    if nested == 0 {
        if let Some(got_kind) = kind_of_event(evt) {
            assert_eq!(
                expected[s], got_kind,
                "walker attributed event to section {s} ({got_kind:?}), \
                 but wasmparser says section {s} is {:?}: {evt:?}",
                expected[s]
            );
        }
    }
}

fn section_idx_of(evt: &Evt) -> Option<usize> {
    match *evt {
        Evt::EnterRootComp | Evt::ExitRootComp => None,
        Evt::EnterComp { sec, .. }
        | Evt::ExitComp { sec, .. }
        | Evt::Module { sec, .. }
        | Evt::CompType { sec, .. }
        | Evt::EnterCompTypeInst { sec, .. }
        | Evt::ExitCompTypeInst { sec, .. }
        | Evt::EnterCompTypeComp { sec, .. }
        | Evt::ExitCompTypeComp { sec, .. }
        | Evt::CompTypeDecl { sec, .. }
        | Evt::InstTypeDecl { sec, .. }
        | Evt::CompInstance { sec, .. }
        | Evt::Canon { sec, .. }
        | Evt::Alias { sec, .. }
        | Evt::CompImport { sec, .. }
        | Evt::CompExport { sec, .. }
        | Evt::EnterCoreRecGroup { sec, .. }
        | Evt::ExitCoreRecGroup { sec }
        | Evt::CoreSubtype { sec, .. }
        | Evt::EnterCoreModuleType { sec, .. }
        | Evt::ExitCoreModuleType { sec, .. }
        | Evt::ModuleTypeDecl { sec, .. }
        | Evt::CoreInstance { sec, .. }
        | Evt::CustomSection { sec }
        | Evt::StartSection { sec } => sec,
    }
}

// --- Target ------------------------------------------------------------

fuzz_target!(|smith: SmithComponent| {
    let bytes = smith.to_bytes();

    if wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&bytes)
        .is_err()
    {
        return;
    }

    let comp = match wirm::Component::parse(&bytes, false, false, false) {
        Ok(c) => c,
        Err(_) => return,
    };

    // 1. Event equivalence: structural and topological walks produce the
    //    same event sequence on a smith-generated component.
    let mut r_struct = Recorder { events: Vec::new() };
    walk_structural(&comp, &mut r_struct);
    let mut r_topo = Recorder { events: Vec::new() };
    walk_topological(&comp, &mut r_topo);

    assert_eq!(
        r_struct.events.len(),
        r_topo.events.len(),
        "walkers produced different event counts: structural={}, topological={}",
        r_struct.events.len(),
        r_topo.events.len(),
    );
    for (i, (a, b)) in r_struct.events.iter().zip(r_topo.events.iter()).enumerate() {
        assert_eq!(
            a, b,
            "walkers diverge at event #{i}: structural={a:?}, topological={b:?}"
        );
    }

    // 2. Each root-component walker event's `section_idx` points to a
    //    wasmparser section of the matching kind. Covers bounds,
    //    monotonicity, and correct attribution in one check.
    let expected = wasmparser_root_kinds(&bytes);
    check_root_sections_vs_wasmparser(&r_struct.events, &expected);
});
