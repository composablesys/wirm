//! Iterator to traverse a Component

use crate::ir::component::Component;
use crate::ir::id::{FunctionID, GlobalID, LocalID, ModuleID};
use crate::ir::module::module_functions::FuncKind;
use crate::ir::module::module_globals::Global;
use crate::ir::types::{DataType, FuncInstrMode, InjectedInstrs, InstrumentationMode, Location};
use crate::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
use crate::module_builder::AddLocal;
use crate::opcode::{Inject, InjectAt, Instrumenter, MacroOpcode, Opcode};
use crate::subiterator::component_subiterator::ComponentSubIterator;
use std::collections::HashMap;
use std::iter::Iterator as StdIter;
use wasmparser::Operator;

/// Iterator for a Component.
pub struct ComponentIterator<'a, 'b> {
    /// The Component to iterate
    pub comp: &'a mut Component<'b>,
    /// The SubIterator for this Component
    comp_iterator: ComponentSubIterator,
}

fn print_metadata(metadata: &HashMap<ModuleID, Vec<(FunctionID, usize)>>) {
    for c in metadata.keys() {
        println!("Module: {:?}", c);
        for (m, i) in metadata.get(c).unwrap().iter() {
            println!("Function: {:?} Instr: {:?}", m, i);
        }
    }
}

#[allow(dead_code)]
impl<'a, 'b> ComponentIterator<'a, 'b> {
    /// Creates a new Component Iterator
    pub fn new(
        comp: &'a mut Component<'b>,
        skip_funcs: HashMap<ModuleID, Vec<FunctionID>>,
    ) -> Self {
        // Creates Module -> Function -> Number of Instructions
        let mut metadata = HashMap::new();
        for (mod_idx, m) in comp.modules.iter().enumerate() {
            metadata.insert(ModuleID(mod_idx as u32), m.get_func_metadata());
        }
        print_metadata(&metadata);
        let num_modules = comp.num_modules;
        ComponentIterator {
            comp,
            comp_iterator: ComponentSubIterator::new(
                ModuleID(0),
                num_modules,
                metadata,
                skip_funcs,
            ),
        }
    }

    /// Returns the current module the component iterator is in
    pub fn curr_module(&self) -> ModuleID {
        if let (
            Location::Component {
                mod_idx,
                func_idx: _func_idx,
                instr_idx: _instr_idx,
                ..
            },
            ..,
        ) = self.curr_loc()
        {
            mod_idx
        } else {
            panic!("Should have gotten component location");
        }
    }

    pub fn curr_op_owned(&self) -> Option<Operator<'b>> {
        if self.comp_iterator.end() {
            None
        } else if let (
            Location::Component {
                mod_idx,
                func_idx,
                instr_idx,
                ..
            },
            ..,
        ) = self.comp_iterator.curr_loc()
        {
            match &self.comp.modules[*mod_idx as usize]
                .functions
                .get(func_idx)
                .kind
            {
                FuncKind::Import(_) => None,
                FuncKind::Local(l) => Some(l.body.instructions[instr_idx].op.clone()),
            }
        } else {
            panic!("Should have gotten Component Location!")
        }
    }
}

