use std::path::{Path, PathBuf};

mod common;
use common::{write_to_file, WAT_OUTPUT_DIR};
use wirm::ir::module::Module;
use wirm::Component;

const TEST_INPUTS: &str = "tests/test_inputs";

fn collect_wat_under(dir: &Path, kind: &str, out: &mut Vec<PathBuf>) {
    for entry in
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
    {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_wat_under(&path, kind, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("wat")
            && path.components().any(|c| c.as_os_str() == kind)
        {
            out.push(path);
        }
    }
}

fn fixtures(kind: &str) -> Vec<PathBuf> {
    let root = format!(
        "{}/{TEST_INPUTS}",
        std::env::var("CARGO_MANIFEST_DIR").unwrap()
    );
    let mut files = vec![];
    collect_wat_under(Path::new(&root), kind, &mut files);
    files.sort();
    assert!(!files.is_empty(), "no {kind} .wat files found under {root}");
    files
}

fn round_trip(path: &Path, reencode: impl Fn(&Path, &[u8]) -> Vec<u8>) {
    let buff = wat::parse_file(path)
        .unwrap_or_else(|e| panic!("couldn't convert {} to Wasm: {e}", path.display()));
    let original = wasmprinter::print_bytes(&buff).expect("couldn't convert original Wasm to wat");
    let result = reencode(path, &buff);
    let out = wasmprinter::print_bytes(&result).expect("couldn't translate Wasm to wat");

    if out != original {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        write_to_file(
            original.as_bytes(),
            format!("{WAT_OUTPUT_DIR}/{name}_original.wat"),
        );
        write_to_file(
            out.as_bytes(),
            format!("{WAT_OUTPUT_DIR}/{name}_reencoded.wat"),
        );
    }
    assert_eq!(out, original, "round-trip mismatch for {}", path.display());
}

#[test]
fn round_trip_components() {
    for path in fixtures("components") {
        println!("round-tripping component {}", path.display());
        round_trip(&path, |p, bytes| {
            Component::parse(bytes, false, false)
                .unwrap_or_else(|e| panic!("wirm couldn't parse {}: {e:?}", p.display()))
                .encode()
                .unwrap_or_else(|e| panic!("wirm couldn't encode {}: {e:?}", p.display()))
        });
    }
}

#[test]
fn round_trip_modules() {
    for path in fixtures("modules") {
        println!("round-tripping module {}", path.display());
        round_trip(&path, |p, bytes| {
            Module::parse(bytes, false, false)
                .unwrap_or_else(|e| panic!("wirm couldn't parse {}: {e:?}", p.display()))
                .encode()
                .unwrap_or_else(|e| panic!("wirm couldn't encode {}: {e:?}", p.display()))
        });
    }
}
