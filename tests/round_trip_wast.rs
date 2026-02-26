use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;
use wirm::Component;
use wirm::Module;

fn wasm_tools() -> Command {
    Command::new("wasm-tools")
}

fn roundtrip(filename: String, component: bool) {
    println!("\nfilename: {:?}", filename);
    let buff = wat::parse_file(filename).expect("couldn't convert the input wat to Wasm");
    let original = wasmprinter::print_bytes(&buff).expect("couldn't convert original Wasm to wat");
    println!("original: {:?}", original);
    if component {
        let parser = Component::parse(&buff, false, false).expect("Unable to parse");
        let result = parser.encode().expect("error");
        let out = wasmprinter::print_bytes(result.clone()).expect("couldn't translate Wasm to wat");
        assert_eq!(out, original);
    } else {
        let parser = Module::parse(&buff, false, false).expect("Unable to parse");
        let result = parser.encode().expect("error during parse");
        let out = wasmprinter::print_bytes(result.clone()).expect("couldn't translate Wasm to wat");
        assert_eq!(out, original);
    }
}

fn test_wast(path: &Path, component: bool) {
    let path = path.to_str().unwrap().replace("\\", "/");
    for entry in fs::read_dir(path).unwrap() {
        let file = entry.unwrap();
        match file.path().extension() {
            None => continue,
            Some(ext) => {
                if ext.to_str() != Some("wast") {
                    continue;
                }
            }
        }
        let mut cmd = wasm_tools();
        let td = tempfile::TempDir::new().unwrap();
        cmd.arg("json-from-wast")
            .arg(file.path())
            .arg("--pretty")
            .arg("--wasm-dir")
            .arg(td.path())
            .arg("-o")
            .arg(td.path().join(format!(
                "{:?}.json",
                Path::new(&file.path())
                    .file_stem()
                    .unwrap()
                    .to_str()
                    .unwrap()
            )));
        let output = cmd.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!("failed to run {cmd:?}\nstdout: {stdout}\nstderr: {stderr}");
        }
        // For every file that is not invalid in the output, do round-trip
        for entry in fs::read_dir(td.path()).unwrap() {
            let file_json = entry.unwrap();
            match file_json.path().extension() {
                None => continue,
                Some(ext) => {
                    if ext.to_str() != Some("json") {
                        continue;
                    }
                }
            }
            let json: Value = serde_json::from_str(
                &fs::read_to_string(file_json.path()).expect("Unable to open file"),
            )
            .unwrap();
            if let Value::Object(map) = json {
                if let Value::Array(vals) = map.get_key_value("commands").unwrap().1 {
                    for value in vals {
                        if let Value::Object(testcase) = value {
                            // If assert is not in the string, that means it is a valid test case
                            if let Value::String(ty) = testcase.get_key_value("type").unwrap().1 {
                                if !ty.contains("assert") && testcase.contains_key("filename") {
                                    if let Value::String(test_file) =
                                        testcase.get_key_value("filename").unwrap().1
                                    {
                                        // Do round-trip
                                        roundtrip(
                                            Path::new(td.path())
                                                .join(test_file)
                                                .to_str()
                                                .unwrap()
                                                .parse()
                                                .unwrap(),
                                            component,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
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
