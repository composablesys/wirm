//! Iterator to traverse a Module

use crate::ir::id::{FunctionID, GlobalID, LocalID};
use crate::ir::module::module_functions::FuncKind;
use crate::ir::module::module_globals::Global;
use crate::ir::module::Module;
use crate::ir::types::{DataType, FuncInstrMode, InjectedInstrs, InstrumentationMode, Location};
use crate::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
use crate::module_builder::AddLocal;
use crate::opcode::{Inject, InjectAt, Instrumenter, MacroOpcode, Opcode};
use crate::subiterator::module_subiterator::ModuleSubIterator;
use wasmparser::Operator;

/// Iterator for a Module.
// 'b should outlive 'a
pub struct ModuleIterator<'a, 'b> {
    /// The Module to Iterate
    pub module: &'a mut Module<'b>,
    /// The SubIterator for this Module
    mod_iterator: ModuleSubIterator,
}

#[allow(dead_code)]
impl<'a, 'b> ModuleIterator<'a, 'b> {
    /// Creates a new ModuleIterator
    pub fn new(module: &'a mut Module<'b>, skip_funcs: &Vec<FunctionID>) -> Self {
        let metadata = module.get_func_metadata();
        ModuleIterator {
            module,
            mod_iterator: ModuleSubIterator::new(metadata, skip_funcs.to_owned()),
        }
    }

    pub fn curr_op_owned(&self) -> Option<Operator<'b>> {
        if let (
            Location::Module {
                func_idx,
                instr_idx,
                ..
            },
            ..,
        ) = self.mod_iterator.curr_loc()
        {
            match &self.module.functions.get(func_idx).kind {
                FuncKind::Import(_) => panic!("Cannot get an instruction to an imported function"),
                FuncKind::Local(l) => Some(l.body.instructions[instr_idx].op.clone()),
            }
        } else {
            panic!("Should have gotten Module Location!")
        }
    }
}

impl<'b> Inject<'b> for ModuleIterator<'_, 'b> {
    /// Injects an Operator at the current location
    ///
    /// # Example
    /// ```no_run
    /// use wirm::ir::module::Module;
    /// use wirm::iterator::module_iterator::ModuleIterator;
    /// use wasmparser::Operator;
    /// use wirm::ir::types::{Location};
    /// use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
    /// use wirm::opcode::{Instrumenter, Opcode};
    ///
    /// let file = "path_to_file";
    /// let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    /// // Must use `parse_only_module` here as we are only concerned about a Module and not a module that is inside a Component
    /// let mut module = Module::parse(&buff, false).expect("Unable to parse");
    /// let mut module_it = ModuleIterator::new(&mut module, &vec![]);
    ///
    /// // Everytime there is a `call 1` instruction we want to inject an `i32.const 0`
    /// let interested = Operator::Call { function_index: 1 };
    ///
    /// loop {
    ///     let op = module_it.curr_op();
    ///     let instr_mode = module_it.curr_instrument_mode();
    ///
    ///     if let Location::Module {
    ///         func_idx,
    ///         instr_idx,
    ///     } = module_it.curr_loc().0
    ///     {
    ///         if *module_it.curr_op().unwrap() == interested {
    ///             module_it.before().i32_const(1);
    ///         }
    ///         if module_it.next().is_none() {
    ///             break;
    ///         };
    ///     } else {
    ///         // Ensures we only get the location of a module while parsing a component
    ///         panic!("Should've gotten Module Location!");
    ///     }
    /// }
    /// ```
    fn inject(&mut self, instr: Operator<'b>) {
        if let (
            Location::Module {
                func_idx,
                instr_idx,
                ..
            },
            ..,
        ) = self.curr_loc()
        {
            match self.module.functions.get_mut(func_idx as FunctionID).kind {
                FuncKind::Import(_) => panic!("Cannot get an instruction to an imported function"),
                FuncKind::Local(ref mut l) => l.add_instr(instr, instr_idx),
            }
        } else {
            panic!("Should have gotten Module Location!")
        }
    }
}
impl<'a> InjectAt<'a> for ModuleIterator<'_, 'a> {
    fn inject_at(&mut self, idx: usize, mode: InstrumentationMode, instr: Operator<'a>) {
        if let (Location::Module { func_idx, .. }, ..) = self.curr_loc() {
            let loc = Location::Module {
                func_idx,
                instr_idx: idx,
            };
            self.set_instrument_mode_at(mode, loc);
            self.add_instr_at(loc, instr);
        } else {
            panic!("Should have gotten Module Location!")
        }
    }
}
impl<'a> Opcode<'a> for ModuleIterator<'_, 'a> {}
impl<'a> MacroOpcode<'a> for ModuleIterator<'_, 'a> {}
impl<'a> Instrumenter<'a> for ModuleIterator<'_, 'a> {
    ///Can be called after finishing some instrumentation to reset the mode.
    fn finish_instr(&mut self) {
        if let (
            Location::Module {
                func_idx,
                instr_idx,
                ..
            },
            ..,
        ) = self.mod_iterator.curr_loc()
        {
            match &mut self.module.functions.get_mut(func_idx as FunctionID).kind {
                FuncKind::Import(_) => panic!("Cannot get an instruction to an imported function"),
                FuncKind::Local(l) => {
                    l.instr_flag.finish_instr();
                    l.body.instructions[instr_idx].instr_flag.finish_instr();
                }
            }
        } else {
            panic!("Should have gotten Module Location and not Module Location!")
        }
    }
    /// Returns the Instrumentation at the current Location
    fn curr_instrument_mode(&self) -> &Option<InstrumentationMode> {
        if let (
            Location::Module {
                func_idx,
                instr_idx,
                ..
            },
            ..,
        ) = self.mod_iterator.curr_loc()
        {
            match &self.module.functions.get(func_idx as FunctionID).kind {
                FuncKind::Import(_) => panic!("Cannot get an instruction to an imported function"),
                FuncKind::Local(l) => &l.body.instructions[instr_idx].instr_flag.current_mode,
            }
        } else {
            panic!("Should have gotten Module Location and not Module Location!")
        }
    }

