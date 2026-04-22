use crate::ir::component::visitor::driver::VisitEvent;
use crate::ir::component::visitor::events_structural::get_structural_events;
use crate::ir::component::visitor::events_topological::get_topological_events;
use crate::ir::component::visitor::VisitCtx;
use crate::{Component, Module};
use std::fs;
use std::path::Path;
use wasmparser::{ComponentTypeDeclaration, InstanceTypeDeclaration};

const WASM_TOOLS_TEST_COMP_INPUTS: &str = "./tests/wasm-tools/component-model";

#[test]
fn test_equivalent_visit_events_wast_components() {
    let path_str = WASM_TOOLS_TEST_COMP_INPUTS.to_string();
    tests_from_wast(Path::new(&path_str), test_event_generation);
}

#[test]
fn test_equivalent_visit_events_wast_components_async() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}/async");
    tests_from_wast(Path::new(&path_str), test_event_generation);
}

#[test]
fn test_equivalent_visit_events_wast_components_error_context() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}/error-context");
    tests_from_wast(Path::new(&path_str), test_event_generation);
}

#[test]
fn test_equivalent_visit_events_wast_components_gc() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}/gc");
    tests_from_wast(Path::new(&path_str), test_event_generation);
}

#[test]
fn test_equivalent_visit_events_wast_components_shared() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}/shared-everything-threads");
    tests_from_wast(Path::new(&path_str), test_event_generation);
}

#[test]
fn test_equivalent_visit_events_wast_components_values() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}/values");
    tests_from_wast(Path::new(&path_str), test_event_generation);
}

fn get_events<'ir>(
    comp: &'ir Component<'ir>,
    get_evts: fn(&'ir Component<'ir>, &mut VisitCtx<'ir>, &mut Vec<VisitEvent<'ir>>),
) -> Vec<VisitEvent<'ir>> {
    let mut ctx = VisitCtx::new(comp);
    let mut events = Vec::new();
    get_evts(comp, &mut ctx, &mut events);

    events
}

fn check_event_validity(evts0: &Vec<VisitEvent>, evts1: &Vec<VisitEvent>) {
    check_validity_of(evts0);

    // Now we know that the events of evts0 is valid, if they are equal to evts1, then we know
    // that evts1 is valid!
    check_equality(evts0, evts1);
}

