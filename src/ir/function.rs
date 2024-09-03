//! Function Builder

use crate::ir::id::{FunctionID, ImportsID, LocalID, ModuleID};
use crate::ir::module::module_functions::{add_local, LocalFunction};
use crate::ir::module::{Module, ReIndexable};
use crate::ir::types::DataType;
use crate::ir::types::InstrumentationMode;
use crate::ir::types::{Body, FuncInstrFlag, FuncInstrMode};
use crate::module_builder::AddLocal;
use crate::opcode::{Inject, InjectAt, Instrumenter, MacroOpcode, Opcode};
use crate::{Component, Location};
use wasmparser::{Operator, TypeRef};

// TODO: probably need better reasoning with lifetime here
/// Build a function from scratch
/// See an example [here].
///
/// [here]: https://github.com/thesuhas/orca/blob/314af2df01203e7715aa728e7388cf39c564e9d7/fac_orca/src/main.rs#L16
pub struct FunctionBuilder<'a> {
    // pub(crate) id: u32, // function index
    pub(crate) params: Vec<DataType>,
    pub(crate) results: Vec<DataType>,
    #[allow(dead_code)]
    pub(crate) name: Option<String>,
    pub body: Body<'a>,
}

impl<'a> FunctionBuilder<'a> {
    pub fn new(params: &[DataType], results: &[DataType]) -> Self {
        Self {
            params: params.to_vec(),
            results: results.to_vec(),
            name: None,
            body: Body::default(),
        }
    }

    /// Finish building a function (have side effect on module IR),
    /// return function index
    pub fn finish_module(mut self, module: &mut Module<'a>) -> FunctionID {
        // add End as last instruction
        self.end();
        let id = module.add_local_func(self.name, &self.params, &self.results, self.body.clone());

        assert_eq!(
            module.functions.len() as u32,
            module.num_local_functions + module.imports.num_funcs
        );

        id
    }

    pub fn replace_import_in_module(mut self, module: &mut Module<'a>, import_id: ImportsID) {
        // add End as last instruction
        self.end();

        let err_msg = "Could not replace the specified import with this function,";
        if let TypeRef::Func(imp_ty_id) = module.imports.get(import_id).ty {
            if let Some(ty) = module.types.get(imp_ty_id) {
                if *ty.params == self.params && *ty.results == self.results {
                    let local_func = LocalFunction::new(
                        imp_ty_id,
                        import_id,
                        self.body.clone(),
                        self.params.len(),
                    );
                    module.convert_import_fn_to_local(import_id, local_func);
                } else {
                    panic!("{err_msg} types are not equivalent.")
                }
            } else {
                panic!("{err_msg} could not find an associated type for the specified import ID: {import_id}.")
            }
        } else {
            panic!("{err_msg} the specified import ID does not point to a function!")
        }
    }

    /// Finish building a function (have side effect on component IR),
    /// return function index
    pub fn finish_component(mut self, comp: &mut Component<'a>, mod_idx: ModuleID) -> FunctionID {
        // add End as last instruction
        self.end();

        let id = comp.modules[mod_idx as usize].add_local_func(
            self.name,
            &self.params,
            &self.results,
            self.body.clone(),
        );

        assert_eq!(
            comp.modules[mod_idx as usize].functions.len() as u32,
            comp.modules[mod_idx as usize].num_local_functions
                + comp.modules[mod_idx as usize].imports.num_funcs
                + comp.modules[mod_idx as usize].imports.num_funcs_added
        );
        id
    }

    pub fn set_name(&mut self, name: String) {
        self.name = Some(name)
    }
}

impl<'a> Inject<'a> for FunctionBuilder<'a> {
    /// Inject an operator at the end of the function
    // here the location of the injection is always at the end of the function
    fn inject(&mut self, op: Operator<'a>) {
        self.body.push_op(op)
    }
}
impl<'a> Opcode<'a> for FunctionBuilder<'a> {}
impl<'a> MacroOpcode<'a> for FunctionBuilder<'a> {}

impl AddLocal for FunctionBuilder<'_> {
    /// add a local and return local index
    /// (note that local indices start after)
    fn add_local(&mut self, ty: DataType) -> LocalID {
        add_local(
            ty,
            self.params.len(),
            &mut self.body.num_locals,
            &mut self.body.locals,
        )
    }
}

/// Modify a function
/// Uses same injection logic as Iterator, which is different from
/// FunctionBuilder since FunctionModifier does side effect to operators at encoding
/// (it only modifies the Instrument type)
pub struct FunctionModifier<'a, 'b> {
    pub instr_flag: FuncInstrFlag<'a>,
    pub body: &'a mut Body<'b>,
    pub args: &'a mut Vec<LocalID>,
    pub(crate) instr_idx: Option<usize>,
}

