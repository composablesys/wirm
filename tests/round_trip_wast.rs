use std::fs;
use std::path::Path;
use wirm::Component;
use wirm::Module;

mod common;
use common::wast_iter::for_each_valid_wasm_in_wast;

fn roundtrip(label: &str, bytes: &[u8], component: bool) {
    println!("\n{label}");
    let original = wasmprinter::print_bytes(bytes).expect("couldn't convert original Wasm to wat");
    println!("original: {:?}", original);
    if component {
        let parser = Component::parse(bytes, false, false).expect("Unable to parse");
        let result = parser.encode().expect("error");
        let out = wasmprinter::print_bytes(result.clone()).expect("couldn't translate Wasm to wat");
        assert_eq!(out, original);
    } else {
        let parser = Module::parse(bytes, false, false).expect("Unable to parse");
        let result = parser.encode().expect("error during parse");
        let out = wasmprinter::print_bytes(result.clone()).expect("couldn't translate Wasm to wat");
        assert_eq!(out, original);
    }
}

fn test_wast(path: &Path, component: bool) {
    let path = path.to_str().unwrap().replace("\\", "/");
    for entry in fs::read_dir(&path).unwrap() {
        let file = entry.unwrap();
        if file.path().extension().and_then(|e| e.to_str()) != Some("wast") {
            continue;
        }
        for_each_valid_wasm_in_wast(&file.path(), |label, bytes| {
            roundtrip(label, bytes, component)
        });
    }
}

const WASM_TOOLS_TEST_COMP_INPUTS: &str = "./tests/wasm-tools/component-model";

#[test]
fn test_wast_components() {
    let path_str = WASM_TOOLS_TEST_COMP_INPUTS.to_string();
    test_wast(Path::new(&path_str), true);
}

#[test]
fn test_wast_components_async() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}/async");
    test_wast(Path::new(&path_str), true);
}

#[test]
fn test_wast_components_error_context() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}/error-context");
    test_wast(Path::new(&path_str), true);
}

#[test]
fn test_wast_components_gc() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}/gc");
    test_wast(Path::new(&path_str), true);
}

#[test]
fn test_wast_components_shared_everything_threads() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}/shared-everything-threads");
    test_wast(Path::new(&path_str), true);
}

#[test]
fn test_wast_components_values() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}/values");
    test_wast(Path::new(&path_str), true);
}

#[test]
fn test_wast_gc() {
    test_wast(Path::new("./tests/wasm-tools/gc/"), false);
}