impl<'b> Inject<'b> for ComponentIterator<'_, 'b> {
    /// Injects an Operator at the current location
    ///
    /// # Example
    /// ```no_run
    /// use std::collections::HashMap;
    /// use wirm::ir::component::Component;
    /// use wirm::iterator::component_iterator::ComponentIterator;
    /// use wasmparser::Operator;
    /// use wirm::ir::types::{Location};
    /// use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
    /// use wirm::opcode::{Instrumenter, Opcode};
    ///
    /// let file = "path_to_file";
    /// let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    /// let mut component = Component::parse(&buff, false).expect("Unable to parse");
    /// let mut comp_it = ComponentIterator::new(&mut component, HashMap::new());
    ///
    /// // Everytime there is a `call 1` instruction we want to inject an `i32.const 0`
    /// let interested = Operator::Call { function_index: 1 };
    ///
    /// loop {
    ///     let op = comp_it.curr_op();
    ///     let instr_mode = comp_it.curr_instrument_mode();
    ///
    ///     if let Location::Component {
    ///         mod_idx,
    ///         func_idx,
    ///         instr_idx,
    ///         ..
    ///     } = comp_it.curr_loc().0
    ///     {
    ///         if *comp_it.curr_op().unwrap() == interested {
    ///             comp_it.before().i32_const(1);
    ///         }
    ///         if comp_it.next().is_none() {
    ///             break;
    ///         };
    ///     } else {
    ///         // Ensures we only get the location of a module while parsing a component
    ///         panic!("Should've gotten Component Location!");
    ///     }
    /// }
    /// ```
    fn inject(&mut self, instr: Operator<'b>) {
        if let (
            Location::Component {
                mod_idx,
                func_idx,
                instr_idx,
                ..
            },
            ..,
        ) = self.curr_loc()
        {
            match self.comp.modules[*mod_idx as usize]
                .functions
                .get_mut(func_idx)
                .kind
            {
                FuncKind::Import(_) => panic!("Can't inject into an imported function!"),
                FuncKind::Local(ref mut l) => l.add_instr(instr, instr_idx),
            }
        } else {
            panic!("Should have gotten component location!")
        }
    }
}
impl<'b> InjectAt<'b> for ComponentIterator<'_, 'b> {
    fn inject_at(&mut self, idx: usize, mode: InstrumentationMode, instr: Operator<'b>) {
        if let (
            Location::Component {
                mod_idx, func_idx, ..
            },
            ..,
        ) = self.curr_loc()
        {
            let loc = Location::Component {
                mod_idx,
                func_idx,
                instr_idx: idx,
            };
            self.set_instrument_mode_at(mode, loc);
            self.add_instr_at(loc, instr);
        } else {
            panic!("Should have gotten Component Location!")
        }
    }
}
impl<'b> Opcode<'b> for ComponentIterator<'_, 'b> {}
impl<'b> MacroOpcode<'b> for ComponentIterator<'_, 'b> {}
impl<'b> Instrumenter<'b> for ComponentIterator<'_, 'b> {
    ///Can be called after finishing some instrumentation to reset the mode.
    fn finish_instr(&mut self) {
        if let (
            Location::Component {
                mod_idx,
                func_idx,
                instr_idx,
                ..
            },
            ..,
        ) = self.comp_iterator.curr_loc()
        {
            match &mut self.comp.modules[*mod_idx as usize]
                .functions
                .get_mut(func_idx)
                .kind
            {
                FuncKind::Import(_) => panic!("Can't inject into an imported function!"),
                FuncKind::Local(l) => {
                    l.instr_flag.finish_instr();
                    l.body.instructions[instr_idx].instr_flag.finish_instr();
                }
            }
        } else {
            panic!("Should have gotten Component Location and not Module Location!")
        }
    }

    /// Returns the Instrumentation at the current Location
    fn curr_instrument_mode(&self) -> &Option<InstrumentationMode> {
        if let (
            Location::Component {
                mod_idx,
                func_idx,
                instr_idx,
                ..
            },
            ..,
        ) = self.comp_iterator.curr_loc()
        {
            match &self.comp.modules[*mod_idx as usize]
                .functions
                .get(func_idx)
                .kind
            {
                FuncKind::Import(_) => {
                    panic!("Can't get instrumentation from an imported function!")
                }
                FuncKind::Local(l) => &l.body.instructions[instr_idx].instr_flag.current_mode,
            }
        } else {
            panic!("Should have gotten Component Location and not Module Location!")
        }
    }

    fn set_instrument_mode_at(&mut self, mode: InstrumentationMode, loc: Location) {
        if let Location::Component {
            mod_idx,
            func_idx,
            instr_idx,
            ..
        } = loc
        {
            match self.comp.modules[*mod_idx as usize]
                .functions
                .get_mut(func_idx)
                .kind
            {
                FuncKind::Import(_) => panic!("Can't instrument into an imported function!"),
                FuncKind::Local(ref mut l) => {
                    l.body.instructions[instr_idx].instr_flag.current_mode = Some(mode)
                }
            }
        } else {
            panic!("Should have gotten component location!")
        }
    }

