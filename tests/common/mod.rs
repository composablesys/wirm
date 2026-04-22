#![allow(dead_code)]
use log::{error, trace};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::path::Path;
use wasmparser::Operator;

pub mod validate;
pub mod wast_iter;
use wirm::ir::types::InstrumentationMode;
use wirm::iterator::component_iterator::ComponentIterator;
use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator as WirmIterator};
use wirm::iterator::module_iterator::ModuleIterator;
use wirm::opcode::{Inject, Instrumenter, MacroOpcode, Opcode};
use wirm::{Component, Location, Module};

pub const WASM_OUTPUT_DIR: &str = "output/wasm";
pub const WAT_OUTPUT_DIR: &str = "output/wat";

/// create output path if it doesn't exist
pub fn ensure_containing_dir(path: impl AsRef<Path>) {
    if !path.as_ref().exists() {
        fs::create_dir_all(path.as_ref().to_path_buf().parent().unwrap()).unwrap();
    }
}

/// Write bytes to a given path on disk
pub fn write_to_file(bytes: &[u8], path: impl AsRef<Path>) {
    ensure_containing_dir(&path);
    let mut file = match File::create(path) {
        Ok(file) => file,
        Err(e) => {
            error!("Failed to create the file: {}", e);
            return;
        }
    };

    // Write the string to the file
    match file.write_all(bytes) {
        Ok(_) => trace!("Data successfully written to the file."),
        Err(e) => error!("Failed to write to the file: {}", e),
    }
}

// ========================
// ==== TEST FRAMEWORK ====
// ========================

pub fn check_instrumentation_encoding(wirm_wat: &String, file: &str) -> Result<(), std::io::Error> {
    let f = File::open(file)?;
    let mut reader = BufReader::new(f);
    let wat_with_instr = get_wat_with_inline_instrumentation(&mut reader)?;
    assert_eq!(*wirm_wat, wat_with_instr);
    Ok(())
}

const INSERT_PREFIX_PATTERN: &str = ";; << ";
const REPLACE_PREFIX_PATTERN: &str = ";; < ";
const REMOVE_PREFIX_PATTERN: &str = ";; rm";
fn get_wat_with_inline_instrumentation(
    reader: &mut BufReader<File>,
) -> Result<String, std::io::Error> {
    let mut wat_with_instr = String::new();

    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 {
        if line.contains(REMOVE_PREFIX_PATTERN) {
            // Do not include
            line.clear();
            continue;
        } else if line.contains(INSERT_PREFIX_PATTERN) {
            // Just insert the code! This should retain indentation
            let new_line = line.replace(INSERT_PREFIX_PATTERN, "");
            wat_with_instr += &new_line;
        } else if line.contains(REPLACE_PREFIX_PATTERN) {
            // Replace the code! Just remove all non-whitespace before and the pattern itself

            // Find the end of the indentation
            let mut end_whitespace_idx = 0;
            for (idx, c) in line.chars().enumerate() {
                if !c.is_whitespace() {
                    end_whitespace_idx = idx;
                    break;
                }
            }
            // Find the beginning of the command
            let command_start = line.find(REPLACE_PREFIX_PATTERN).unwrap();

            // remove original
            line.replace_range(end_whitespace_idx..command_start, "");
            // remove the command
            let new_line = line.replace(REPLACE_PREFIX_PATTERN, "");
            wat_with_instr += &new_line;
        } else {
            wat_with_instr += &line;
        }

        line.clear();
    }
    Ok(
        wasmprinter::print_bytes(wat::parse_str(wat_with_instr.clone()).expect("Error encoding"))
            .unwrap(),
    )
}

// ==================================
// ==== INSTRUMENTATION HELPERS  ====
// ==================================

/// Enum to match block-forming operators without comparing fields.
#[derive(Debug)]
pub enum SupportedOperators {
    // block-style
    Block,
    Loop,
    If,
    Else,
    // branching
    Br,
    BrIf,
    BrTable,
}

/// Parse a WAT file, run `instrument` with a `ModuleIterator`, encode, and assert the encoding
/// matches the WAT file's inline annotations. Panics on mismatch.
pub fn run_module_instr_test<F>(file: &str, instrument: F)
where
    F: for<'a, 'b> FnOnce(&mut ModuleIterator<'a, 'b>),
{
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    {
        let mut mod_it = ModuleIterator::new(&mut module, &vec![]);
        instrument(&mut mod_it);
    }
    let result = module.encode().expect("error encoding");
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    check_instrumentation_encoding(&out, file).expect("instrumentation encoding mismatch");
}

