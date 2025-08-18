use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct IdxSpaces {
    // Component-level spaces
    pub comp_func: IdxSpace,
    pub comp_val: IdxSpace,
    pub comp_type: IdxSpace,
    pub comp_inst: IdxSpace,
    pub comp: IdxSpace,

    // Core space (added by component model)
    pub core_inst: IdxSpace, // (these are module instances)
    pub module: IdxSpace,

    // Core spaces that exist at the component-level
    pub core_type: IdxSpace,
    pub core_func: IdxSpace, // these are canonical function decls!

    // General trackers for indices without semantic index spaces
    pub last_processed_imp: usize,
    pub last_processed_exp: usize,
    pub last_processed_alias: usize,
    pub last_processed_custom: usize,
}
impl IdxSpaces {
    pub fn new() -> Self {
        Self {
            comp_func: IdxSpace::new("component_functions".to_string()),
            comp_val: IdxSpace::new("component_values".to_string()),
            comp_type: IdxSpace::new("component_types".to_string()),
            comp_inst: IdxSpace::new("component_instances".to_string()),
            comp: IdxSpace::new("components".to_string()),

            core_inst: IdxSpace::new("core_instances".to_string()),
            module: IdxSpace::new("core_modules".to_string()),

            core_type: IdxSpace::new("core_types".to_string()),
            core_func: IdxSpace::new("core_functions".to_string()),
            ..Self::default()
        }
    }
}

#[derive(Default)]
pub(crate) struct IdxSpace {
    current: usize,
    // This represents the number of external structures that contribute to
    // the current ID
    // (e.g. component type indices come from the (type ...) and (export ...) expressions
    num_external: usize,
    map: HashMap<usize, usize>,
    name: String
}
impl IdxSpace {
    pub fn new(name: String) -> Self {
        Self {
            name,
            current: 0,
            ..Default::default()
        }
    }
    pub fn ooo(&mut self, id: usize) -> bool {
        id > self.current
    }

    pub fn id_for(&mut self, to_check: usize) -> usize {
        // let id = if to_check == self.current - 1 {
        let id = if to_check == self.current {
            // we've reached the end of the current index space
            // self.next() - 1
            self.next()
        } else if self.ooo(to_check) {
            panic!("[{}] we're going out of order, but we're not handling it! checking: {to_check}, current: {}", self.name, self.current)
        } else {
            // we're skipping around!
            println!("[{}] skipping around: {to_check}? -- {}!", self.name, self.current);
            to_check
        };
        id - self.curr_external()
    }

    pub fn next(&mut self) -> usize {
        println!("[{}] {} >> {}", self.name, self.current, self.current + 1);
        let curr = self.current;
        self.current += 1;
        curr
    }

    pub fn curr(&self) -> usize {
        // account for the zero-based indexing
        // self.current - 1
        self.current
    }

    pub fn curr_external(&self) -> usize {
        self.num_external
    }

    pub fn was_external(&mut self) {
        self.num_external += 1;
    }

    pub fn assign(&mut self, from: usize, to: usize) {
        // account for the zero-based indexing
        // println!("[{}] assigning {} to {}", self.name, from + 1, to + 1);
        // self.map.insert(from + 1, to + 1);
        println!("[{}] assigning {} to {}", self.name, from, to + self.num_external);
        self.map.insert(from, to + self.num_external);
    }

    pub fn lookup(&self, id: usize) -> usize {
        // account for the zero-based indexing
        // if let Some(to) = self.map.get(&(id + 1)) {
        if let Some(to) = self.map.get(&(id)) {
            *to
        } else {
            panic!("[{}] Can't find id {} in id-tracker...current: {}", self.name, id, self.current);
        }
    }
}