/// Events are VALID if:
/// 1. every enter* is paired with an exit*
/// 2. recgroup subtypes only appear between enter_recgroup and exit_recgroup
/// 3. mod type decls only appear between enter/exit core type
/// 4. comp and inst type decls only appear between enter/exit comp type
///   - if the decl contains a comp type, the next event is enter_comp_type
///   - if the decl contains a core type, the next event is enter_core_type
fn check_validity_of(evts: &Vec<VisitEvent>) {
    let mut stack = vec![];
    let mut next_is_enter_comp_type = false;
    let mut next_is_enter_core_type = false;

    for evt in evts.iter() {
        if next_is_enter_comp_type {
            assert!(is_comp_ty_enter(evt),
                "Had a declaration with an inner component type, but the next event was not an enter component type event."
            );
            next_is_enter_comp_type = false;
        }
        if next_is_enter_core_type {
            assert!(is_core_ty_enter(evt),
                    "Had a declaration with an inner core type, but the next event was not an enter core type event."
            );
            next_is_enter_core_type = false;
        }

        if is_enter_evt(evt) {
            stack.push(evt);
        }
        // 1. every enter is paired with an exit
        if is_exit_evt(evt) {
            let enter = stack.last().unwrap();
            assert!(
                enter_exit_match(stack.last().unwrap(), evt),
                "Received mismatched enter/exit events:\n- enter: {enter:?}\n- exit: {evt:?}"
            );
            stack.pop();
        }

        // 2. recgroup subtypes only appear between enter_recgroup and exit_recgroup
        if is_subtype(evt) {
            assert!(
                is_recgroup_enter(stack.last().unwrap()),
                "Received a recgroup subtype event without a recgroup enter event!"
            );
        }

        // 3. mod type decls only appear between enter/exit core type
        if is_mod_decl(evt) {
            assert!(
                is_core_ty_enter(stack.last().unwrap()),
                "Received a module type decl without a core type enter event!"
            );
        }

        // 4. comp and inst type decls only appear between enter/exit comp type
        if is_comp_ty_decl(evt) || is_inst_ty_decl(evt) {
            assert!(
                is_comp_ty_enter(stack.last().unwrap()),
                "Received a component or instance type decl without a comp type enter event!"
            );
            // - if the decl contains a comp type, the next event is enter_comp_type
            if decl_contains_inner_comp_ty(evt) {
                next_is_enter_comp_type = true;
            } else if decl_contains_inner_core_ty(evt) {
                // - if the decl contains a core type, the next event is enter_core_type
                next_is_enter_core_type = true;
            }
        }
    }
}
fn is_enter_evt(evt: &VisitEvent) -> bool {
    matches!(
        evt,
        VisitEvent::EnterRootComp { .. }
            | VisitEvent::EnterComp { .. }
            | VisitEvent::EnterCompType { .. }
            | VisitEvent::EnterCoreType { .. }
            | VisitEvent::EnterCoreRecGroup { .. }
    )
}
fn is_exit_evt(evt: &VisitEvent) -> bool {
    matches!(
        evt,
        VisitEvent::ExitRootComp { .. }
            | VisitEvent::ExitComp { .. }
            | VisitEvent::ExitCompType { .. }
            | VisitEvent::ExitCoreType { .. }
            | VisitEvent::ExitCoreRecGroup { .. }
    )
}
fn is_subtype(evt: &VisitEvent) -> bool {
    matches!(evt, VisitEvent::CoreSubtype { .. })
}
fn is_recgroup_enter(evt: &VisitEvent) -> bool {
    matches!(evt, VisitEvent::EnterCoreRecGroup { .. })
}
fn is_mod_decl(evt: &VisitEvent) -> bool {
    matches!(evt, VisitEvent::ModuleTypeDecl { .. })
}
fn is_core_ty_enter(evt: &VisitEvent) -> bool {
    matches!(evt, VisitEvent::EnterCoreType { .. })
}
fn is_comp_ty_decl(evt: &VisitEvent) -> bool {
    matches!(evt, VisitEvent::CompTypeDecl { .. })
}
fn is_inst_ty_decl(evt: &VisitEvent) -> bool {
    matches!(evt, VisitEvent::InstTypeDecl { .. })
}
fn is_comp_ty_enter(evt: &VisitEvent) -> bool {
    matches!(evt, VisitEvent::EnterCompType { .. })
}
fn decl_contains_inner_comp_ty(evt: &VisitEvent) -> bool {
    match evt {
        VisitEvent::CompTypeDecl { decl, .. } => matches!(decl, ComponentTypeDeclaration::Type(_)),
        VisitEvent::InstTypeDecl { decl, .. } => matches!(decl, InstanceTypeDeclaration::Type(_)),
        _ => false,
    }
}
fn decl_contains_inner_core_ty(evt: &VisitEvent) -> bool {
    match evt {
        VisitEvent::CompTypeDecl { decl, .. } => {
            matches!(decl, ComponentTypeDeclaration::CoreType(_))
        }
        VisitEvent::InstTypeDecl { decl, .. } => {
            matches!(decl, InstanceTypeDeclaration::CoreType(_))
        }
        _ => false,
    }
}

fn enter_exit_match(enter: &VisitEvent, exit: &VisitEvent) -> bool {
    matches!(
        (enter, exit),
        (
            VisitEvent::EnterRootComp { .. },
            VisitEvent::ExitRootComp { .. }
        ) | (VisitEvent::EnterComp { .. }, VisitEvent::ExitComp { .. })
            | (
                VisitEvent::EnterCompType { .. },
                VisitEvent::ExitCompType { .. }
            )
            | (
                VisitEvent::EnterCoreRecGroup { .. },
                VisitEvent::ExitCoreRecGroup { .. }
            )
            | (
                VisitEvent::EnterCoreType { .. },
                VisitEvent::ExitCoreType { .. }
            )
    )
}