    fn set_instrument_mode_at(&mut self, mode: InstrumentationMode, loc: Location) {
        if let Location::Module {
            func_idx,
            instr_idx,
            ..
        } = loc
        {
            match self.module.functions.get_mut(func_idx as FunctionID).kind {
                FuncKind::Import(_) => panic!("Cannot add an instruction to an imported function"),
                FuncKind::Local(ref mut l) => {
                    l.body.instructions[instr_idx].instr_flag.current_mode = Some(mode)
                }
            }
        } else {
            panic!("Should have gotten module location!")
        }
    }

    fn curr_func_instrument_mode(&self) -> &Option<FuncInstrMode> {
        if let (Location::Module { func_idx, .. }, ..) = self.mod_iterator.curr_loc() {
            match &self.module.functions.get(func_idx as FunctionID).kind {
                FuncKind::Import(_) => panic!("Cannot get an instruction to an imported function"),
                FuncKind::Local(l) => &l.instr_flag.current_mode,
            }
        } else {
            panic!("Should have gotten Module Location and not Module Location!")
        }
    }

    fn set_func_instrument_mode(&mut self, mode: FuncInstrMode) {
        if let (Location::Module { func_idx, .. }, ..) = self.mod_iterator.curr_loc() {
            match self.module.functions.get_mut(func_idx as FunctionID).kind {
                FuncKind::Import(_) => panic!("Cannot get an instruction to an imported function"),
                FuncKind::Local(ref mut l) => l.instr_flag.current_mode = Some(mode),
            }
        } else {
            panic!("Should have gotten Module Location and not Module Location!")
        }
    }

    fn curr_instr_len(&self) -> usize {
        if let (
            Location::Module {
                func_idx,
                instr_idx,
                ..
            },
            ..,
        ) = self.mod_iterator.curr_loc()
        {
            match &self.module.functions.get(func_idx as FunctionID).kind {
                FuncKind::Import(_) => panic!("Cannot get an instruction to an imported function"),
                FuncKind::Local(l) => l.instr_len_at(instr_idx),
            }
        } else {
            panic!("Should have gotten Module Location and not Module Location!")
        }
    }

    fn clear_instr_at(&mut self, loc: Location, mode: InstrumentationMode) {
        if let Location::Module {
            func_idx,
            instr_idx,
            ..
        } = loc
        {
            match self.module.functions.get_mut(func_idx as FunctionID).kind {
                FuncKind::Import(_) => panic!("Cannot add an instruction to an imported function"),
                FuncKind::Local(ref mut l) => {
                    l.clear_instr_at(instr_idx, mode);
                }
            }
            // Only injects if it is an instrumented location
        } else {
            panic!("Should have gotten Module Location!")
        }
    }