    fn curr_func_instrument_mode(&self) -> &Option<FuncInstrMode> {
        if let (
            Location::Component {
                mod_idx, func_idx, ..
            },
            ..,
        ) = self.comp_iterator.curr_loc()
        {
            match &self.comp.modules[*mod_idx as usize]
                .functions
                .get(func_idx)
                .kind
            {
                FuncKind::Import(_) => {
                    panic!("Can't get instrumentation from an imported function!")
                }
                FuncKind::Local(l) => &l.instr_flag.current_mode,
            }
        } else {
            panic!("Should have gotten Component Location and not Module Location!")
        }
    }

    fn set_func_instrument_mode(&mut self, mode: FuncInstrMode) {
        if let (
            Location::Component {
                mod_idx, func_idx, ..
            },
            ..,
        ) = self.curr_loc()
        {
            match self.comp.modules[*mod_idx as usize]
                .functions
                .get_mut(func_idx)
                .kind
            {
                FuncKind::Import(_) => panic!("Can't instrument into an imported function!"),
                FuncKind::Local(ref mut l) => l.instr_flag.current_mode = Some(mode),
            }
        } else {
            panic!("Should have gotten component location!")
        }
    }

    fn curr_instr_len(&self) -> usize {
        if let (
            Location::Component {
                mod_idx,
                func_idx,
                instr_idx,
                ..
            },
            ..,
        ) = self.comp_iterator.curr_loc()
        {
            match &self.comp.modules[*mod_idx as usize]
                .functions
                .get(func_idx)
                .kind
            {
                FuncKind::Import(_) => {
                    panic!("Can't get instrumentation from an imported function!")
                }
                FuncKind::Local(l) => l.instr_len_at(instr_idx),
            }
        } else {
            panic!("Should have gotten Component Location and not Module Location!")
        }
    }

    fn clear_instr_at(&mut self, loc: Location, mode: InstrumentationMode) {
        if let Location::Component {
            mod_idx,
            func_idx,
            instr_idx,
            ..
        } = loc
        {
            match self.comp.modules[*mod_idx as usize]
                .functions
                .get_mut(func_idx)
                .kind
            {
                FuncKind::Import(_) => panic!("Can't instrument into an imported function!"),
                FuncKind::Local(ref mut l) => l.clear_instr_at(instr_idx, mode),
            }
        } else {
            panic!("Should have gotten component location!")
        }
    }

    fn add_instr_at(&mut self, loc: Location, instr: Operator<'b>) {
        if let Location::Component {
            mod_idx,
            func_idx,
            instr_idx,
            ..
        } = loc
        {
            match self.comp.modules[*mod_idx as usize]
                .functions
                .get_mut(func_idx)
                .kind
            {
                FuncKind::Import(_) => panic!("Can't instrument into an imported function!"),
                FuncKind::Local(ref mut l) => {
                    l.add_instr(instr, instr_idx);
                }
            }
        } else {
            panic!("Should have gotten Component Location and not Module Location!")
        }
    }

    fn empty_alternate_at(&mut self, loc: Location) -> &mut Self {
        if let Location::Component {
            mod_idx,
            func_idx,
            instr_idx,
            ..
        } = loc
        {
            match self.comp.modules[*mod_idx as usize]
                .functions
                .get_mut(func_idx)
                .kind
            {
                FuncKind::Import(_) => panic!("Can't instrument into an imported function!"),
                FuncKind::Local(ref mut l) => {
                    l.body.instructions[instr_idx].instr_flag.alternate =
                        Some(InjectedInstrs::default());
                }
            }
        } else {
            panic!("Should have gotten Component Location and not Module Location!")
        }

        self
    }

