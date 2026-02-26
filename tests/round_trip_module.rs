use log::debug;
use wirm::ir::id::FunctionID;
use wirm::ir::module::Module;

mod common;
use common::{write_to_file, WASM_OUTPUT_DIR, WAT_OUTPUT_DIR};

fn round_trip_module(testname: &str, folder: &str) {
    let filename = format!(
        "{}/tests/test_inputs/{}/{}.wat",
        std::env::var("CARGO_MANIFEST_DIR").unwrap(),
        folder,
        testname
    );
    let buff = wat::parse_file(filename).expect("couldn't convert the input wat to Wasm");
    let original =
        wasmprinter::print_bytes(buff.clone()).expect("couldn't convert original Wasm to wat");
    let module = Module::parse(&buff, false, false).unwrap();
    let result = module.encode().expect("error!");
    let out = wasmprinter::print_bytes(result).expect("couldn't translated Wasm to wat");

    if out != original {
        debug!("Test: {:?} failed! Writing to file to check", testname);
        write_to_file(
            out.as_bytes(),
            format!("{WAT_OUTPUT_DIR}/module_{}.wat", testname),
        );
    }
    assert_eq!(out, original);
}

macro_rules! make_round_trip_tests_module {
    ($folder:literal, $($name:ident),*) => {
        $(
            #[test]
            fn $name() {
                crate::round_trip_module(stringify!($name), $folder)
            }
        )*
    };
}

mod round_trip {
    // TODO -- change this to just search and find files (keeps from having to manually maintain this list)
    make_round_trip_tests_module!(
        "dfinity/modules",
        import_func,
        data_section,
        func,
        func_locals,
        table,
        table_init,
        globals,
        exports,
        start,
        const_expr
    );

    make_round_trip_tests_module!("handwritten/modules", add, block, func1, import, _start);

    // make_round_trip_tests_module!("spin", kotlin_test);
}

#[test]
fn set_name() {
    let filename = "tests/test_inputs/handwritten/modules/func1.wat";
    let buff = wat::parse_file(filename).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false, false).unwrap();
    module.set_fn_name(FunctionID(1), "test".to_string());
    // println!("{:#?}", module);
    let result = module.encode().expect("error!");

    //write result to file
    write_to_file(&result, format!("{WASM_OUTPUT_DIR}/func1.wasm"));
    let out = wasmprinter::print_bytes(result).expect("couldn't translated Wasm to wat");
    debug!("{}", out);
}