/// Parse a WAT file, run `instrument` with a `ComponentIterator`, encode, and assert the encoding
/// matches the WAT file's inline annotations. Panics on mismatch.
pub fn run_component_instr_test<F>(file: &str, instrument: F)
where
    F: for<'a, 'b> FnOnce(&mut ComponentIterator<'a, 'b>),
{
    let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    let mut component = Component::parse(&buff, false, false).expect("Unable to parse");
    {
        let mut comp_it = ComponentIterator::new(&mut component, HashMap::new());
        instrument(&mut comp_it);
    }
    let result = component.encode().expect("error encoding");
    let out = wasmprinter::print_bytes(result).expect("couldn't translate wasm to wat");
    check_instrumentation_encoding(&out, file).expect("instrumentation encoding mismatch");
}

/// Sink for `Opcode` injection that stores operators in a `Vec` instead of mutating a module.
///
/// Used by [`opcode_test!`] to replay the same method chain against a recorder and produce the
/// `expected` operator list automatically, eliminating the need for test authors to write each
/// injection twice (once as a method call, once as an `Operator` literal).
pub struct OpRecorder<'a> {
    ops: Vec<Operator<'a>>,
}

impl<'a> OpRecorder<'a> {
    pub fn new() -> Self {
        Self { ops: Vec::new() }
    }

    pub fn finish(self) -> Vec<Operator<'a>> {
        self.ops
    }
}

impl<'a> Default for OpRecorder<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Inject<'a> for OpRecorder<'a> {
    fn inject(&mut self, instr: Operator<'a>) {
        self.ops.push(instr);
    }
}

impl<'a> Opcode<'a> for OpRecorder<'a> {}

impl<'a> MacroOpcode<'a> for OpRecorder<'a> {}

/// Defines a `#[test]` that injects a chain of `Opcode` method calls before the first instruction
/// of function `$target` in `$wat`, then asserts the encoded body begins with exactly those
/// operators. The macro expands the chain twice — once on an [`OpRecorder`] to derive the expected
/// operator list and once on `mod_it.before()` to perform the injection — so the test author
/// writes the chain once.
#[macro_export]
macro_rules! opcode_test {
    ($name:ident, $wat:expr, $target:expr, $($chain:tt)*) => {
        #[test]
        fn $name() {
            #[allow(unused_imports)]
            use ::wirm::Opcode as _;
            #[allow(unused_imports)]
            use ::wirm::opcode::MacroOpcode as _;
            #[allow(unused_imports)]
            use ::wirm::iterator::iterator_trait::IteratingInstrumenter as _;
            let mut rec = $crate::common::OpRecorder::new();
            let _ = (&mut rec) $($chain)*;
            let expected = rec.finish();
            $crate::common::validate_module_instr($wat, $target, &expected, |mod_it| {
                let _ = mod_it.before() $($chain)*;
            });
        }
    };
}