    fn empty_block_alt_at(&mut self, loc: Location) -> &mut Self {
        if let Location::Component {
            mod_idx,
            func_idx,
            instr_idx,
            ..
        } = loc
        {
            match self.comp.modules[*mod_idx as usize]
                .functions
                .get_mut(func_idx)
                .kind
            {
                FuncKind::Import(_) => panic!("Can't instrument into an imported function!"),
                FuncKind::Local(ref mut l) => {
                    l.body.instructions[instr_idx].instr_flag.block_alt =
                        Some(InjectedInstrs::default());
                    l.instr_flag.has_special_instr |= true;
                }
            }
        } else {
            panic!("Should have gotten Component Location and not Module Location!")
        }

        self
    }

    fn append_tag_at(&mut self, data: Vec<u8>, loc: Location) -> &mut Self {
        if let Location::Component {
            mod_idx,
            func_idx,
            instr_idx,
            ..
        } = loc
        {
            match &mut self.comp.modules[*mod_idx as usize]
                .functions
                .get_mut(func_idx)
                .kind
            {
                FuncKind::Import(_) => panic!("Can't inject into an imported function!"),
                FuncKind::Local(l) => l.append_instr_tag_at(data, instr_idx),
            }
        } else {
            panic!("Should have gotten Component Location and not Module Location!")
        }
        self
    }

    /// Gets the injected instruction at the current location by index
    fn get_injected_val(&self, idx: usize) -> &Operator {
        if let (
            Location::Component {
                mod_idx,
                func_idx,
                instr_idx,
                ..
            },
            ..,
        ) = self.comp_iterator.curr_loc()
        {
            match &self.comp.modules[*mod_idx as usize]
                .functions
                .get(func_idx)
                .kind
            {
                FuncKind::Import(_) => panic!("Can't inject into an imported function!"),
                FuncKind::Local(l) => l.body.instructions[instr_idx].instr_flag.get_instr(idx),
            }
        } else {
            panic!("Should have gotten Component Location and not Module Location!")
        }
    }
}
impl<'b> IteratingInstrumenter<'b> for ComponentIterator<'_, 'b> {
    fn add_global(&mut self, global: Global) -> GlobalID {
        let curr_mod = *self.curr_module() as usize;
        self.comp.modules[curr_mod].globals.add(global)
    }
}

// Note: Marked Trait as the same lifetime as component
impl Iterator for ComponentIterator<'_, '_> {
    /// Resets the Component Iterator
    fn reset(&mut self) {
        self.comp_iterator.reset();
    }

    /// Goes to the next instruction
    fn next(&mut self) -> Option<&Operator> {
        match self.comp_iterator.next() {
            false => None,
            true => self.curr_op(),
        }
    }

    /// Returns the Current Location as a Location and a bool value that
    /// says whether the location is at the end of the function.
    fn curr_loc(&self) -> (Location, bool) {
        self.comp_iterator.curr_loc()
    }

    /// Returns the instruction at the current location
    fn curr_op(&self) -> Option<&Operator> {
        if self.comp_iterator.end() {
            None
        } else if let (
            Location::Component {
                mod_idx,
                func_idx,
                instr_idx,
                ..
            },
            ..,
        ) = self.comp_iterator.curr_loc()
        {
            match &self.comp.modules[*mod_idx as usize]
                .functions
                .get(func_idx)
                .kind
            {
                FuncKind::Import(_) => panic!("Can't inject into an imported function!"),
                FuncKind::Local(l) => Some(&l.body.instructions[instr_idx].op),
            }
        } else {
            panic!("Should have gotten Component Location and not Module Location!")
        }
    }
}

impl AddLocal for ComponentIterator<'_, '_> {
    fn add_local(&mut self, val_type: DataType) -> LocalID {
        let curr_loc = self.curr_loc().0;
        if let Location::Component {
            mod_idx, func_idx, ..
        } = curr_loc
        {
            {
                self.comp.modules[*mod_idx as usize]
                    .functions
                    .add_local(func_idx, val_type)
            }
        } else {
            panic!("Should have gotten Component Location and not Module Location!")
        }
    }
}
