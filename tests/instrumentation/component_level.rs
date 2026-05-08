use crate::instrumentation::test_module::{try_path, validate_wasm};
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentImportName,
    ComponentType, ComponentTypeRef, Export, ExternalKind, Instance, InstanceTypeDeclaration,
    InstantiationArg, InstantiationArgKind,
};
use wirm::Component;

pub const WHAMM_CORE_LIB_NAME: &str = "whamm_core";
const TEST_DEBUG_DIR: &str = "output/tests/debug_me/instrumentation/";
#[test]
fn whamm_side_effects() {
    let file = "tests/test_inputs/spin/hello_world.wat";
    let output_wasm_path = format!("{TEST_DEBUG_DIR}/whamm_side_effects.wasm");
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut component = Component::parse(&buff, false, false).expect("Unable to parse");

    let lib_path = "tests/test_inputs/whamm/whamm_core.wasm";
    let lib_buff = wat::parse_file(lib_path).expect("couldn't convert the input wat to Wasm");

    configure_component_libraries(0, &mut component, lib_buff.as_slice());

    try_path(&output_wasm_path);
    if let Err(e) = component.emit_wasm(&output_wasm_path) {
        panic!(
            "Failed to dump wasm to {output_wasm_path} due to error: {}",
            e
        );
    }
    validate_wasm(&output_wasm_path);
}

pub fn configure_component_libraries<'a>(
    target_module_id: u32,
    component: &mut Component<'a>,
    core_lib: &'a [u8],
) {
    // find "wasi_snapshot_preview1" instance
    let mut wasi_instance = None;
    let wasi_exports = ["fd_write", "environ_get", "environ_sizes_get", "proc_exit"];
    for (i, inst) in component.instances.iter().enumerate() {
        if let Instance::FromExports(exports) = inst {
            let mut found_count = 0;
            for export in exports.iter() {
                if wasi_exports.contains(&export.name) {
                    found_count += 1;
                }
            }

            if found_count == wasi_exports.len() {
                wasi_instance = Some(i);
                break;
            }
        }
    }
    if wasi_instance.is_some() {
        configure_lib(target_module_id, component, WHAMM_CORE_LIB_NAME, core_lib);
    } else {
        panic!(
            "Target component does not already import wasi_snapshot_preview1, not supported yet."
        )
    }

    fn configure_lib<'a>(
        target_module_id: u32,
        wasm: &mut Component<'a>,
        lib_name: &'a str,
        lib_bytes: &'a [u8],
    ) {
        let wasi_name = "wasi_snapshot_preview1";
        let lib_wasm = Component::parse(lib_bytes, false, true).unwrap();

        // Create an instance type that defines the library
        let mut decls = vec![];
        // let mut num_exported_fns = 0;
        let mut curr_ty_id = 0;
        for export in lib_wasm.exports.iter() {
            let comp_ty = lib_wasm.get_type_of_exported_lift_func(export);
            if let Some(ty) = comp_ty.as_ref() {
                if matches!(ty, ComponentType::Func(_)) {
                    decls.push(InstanceTypeDeclaration::Type((*ty).clone()));
                    decls.push(InstanceTypeDeclaration::Export {
                        name: export.name,
                        ty: ComponentTypeRef::Func(curr_ty_id),
                    });
                    curr_ty_id += 1;
                }
            }
        }
        let inst_ty_id = wasm.add_component_type_instance(decls);

        // Import the library from an external provider
        let inst_id = wasm.add_import(ComponentImport {
            name: ComponentImportName("whamm-core"),
            ty: ComponentTypeRef::Instance(*inst_ty_id),
        });

        // Lower the exported functions using aliases
        let mut exports = vec![];
        for ComponentExport { name, kind, .. } in lib_wasm.exports.iter() {
            let func_id = wasm.add_alias_func(ComponentAlias::InstanceExport {
                name: name.0,
                kind: *kind,
                instance_index: inst_id,
            });
            let canon_id = wasm.add_canon_func(CanonicalFunction::Lower {
                func_index: *func_id,
                options: vec![].into_boxed_slice(),
            });

            exports.push(Export {
                name: name.0,
                kind: ExternalKind::Func,
                index: *canon_id,
            });
        }

        // Create a core instance from the library
        let lib_inst_id = wasm.add_core_instance(Instance::FromExports(exports.into_boxed_slice()));

        // Edit the instantiation of the instrumented module to include the added library
        for inst in wasm.instances.iter_mut() {
            if let Instance::Instantiate { module_index, args } = inst {
                if target_module_id == *module_index {
                    let mut uses_wasi = false;
                    let mut new_args = vec![];
                    for arg in args.iter() {
                        if arg.name == wasi_name {
                            uses_wasi = true;
                        }
                        new_args.push(arg.clone());
                    }
                    assert!(uses_wasi, "Target module does not already import wasi_snapshot_preview1, not supported yet.");

                    new_args.push(InstantiationArg {
                        name: lib_name,
                        kind: InstantiationArgKind::Instance,
                        index: *lib_inst_id,
                    });

                    *args = new_args.into_boxed_slice();
                }
            }
        }
    }
}