fn check_equality(evts0: &Vec<VisitEvent>, evts1: &Vec<VisitEvent>) {
    for (a, b) in evts0.iter().zip(evts1.iter()) {
        match (a, b) {
            (
                VisitEvent::EnterRootComp { component: a_comp },
                VisitEvent::EnterRootComp { component: b_comp },
            ) => {
                assert_eq!(a_comp.id, b_comp.id);
                // check pointing to same memory region
                assert_eq!(*a_comp as *const Component, *b_comp as *const Component);
            }
            (
                VisitEvent::ExitRootComp { component: a_comp },
                VisitEvent::ExitRootComp { component: b_comp },
            ) => {
                assert_eq!(a_comp.id, b_comp.id);
            }
            (
                VisitEvent::EnterComp {
                    section_idx: a_sec,
                    idx: a_idx,
                    component: a_comp,
                },
                VisitEvent::EnterComp {
                    section_idx: b_sec,
                    idx: b_idx,
                    component: b_comp,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_idx, b_idx);
                assert_eq!(a_comp.id, b_comp.id);
            }
            (
                VisitEvent::ExitComp {
                    section_idx: a_sec,
                    idx: a_idx,
                    component: a_comp,
                },
                VisitEvent::ExitComp {
                    section_idx: b_sec,
                    idx: b_idx,
                    component: b_comp,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_idx, b_idx);
                assert_eq!(a_comp.id, b_comp.id);
            }
            (
                VisitEvent::Module {
                    section_idx: a_sec,
                    idx: a_idx,
                    module: a_mod,
                },
                VisitEvent::Module {
                    section_idx: b_sec,
                    idx: b_idx,
                    module: b_mod,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_idx, b_idx);
                // check pointing to same memory region
                assert_eq!(*a_mod as *const Module, *b_mod as *const Module);
            }
            (
                VisitEvent::EnterCompType {
                    section_idx: a_sec,
                    idx: a_idx,
                    ty: a_ty,
                },
                VisitEvent::EnterCompType {
                    section_idx: b_sec,
                    idx: b_idx,
                    ty: b_ty,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_idx, b_idx);
                assert_eq!(a_ty, b_ty);
            }
            (
                VisitEvent::ExitCompType {
                    section_idx: a_sec,
                    idx: a_idx,
                    ty: a_ty,
                },
                VisitEvent::ExitCompType {
                    section_idx: b_sec,
                    idx: b_idx,
                    ty: b_ty,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_idx, b_idx);
                assert_eq!(a_ty, b_ty);
            }
            (
                VisitEvent::CompTypeDecl {
                    section_idx: a_sec,
                    parent: a_parent,
                    idx: a_idx,
                    decl: a_decl,
                },
                VisitEvent::CompTypeDecl {
                    section_idx: b_sec,
                    parent: b_parent,
                    idx: b_idx,
                    decl: b_decl,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_parent, b_parent);
                assert_eq!(a_idx, b_idx);
                assert_eq!(a_decl, b_decl);
            }
            (
                VisitEvent::InstTypeDecl {
                    section_idx: a_sec,
                    parent: a_parent,
                    idx: a_idx,
                    decl: a_decl,
                },
                VisitEvent::InstTypeDecl {
                    section_idx: b_sec,
                    parent: b_parent,
                    idx: b_idx,
                    decl: b_decl,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_parent, b_parent);
                assert_eq!(a_idx, b_idx);
                assert_eq!(a_decl, b_decl);
            }
            (
                VisitEvent::CompInst {
                    section_idx: a_sec,
                    idx: a_idx,
                    inst: a_inst,
                },
                VisitEvent::CompInst {
                    section_idx: b_sec,
                    idx: b_idx,
                    inst: b_inst,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_idx, b_idx);
                assert_eq!(a_inst, b_inst);
            }
            (
                VisitEvent::Canon {
                    section_idx: a_sec,
                    kind: a_kind,
                    idx: a_idx,
                    canon: a_canon,
                },
                VisitEvent::Canon {
                    section_idx: b_sec,
                    kind: b_kind,
                    idx: b_idx,
                    canon: b_canon,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_kind, b_kind);
                assert_eq!(a_idx, b_idx);
                assert_eq!(a_canon, b_canon);
            }
            (
                VisitEvent::Alias {
                    section_idx: a_sec,
                    kind: a_kind,
                    idx: a_idx,
                    alias: a_alias,
                },
                VisitEvent::Alias {
                    section_idx: b_sec,
                    kind: b_kind,
                    idx: b_idx,
                    alias: b_alias,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_kind, b_kind);
                assert_eq!(a_idx, b_idx);
                assert_eq!(a_alias, b_alias);
            }
            (
                VisitEvent::Import {
                    section_idx: a_sec,
                    kind: a_kind,
                    idx: a_idx,
                    imp: a_imp,
                },
                VisitEvent::Import {
                    section_idx: b_sec,
                    kind: b_kind,
                    idx: b_idx,
                    imp: b_imp,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_kind, b_kind);
                assert_eq!(a_idx, b_idx);
                assert_eq!(a_imp, b_imp);
            }
            (
                VisitEvent::Export {
                    section_idx: a_sec,
                    kind: a_kind,
                    idx: a_idx,
                    exp: a_exp,
                },
                VisitEvent::Export {
                    section_idx: b_sec,
                    kind: b_kind,
                    idx: b_idx,
                    exp: b_exp,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_kind, b_kind);
                assert_eq!(a_idx, b_idx);
                assert_eq!(a_exp, b_exp);
            }
            (
                VisitEvent::EnterCoreRecGroup {
                    section_idx: a_sec,
                    ty: a_ty,
                    count: a_count,
                },
                VisitEvent::EnterCoreRecGroup {
                    section_idx: b_sec,
                    ty: b_ty,
                    count: b_count,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_ty, b_ty);
                assert_eq!(a_count, b_count);
            }
            (
                VisitEvent::CoreSubtype {
                    section_idx: a_sec,
                    parent_idx: a_pidx,
                    subvec_idx: a_sidx,
                    subtype: a_ty,
                },
                VisitEvent::CoreSubtype {
                    section_idx: b_sec,
                    parent_idx: b_pidx,
                    subvec_idx: b_sidx,
                    subtype: b_ty,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_pidx, b_pidx);
                assert_eq!(a_sidx, b_sidx);
                assert_eq!(a_ty, b_ty);
            }
            (
                VisitEvent::ExitCoreRecGroup { section_idx: a_sec },
                VisitEvent::ExitCoreRecGroup { section_idx: b_sec },
            ) => {
                assert_eq!(a_sec, b_sec);
            }
            (
                VisitEvent::EnterCoreType {
                    section_idx: a_sec,
                    idx: a_idx,
                    ty: a_ty,
                },
                VisitEvent::EnterCoreType {
                    section_idx: b_sec,
                    idx: b_idx,
                    ty: b_ty,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_idx, b_idx);
                assert_eq!(a_ty, b_ty);
            }
            (
                VisitEvent::ModuleTypeDecl {
                    section_idx: a_sec,
                    parent: a_parent,
                    idx: a_idx,
                    decl: a_decl,
                },
                VisitEvent::ModuleTypeDecl {
                    section_idx: b_sec,
                    parent: b_parent,
                    idx: b_idx,
                    decl: b_decl,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_parent, b_parent);
                assert_eq!(a_idx, b_idx);
                assert_eq!(a_decl, b_decl);
            }
            (
                VisitEvent::ExitCoreType {
                    section_idx: a_sec,
                    idx: a_idx,
                    ty: a_ty,
                },
                VisitEvent::ExitCoreType {
                    section_idx: b_sec,
                    idx: b_idx,
                    ty: b_ty,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_idx, b_idx);
                assert_eq!(a_ty, b_ty);
            }
            (
                VisitEvent::CoreInst {
                    section_idx: a_sec,
                    idx: a_idx,
                    inst: a_inst,
                },
                VisitEvent::CoreInst {
                    section_idx: b_sec,
                    idx: b_idx,
                    inst: b_inst,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_idx, b_idx);
                assert_eq!(a_inst, b_inst);
            }
            (
                VisitEvent::CustomSection {
                    section_idx: a_sec,
                    sect: a_sect,
                },
                VisitEvent::CustomSection {
                    section_idx: b_sec,
                    sect: b_sect,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                // best effort check here
                assert_eq!(a_sect.name, b_sect.name);
            }
            (
                VisitEvent::StartFunc {
                    section_idx: a_sec,
                    func: a_func,
                },
                VisitEvent::StartFunc {
                    section_idx: b_sec,
                    func: b_func,
                },
            ) => {
                assert_eq!(a_sec, b_sec);
                assert_eq!(a_func.func_index, b_func.func_index);
                assert_eq!(a_func.arguments, b_func.arguments);
                assert_eq!(a_func.results, b_func.results);
            }
            _ => panic!("events are not the same discriminant: {a:?} != {b:?}"),
        }
    }
}

fn test_event_generation(label: &str, bytes: &[u8]) {
    println!("\n{label}");
    let original = wasmprinter::print_bytes(bytes).expect("couldn't convert original Wasm to wat");
    println!("original: {:?}", original);

    let comp = Component::parse(bytes, false, false).expect("Unable to parse");
    let evts_struct = get_events(&comp, get_structural_events);
    let evts_topo = get_events(&comp, get_topological_events);
    check_event_validity(&evts_struct, &evts_topo);
}

pub fn tests_from_wast(path: &Path, run_test: fn(&str, &[u8])) {
    for_each_wast_in_dir(path, |wast_path| {
        for_each_valid_wasm_in_wast(wast_path, &run_test);
    });
}

/// Iterates `*.wast` files in `dir` (non-recursive), invoking `f` with each.
fn for_each_wast_in_dir(dir: &Path, mut f: impl FnMut(&Path)) {
    let dir = dir.to_str().unwrap().replace("\\", "/");
    for entry in fs::read_dir(&dir).unwrap() {
        let file = entry.unwrap();
        if file.path().extension().and_then(|e| e.to_str()) == Some("wast") {
            f(&file.path());
        }
    }
}

// Shared with the integration test in tests/round_trip_wast.rs. Kept in
// tests/common/ so all test-only code lives under tests/; reached from
// here via #[path] because lib tests can't normally see tests/.
#[path = "../../../../tests/common/wast_iter.rs"]
mod wast_iter;
use wast_iter::for_each_valid_wasm_in_wast;