/// Parse an inline WAT string, run `instrument` with a `ModuleIterator`, encode, validate, and
/// assert that the resulting body of local function `target_func_idx` starts with operators whose
/// variant names equal those in `expected` (in order). The name-only comparison sidesteps
/// `MemArg.max_align` round-trip differences and `Operator<'a>` lifetime mismatches.
///
/// Intended for opcode-coverage tests that have no golden annotation file. A test passing means:
/// (a) the injection closure compiles (so the `Opcode` trait method was present and typed right),
/// (b) the encoded bytes validate as Wasm with all features, and
/// (c) the injected operators actually landed at the start of the target function body.
pub fn validate_module_instr<F>(
    wat_src: &str,
    target_func_idx: u32,
    expected: &[Operator<'_>],
    instrument: F,
) where
    F: for<'a, 'b> FnOnce(&mut ModuleIterator<'a, 'b>),
{
    let buff = wat::parse_str(wat_src).expect("couldn't parse WAT");
    let mut module = Module::parse(&buff, false, false).expect("Unable to parse");
    {
        let mut mod_it = ModuleIterator::new(&mut module, &vec![]);
        instrument(&mut mod_it);
    }
    let result = module.encode().expect("error encoding");
    crate::common::validate::validate_bytes(&result).expect("wasm validation failed");
    assert_function_body_prefix(&result, target_func_idx, expected);
}

/// Variant name of an `Operator`, e.g. `"I32Add"` / `"I32Const"`. We compare on variant name so
/// payload fields (like `MemArg.max_align`, which the encoder sets from the instruction's natural
/// alignment) don't derail equality.
fn operator_variant_name(op: &Operator<'_>) -> String {
    let dbg = format!("{:?}", op);
    dbg.split(|c: char| c.is_whitespace() || c == '(' || c == '{')
        .next()
        .unwrap_or(&dbg)
        .to_string()
}

/// Parse `wasm_bytes` and assert the local function at `target_func_idx` begins with operators
/// matching `expected`'s variant names.
fn assert_function_body_prefix(wasm_bytes: &[u8], target_func_idx: u32, expected: &[Operator<'_>]) {
    let mut func_bodies: Vec<Vec<String>> = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm_bytes) {
        let payload = payload.expect("failed to parse emitted wasm");
        if let wasmparser::Payload::CodeSectionEntry(body) = payload {
            let mut ops = Vec::new();
            let mut reader = body.get_operators_reader().expect("operators reader");
            while !reader.eof() {
                let op = reader.read().expect("operator read");
                ops.push(operator_variant_name(&op));
            }
            func_bodies.push(ops);
        }
    }
    let body = func_bodies
        .get(target_func_idx as usize)
        .unwrap_or_else(|| {
            panic!(
                "target function index {} has no code entry (found {} functions)",
                target_func_idx,
                func_bodies.len()
            )
        });
    let expected_names: Vec<String> = expected.iter().map(operator_variant_name).collect();
    assert!(
        body.len() >= expected_names.len(),
        "target function body is shorter than expected injection: body={:?}, expected={:?}",
        body,
        expected_names,
    );
    assert_eq!(
        &body[..expected_names.len()],
        expected_names.as_slice(),
        "injected operator sequence did not appear at the start of function {}",
        target_func_idx,
    );
}

pub fn run_block_injection<'a, 'b, 'c>(
    mod_it: &mut ModuleIterator<'a, 'b>,
    ops_of_interest: &Vec<(SupportedOperators, (InstrumentationMode, Vec<Operator<'c>>))>,
) where
    'c: 'b,
{
    loop {
        let op = mod_it.curr_op();
        if let Location::Module {
            func_idx,
            instr_idx,
        } = mod_it.curr_loc().0
        {
            trace!("Func: {:?}, {}: {:?},", func_idx, instr_idx, op);
            for (op, (mode, body)) in ops_of_interest.iter() {
                let matches = match op {
                    SupportedOperators::Block => {
                        matches!(mod_it.curr_op().unwrap(), Operator::Block { .. })
                    }
                    SupportedOperators::Loop => {
                        matches!(mod_it.curr_op().unwrap(), Operator::Loop { .. })
                    }
                    SupportedOperators::If => {
                        matches!(mod_it.curr_op().unwrap(), Operator::If { .. })
                    }
                    SupportedOperators::Else => {
                        matches!(mod_it.curr_op().unwrap(), Operator::Else)
                    }
                    SupportedOperators::Br => {
                        matches!(mod_it.curr_op().unwrap(), Operator::Br { .. })
                    }
                    SupportedOperators::BrIf => {
                        matches!(mod_it.curr_op().unwrap(), Operator::BrIf { .. })
                    }
                    SupportedOperators::BrTable => {
                        matches!(mod_it.curr_op().unwrap(), Operator::BrTable { .. })
                    }
                };
                if matches {
                    if !body.is_empty() {
                        mod_it.set_instrument_mode(*mode);
                        mod_it.inject_all(body);
                        mod_it.finish_instr();
                    } else {
                        match mode {
                            InstrumentationMode::Alternate => {
                                mod_it.empty_alternate();
                            }
                            InstrumentationMode::BlockAlt => {
                                mod_it.empty_block_alt();
                            }
                            _ => {
                                mod_it.inject_all(body);
                            }
                        }
                    }
                }
            }
            if mod_it.next().is_none() {
                break;
            };
        } else {
            panic!("Should've gotten Module Location!");
        }
    }
}

pub fn inject_function_entry<'a, 'b, 'c>(
    mod_it: &mut ModuleIterator<'a, 'b>,
    body: Vec<Operator<'c>>,
) where
    'c: 'b,
{
    let mut curr_func = None;
    loop {
        if let Location::Module { func_idx, .. } = mod_it.curr_loc().0 {
            if curr_func != Some(func_idx) {
                mod_it.func_entry();
                mod_it.inject_all(&body);
            }
            curr_func = Some(func_idx);
        }
        if mod_it.next().is_none() {
            break;
        };
    }
}

pub fn inject_function_exit<'a, 'b, 'c>(
    mod_it: &mut ModuleIterator<'a, 'b>,
    body: Vec<Operator<'c>>,
) where
    'c: 'b,
{
    let mut curr_func = None;
    loop {
        if let Location::Module { func_idx, .. } = mod_it.curr_loc().0 {
            if curr_func != Some(func_idx) {
                mod_it.func_exit();
                mod_it.inject_all(&body);
            }
            curr_func = Some(func_idx);
        }
        if mod_it.next().is_none() {
            break;
        };
    }
}
