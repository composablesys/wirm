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
    println!("filename: {:?}", filename);
    let buff = wat::parse_file(filename).expect("couldn't convert the input wat to Wasm");
    if component {
        let mut parser = Component::parse(&buff, false, false).expect("Unable to parse");
        let result = parser.encode();
        let out = wasmprinter::print_bytes(result.clone()).expect("couldn't translate Wasm to wat");
        let original =
            wasmprinter::print_bytes(&buff).expect("couldn't convert original Wasm to wat");
        assert_eq!(out, original);
    } else {
        let mut parser = Module::parse(&buff, false, false).expect("Unable to parse");
        let result = parser.encode();
        let out = wasmprinter::print_bytes(result.clone()).expect("couldn't translate Wasm to wat");
        let original =
            wasmprinter::print_bytes(&buff).expect("couldn't convert original Wasm to wat");
        assert_eq!(out, original);
    }
    // component.print();
}

fn test_wast(path: String, component: bool) {
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

#[test]
fn test_wast_components() {
    let path = Path::new("./tests/wasm-tools/component-model/");
    // Generate the same output on windows and unix
    let path = path.to_str().unwrap().replace("\\", "/");

    test_wast(path, true);
}

#[test]
fn test_wast_gc() {
    let path = Path::new("./tests/wasm-tools/gc/");
    // Generate the same output on windows and unix
    let path = path.to_str().unwrap().replace("\\", "/");

    test_wast(path, false);
}
