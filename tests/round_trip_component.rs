use wirm::ir::component::Component;

mod common;
use common::{write_to_file, WASM_OUTPUT_DIR};

fn round_trip_component(testname: &str, folder: &str) {
    let filename = format!(
        "{}/tests/test_inputs/{}/{}.wat",
        std::env::var("CARGO_MANIFEST_DIR").unwrap(),
        folder,
        testname
    );
    let buff = wat::parse_file(filename).expect("couldn't convert the input wat to Wasm");
    let mut component = Component::parse(&buff, false, false).expect("Unable to parse");
    // component.print();
    let result = component.encode();
    write_to_file(
        &result,
        format!("{WASM_OUTPUT_DIR}/component_{testname}.wasm"),
    );
    let out = wasmprinter::print_bytes(result.clone()).expect("couldn't translate Wasm to wat");
    let original = wasmprinter::print_bytes(&buff).expect("couldn't convert original Wasm to wat");

    if out != original {
        println!("Test: {:?} failed! Writing to file to check", testname);
        write_to_file(
            original.as_bytes(),
            format!("{}_test_original.wat", testname),
        );
        write_to_file(out.as_bytes(), format!("{}_test.wat", testname));
    }
    assert_eq!(out, original);
}

macro_rules! make_round_trip_tests_component {
    ($folder:literal, $($name:ident),*) => {
        $(
            #[test]
            fn $name() {
                crate::round_trip_component(stringify!($name), $folder)
            }
        )*
    };
}

mod round_trip {
    // note, the dfinity test suite has name conflicts with wasm-tools
    make_round_trip_tests_component!(
        "dfinity/components",
        data_section,
        func,
        func_locals,
        table,
        table_init,
        exports,
        start,
        const_expr
    );

    make_round_trip_tests_component!("handwritten/components", add);

    make_round_trip_tests_component!("wizard/components", func_loop);

    make_round_trip_tests_component!("spec-test/components", if_test);

    make_round_trip_tests_component!("spin", hello_world);
}