impl<'a, 'b> FunctionModifier<'a, 'b> {
    // by default, the instr_idx the last instruction (always Operator::End indicating end of the function)
    // and the Instrument type is set to before
    pub fn init(body: &'a mut Body<'b>, args: &'a mut Vec<LocalID>) -> Self {
        let instr_idx = body.instructions.len() - 1;
        let mut func_modifier = FunctionModifier {
            instr_flag: FuncInstrFlag::default(),
            body,
            args,
            instr_idx: None,
        };
        func_modifier.before_at(Location::Module {
            func_idx: 0, // not used
            instr_idx,
        });
        func_modifier
    }

    /// add a local and return local index
    pub fn add_local(&mut self, ty: DataType) -> LocalID {
        add_local(
            ty,
            self.args.len(),
            &mut self.body.num_locals,
            &mut self.body.locals,
        )
    }
}

impl<'a, 'b> Inject<'b> for FunctionModifier<'a, 'b> {
    // TODO: refactor the inject the function to return a Result rather than panicking?
    fn inject(&mut self, instr: Operator<'b>) {
        if self.instr_flag.current_mode.is_some() {
            // inject at the function level
            self.instr_flag.add_instr(instr);
        } else {
            // inject at instruction level
            if let Some(idx) = self.instr_idx {
                let is_special = self.body.instructions[idx].add_instr(instr);
                // remember if we injected a special instrumentation (to be resolved before encoding)
                self.instr_flag.has_special_instr |= is_special;
            } else {
                panic!("Instruction index not set");
            }
        }
    }
}
impl<'a, 'b> InjectAt<'b> for FunctionModifier<'a, 'b> {
    fn inject_at(&mut self, idx: usize, mode: InstrumentationMode, instr: Operator<'b>) {
        let loc = Location::Module {
            func_idx: 0, // not used
            instr_idx: idx,
        };
        self.set_instrument_mode_at(mode, loc);
        self.add_instr_at(loc, instr);
    }
}
impl<'a, 'b> Opcode<'b> for FunctionModifier<'a, 'b> {}
impl<'a, 'b> MacroOpcode<'b> for FunctionModifier<'a, 'b> {}

impl<'a, 'b> Instrumenter<'b> for FunctionModifier<'a, 'b> {
    fn curr_instrument_mode(&self) -> &Option<InstrumentationMode> {
        if let Some(idx) = self.instr_idx {
            &self.body.instructions[idx].instr_flag.current_mode
        } else {
            panic!("Instruction index not set");
        }
    }

    fn set_instrument_mode_at(&mut self, mode: InstrumentationMode, loc: Location) {
        if let Location::Module { instr_idx, .. } = loc {
            self.instr_idx = Some(instr_idx);
            self.body.instructions[instr_idx].instr_flag.current_mode = Some(mode);
        } else {
            panic!("Should have gotten module location");
        }
    }

    fn curr_func_instrument_mode(&self) -> &Option<FuncInstrMode> {
        &self.instr_flag.current_mode
    }

    fn set_func_instrument_mode(&mut self, mode: FuncInstrMode) {
        self.instr_flag.current_mode = Some(mode);
    }

    fn clear_instr_at(&mut self, loc: Location, mode: InstrumentationMode) {
        if let Location::Module { instr_idx, .. } = loc {
            self.body.clear_instr(instr_idx, mode);
        } else {
            panic!("Should have gotten module location");
        }
    }

    fn add_instr_at(&mut self, loc: Location, instr: Operator<'b>) {
        if let Location::Module { instr_idx, .. } = loc {
            self.body.instructions[instr_idx].add_instr(instr);
        } else {
            panic!("Should have gotten module location");
        }
    }

    fn empty_alternate_at(&mut self, loc: Location) -> &mut Self {
        if let Location::Module { instr_idx, .. } = loc {
            self.body.instructions[instr_idx].instr_flag.alternate = Some(vec![]);
        } else {
            panic!("Should have gotten Component Location and not Module Location!")
        }

        self
    }

    fn empty_block_alt_at(&mut self, loc: Location) -> &mut Self {
        if let Location::Module { instr_idx, .. } = loc {
            self.body.instructions[instr_idx].instr_flag.block_alt = Some(vec![]);
            self.instr_flag.has_special_instr |= true;
        } else {
            panic!("Should have gotten Component Location and not Module Location!")
        }

        self
    }

    fn get_injected_val(&self, idx: usize) -> &Operator {
        self.body.instructions[idx].instr_flag.get_instr(idx)
    }
}
