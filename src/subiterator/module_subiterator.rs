//! SubIterator for a Module

use crate::ir::id::FunctionID;
use crate::ir::types::Location;
use crate::subiterator::function_subiterator::FuncSubIterator;

/// Sub-iterator for a Module. Keeps track of current location in a Module.
pub struct ModuleSubIterator {
    /// The current function the SubIterator is at
    pub(crate) curr_idx: usize,
    /// Metadata containing a functions index and number_of_instructions
    metadata: Vec<(FunctionID, usize)>,
    /// The function iterator used to keep track of the location in the function.
    pub(crate) func_iterator: FuncSubIterator,
    /// Functions to skip. Provide an empty vector if no functions are to be skipped.
    skip_funcs: Vec<FunctionID>,
}

impl ModuleSubIterator {
    /// Creates a new ModuleSubIterator
    pub fn new(metadata: Vec<(FunctionID, usize)>, skip_funcs: Vec<FunctionID>) -> Self {
        // A module with no local functions (imports-only, or empty)
        // yields an iterator that's already "done": `has_curr` is false,
        // `next` returns false on the first call, `curr_op` returns None.
        if metadata.is_empty() {
            return ModuleSubIterator {
                curr_idx: 0,
                metadata,
                func_iterator: FuncSubIterator::new(0),
                skip_funcs,
            };
        }

        let curr_idx = 0;
        let (_curr_fid, curr_num_instrs) = metadata[curr_idx];
        let mut mod_it = ModuleSubIterator {
            curr_idx,
            metadata,
            func_iterator: FuncSubIterator::new(curr_num_instrs),
            skip_funcs,
        };
        mod_it.handle_skips();

        mod_it
    }

    /// Whether the iterator is currently positioned at a valid function.
    /// Returns `false` for a module with no local functions (or when the
    /// iterator has advanced past the last function).
    pub fn has_curr(&self) -> bool {
        self.curr_idx < self.metadata.len()
    }

    pub fn get_curr_func(&self) -> (FunctionID, usize) {
        self.metadata[self.curr_idx]
    }

    /// Returns the current Location in the Module as a Location
    /// and a bool value that says whether the location is at the end of the function.
    pub fn curr_loc(&self) -> (Location, bool) {
        let curr_instr = self.func_iterator.curr_instr;
        (
            Location::Module {
                func_idx: self.get_curr_func().0,
                instr_idx: curr_instr,
            },
            self.func_iterator.is_end(curr_instr),
        )
    }

    /// Resets the ModuleSubIterator when it is a Child SubIterator of a ComponentSubIterator
    pub(crate) fn reset_from_comp_iterator(&mut self, metadata: Vec<(FunctionID, usize)>) {
        self.metadata = metadata;
        self.reset();
    }

    /// Resets the ModuleSubIterator when it is not a Child SubIterator
    pub fn reset(&mut self) {
        self.curr_idx = 0;
        self.handle_skips();
        self.func_iterator.reset(self.get_curr_func().1);
    }

    fn handle_skips(&mut self) {
        let mut curr_fid = self.get_curr_func().0;
        while self.skip_funcs.contains(&curr_fid) {
            self.curr_idx += 1;
            if self.curr_idx >= self.metadata.len() {
                break;
            }
            curr_fid = self.get_curr_func().0;
        }
    }

    /// Goes to the next function in the module
    fn next_function(&mut self) -> bool {
        if !self.has_next_function() {
            return false;
        }
        self.curr_idx += 1;

        // skip over configured funcs
        self.handle_skips();
        if self.curr_idx < self.metadata.len() {
            self.func_iterator = FuncSubIterator::new(self.get_curr_func().1);
            true
        } else {
            false
        }
    }

    /// Checks if there are functions left to visit
    pub fn has_next_function(&self) -> bool {
        self.curr_idx + 1 < self.metadata.len()
    }

    /// Checks if there are functions left to visit
    pub(crate) fn has_next(&self) -> bool {
        self.func_iterator.has_next() || self.has_next_function()
    }

    /// Goes to the next function
    pub(crate) fn next(&mut self) -> bool {
        if self.func_iterator.has_next() {
            self.func_iterator.next()
        } else {
            self.next_function()
        }
    }
}