    fn add_instr_at(&mut self, loc: Location, instr: Operator<'a>) {
        if let Location::Module {
            func_idx,
            instr_idx,
            ..
        } = loc
        {
            match self.module.functions.get_mut(func_idx as FunctionID).kind {
                FuncKind::Import(_) => panic!("Cannot add an instruction to an imported function"),
                FuncKind::Local(ref mut l) => {
                    l.add_instr(instr, instr_idx);
                }
            }
            // Only injects if it is an instrumented location
        } else {
            panic!("Should have gotten Module Location!")
        }
    }

    fn empty_alternate_at(&mut self, loc: Location) -> &mut Self {
        if let Location::Module {
            func_idx,
            instr_idx,
            ..
        } = loc
        {
            match self.module.functions.get_mut(func_idx).kind {
                FuncKind::Import(_) => panic!("Cannot instrument an imported function"),
                FuncKind::Local(ref mut l) => {
                    l.body.instructions[instr_idx].instr_flag.alternate =
                        Some(InjectedInstrs::default())
                }
            }
        } else {
            panic!("Should have gotten Module Location and not Module Location!")
        }
        self
    }

    fn empty_block_alt_at(&mut self, loc: Location) -> &mut Self {
        if let Location::Module {
            func_idx,
            instr_idx,
            ..
        } = loc
        {
            match self.module.functions.get_mut(func_idx as FunctionID).kind {
                FuncKind::Import(_) => panic!("Cannot instrument an imported function"),
                FuncKind::Local(ref mut l) => {
                    l.body.instructions[instr_idx].instr_flag.block_alt =
                        Some(InjectedInstrs::default());
                    l.instr_flag.has_special_instr |= true;
                }
            }
        } else {
            panic!("Should have gotten Module Location and not Module Location!")
        }
        self
    }

    fn append_tag_at(&mut self, data: Vec<u8>, loc: Location) -> &mut Self {
        if let Location::Module {
            func_idx,
            instr_idx,
            ..
        } = loc
        {
            match self.module.functions.get_mut(func_idx as FunctionID).kind {
                FuncKind::Import(_) => panic!("Cannot instrument an imported function"),
                FuncKind::Local(ref mut l) => {
                    l.append_instr_tag_at(data, instr_idx);
                }
            }
        } else {
            panic!("Should have gotten Module Location and not Module Location!")
        }
        self
    }

    /// Gets the injected instruction at the current location by index
    fn get_injected_val(&self, idx: usize) -> &Operator {
        if let (
            Location::Module {
                func_idx,
                instr_idx,
                ..
            },
            ..,
        ) = self.mod_iterator.curr_loc()
        {
            match &self.module.functions.get(func_idx as FunctionID).kind {
                FuncKind::Import(_) => panic!("Cannot get an instruction to an imported function"),
                FuncKind::Local(l) => l.body.instructions[instr_idx].instr_flag.get_instr(idx),
            }
        } else {
            panic!("Should have gotten Component Location and not Module Location!")
        }
    }
}
impl<'a> IteratingInstrumenter<'a> for ModuleIterator<'_, 'a> {
    fn add_global(&mut self, global: Global) -> GlobalID {
        self.module.globals.add(global)
    }
}

impl AddLocal for ModuleIterator<'_, '_> {
    fn add_local(&mut self, val_type: DataType) -> LocalID {
        let curr_loc = self.curr_loc();
        if let (Location::Module { func_idx, .. }, ..) = curr_loc {
            self.module.functions.add_local(func_idx, val_type)
        } else {
            panic!("Should have gotten Module Location!")
        }
    }
}

// Note: Marked Trait as the same lifetime as component
impl<'a> Iterator for ModuleIterator<'_, 'a> {
    /// Resets the Module Iterator
    fn reset(&mut self) {
        self.mod_iterator.reset();
    }

    /// Goes to the next instruction and returns the instruction
    fn next(&mut self) -> Option<&Operator> {
        match self.mod_iterator.next() {
            false => None,
            true => self.curr_op(),
        }
    }

    /// Returns the Current Location as a Location and a bool value that
    /// says whether the location is at the end of the function.
    fn curr_loc(&self) -> (Location, bool) {
        self.mod_iterator.curr_loc()
    }

    /// Returns the current instruction
    fn curr_op(&self) -> Option<&Operator<'a>> {
        if let (
            Location::Module {
                func_idx,
                instr_idx,
                ..
            },
            ..,
        ) = self.mod_iterator.curr_loc()
        {
            match &self.module.functions.get(func_idx).kind {
                FuncKind::Import(_) => panic!("Cannot get an instruction to an imported function"),
                FuncKind::Local(l) => Some(&l.body.instructions[instr_idx].op),
            }
        } else {
            panic!("Should have gotten Module Location!")
        }
    }
}
