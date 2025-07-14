//! Intermediate Representation of a Function

use crate::ir::function::FunctionModifier;
use crate::ir::id::{FunctionID, ImportsID, LocalID, TypeID};
use crate::ir::module::side_effects::{InjectType, Injection};
use crate::ir::module::{GetID, Iter, LocalOrImport, ReIndexable};
use crate::ir::types::{
    Body, FuncInstrFlag, HasInjectTag, InjectTag, Instruction, InstrumentationMode, Tag, TagUtils,
};
use crate::DataType;
use log::warn;
use std::collections::HashMap;
use std::vec::IntoIter;
use wasmparser::Operator;

/// Represents a function. Local or Imported depends on the `FuncKind`.
#[derive(Clone, Debug)]
pub struct Function<'a> {
    pub(crate) kind: FuncKind<'a>,
    name: Option<String>,
    pub(crate) deleted: bool,
}

impl GetID for Function<'_> {
    /// Get the ID of the function
    fn get_id(&self) -> u32 {
        match &self.kind {
            FuncKind::Import(i) => *i.import_fn_id,
            FuncKind::Local(l) => *l.func_id,
        }
    }
}

impl LocalOrImport for Function<'_> {
    /// Check if it's a local function
    fn is_local(&self) -> bool {
        matches!(&self.kind, FuncKind::Local(_))
    }

    /// Check if it's an imported function
    fn is_import(&self) -> bool {
        matches!(&self.kind, FuncKind::Import(_))
    }

    /// Check if this function has been deleted
    fn is_deleted(&self) -> bool {
        self.deleted
    }
}

impl<'a> Function<'a> {
    /// Create a new function
    pub fn new(kind: FuncKind<'a>, name: Option<String>) -> Self {
        Function {
            kind,
            name,
            deleted: false,
        }
    }

    /// Get the TypeID of the function
    pub fn get_type_id(&self) -> TypeID {
        self.kind.get_type()
    }

    /// Change the kind of the Function
    pub(crate) fn set_kind(&mut self, kind: FuncKind<'a>) {
        self.kind = kind;
        // Resets deletion
        self.deleted = false;
    }

    /// Get the kind of the function
    pub fn kind(&self) -> &FuncKind<'a> {
        &self.kind
    }

    /// Unwrap a local function. If it is an imported function, it panics.
    pub fn unwrap_local(&self) -> &LocalFunction<'a> {
        self.kind.unwrap_local()
    }

    /// Unwrap a local function as mutable. If it is an imported function, it panics.
    pub fn unwrap_local_mut(&mut self) -> &mut LocalFunction<'a> {
        self.kind.unwrap_local_mut()
    }

    pub(crate) fn delete(&mut self) {
        self.deleted = true;
    }
}

/// Represents whether a function is a Local Function or an Imported Function
#[derive(Clone, Debug)]
pub enum FuncKind<'a> {
    Local(Box<LocalFunction<'a>>),
    Import(ImportedFunction),
}

impl<'a> FuncKind<'a> {
    /// Unwrap a local function as a read-only reference. If it is an imported function, it panics.
    pub fn unwrap_local(&self) -> &LocalFunction<'a> {
        match &self {
            FuncKind::Local(l) => l,
            FuncKind::Import(_) => panic!("Attempting to unwrap an imported function as a local!!"),
        }
    }
    /// Unwrap a local function as a mutable reference. If it is an imported function, it panics.
    pub fn unwrap_local_mut(&mut self) -> &mut LocalFunction<'a> {
        match self {
            FuncKind::Local(l) => l,
            FuncKind::Import(_) => panic!("Attempting to unwrap an imported function as a local!!"),
        }
    }

    /// Get the TypeID of the function
    pub fn get_type(&self) -> TypeID {
        match &self {
            FuncKind::Local(l) => l.ty_id,
            FuncKind::Import(i) => i.ty_id,
        }
    }
}

impl PartialEq for FuncKind<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (FuncKind::Import(i1), FuncKind::Import(i2)) => i1.ty_id == i2.ty_id,
            (FuncKind::Local(l1), FuncKind::Local(l2)) => l1.ty_id == l2.ty_id,
            _ => false,
        }
    }
}

impl Eq for FuncKind<'_> {}

/// Intermediate Representation of a Local Function
#[derive(Clone, Debug)]
pub struct LocalFunction<'a> {
    pub ty_id: TypeID,
    pub func_id: FunctionID,
    pub instr_flag: FuncInstrFlag<'a>,
    pub body: Body<'a>,
    pub args: Vec<LocalID>,
    pub was_added: bool,
    tag: InjectTag,
}
impl TagUtils for LocalFunction<'_> {
    fn get_or_create_tag(&mut self) -> &mut Tag {
        self.tag.get_or_insert_default()
    }

    fn get_tag(&self) -> &Option<Tag> {
        &self.tag
    }
}
impl HasInjectTag for LocalFunction<'_> {}
impl<'a> LocalFunction<'a> {
    /// Creates a new local function
    pub fn new(
        type_id: TypeID,
        function_id: FunctionID,
        body: Body<'a>,
        num_args: usize,
        was_added: bool,
        tag: InjectTag,
    ) -> Self {
        let mut args = vec![];
        for arg in 0..num_args {
            args.push(LocalID(arg as u32));
        }
        LocalFunction {
            ty_id: type_id,
            func_id: function_id,
            instr_flag: FuncInstrFlag::default(),
            body,
            args,
            was_added,
            tag,
        }
    }
    pub fn add_local(&mut self, ty: DataType) -> LocalID {
        add_local(
            ty,
            self.args.len(),
            &mut self.body.num_locals,
            &mut self.body.locals,
        )
    }

    pub fn add_instr(&mut self, instr: Operator<'a>, instr_idx: usize) {
        if self.instr_flag.current_mode.is_some() {
            // inject at function level
            self.instr_flag.add_instr(instr);
        } else {
            // inject at instruction level
            let is_special = self.body.instructions[instr_idx].add_instr(instr);
            // remember if we injected a special instrumentation (to be resolved before encoding)
            self.instr_flag.has_special_instr |= is_special;
        }
    }

    pub fn instr_len_at(&self, instr_idx: usize) -> usize {
        if self.instr_flag.current_mode.is_some() {
            // get at function level
            self.instr_flag.instr_len()
        } else {
            self.body.instructions[instr_idx].instr_len()
        }
    }

    pub fn append_instr_tag_at(&mut self, data: Vec<u8>, instr_idx: usize) {
        if self.instr_flag.current_mode.is_some() {
            // append at function level
            self.instr_flag.append_to_tag(data);
        } else {
            // append at instruction level
            self.body.instructions[instr_idx]
                .instr_flag
                .append_to_tag(data);
        }
    }

    pub fn clear_instr_at(&mut self, instr_idx: usize, mode: InstrumentationMode) {
        self.body.clear_instr(instr_idx, mode);
    }

    pub(crate) fn add_corrected_special_injections(
        &mut self,
        rel_fid: u32,
        func_mapping: &HashMap<u32, u32>,
        global_mapping: &HashMap<u32, u32>,
        memory_mapping: &HashMap<u32, u32>,
        side_effects: &mut HashMap<InjectType, Vec<Injection<'a>>>,
    ) {
        self.instr_flag.add_injections(
            rel_fid,
            func_mapping,
            global_mapping,
            memory_mapping,
            side_effects,
        );
    }

    pub(crate) fn add_opcode_injections(
        &self,
        rel_fid: u32,
        side_effects: &mut HashMap<InjectType, Vec<Injection<'a>>>,
    ) {
        for (idx, Instruction { instr_flag, .. }) in self.body.instructions.iter().enumerate() {
            instr_flag.add_injections(rel_fid, idx as u32, side_effects);
        }
    }
}

// Must split this out so that the Rust compiler knows that we're not mutating data being iterated
// over in `resolve_special_instrumentation` func.
pub(crate) fn add_local(
    ty: DataType,
    num_params: usize,
    num_locals: &mut u32,
    locals: &mut Vec<(u32, DataType)>,
) -> LocalID {
    let index = num_params + *num_locals as usize;

    let len = locals.len();
    *num_locals += 1;
    if len > 0 {
        let last = len - 1;
        if locals[last].1 == ty {
            locals[last].0 += 1;
        } else {
            locals.push((1, ty));
        }
    } else {
        // If no locals, just append
        locals.push((1, ty));
    }

    LocalID(index as u32)
}

pub(crate) fn add_locals(
    types: &[DataType],
    num_params: usize,
    num_locals: &mut u32,
    locals: &mut Vec<(u32, DataType)>,
) {
    // TODO: Make this more efficient instead of just iterating
    for ty in types.iter() {
        add_local(*ty, num_params, num_locals, locals);
    }
}

/// Intermediate representation of an Imported Function. The actual Import is stored in the Imports field of the module.
#[derive(Clone, Debug)]
pub struct ImportedFunction {
    pub import_id: ImportsID,            // Maps to location in a modules imports
    pub(crate) import_fn_id: FunctionID, // Maps to location in a modules imported functions
    pub ty_id: TypeID,
}

impl ImportedFunction {
    /// Create a new imported function
    pub fn new(id: ImportsID, type_id: TypeID, function_id: FunctionID) -> Self {
        ImportedFunction {
            import_id: id,
            ty_id: type_id,
            import_fn_id: function_id,
        }
    }
}

/// Intermediate representation of all the functions in a module.
#[derive(Clone, Debug, Default)]
pub struct Functions<'a> {
    functions: Vec<Function<'a>>,
    pub(crate) recalculate_ids: bool,
}

impl<'a> Functions<'a> {
    /// Iterate over functions present in the module
    ///
    /// Note: Functions returned by this iterator *may* be deleted.
    pub fn iter(&self) -> impl Iterator<Item = &Function<'a>> {
        Iter::<Function<'a>>::iter(self)
    }
}

impl<'a> Iter<Function<'a>> for Functions<'a> {
    /// Get an iterator for the functions.
    fn iter(&self) -> std::slice::Iter<'_, Function<'a>> {
        self.functions.iter()
    }

    fn get_into_iter(&self) -> IntoIter<Function<'a>> {
        self.functions.clone().into_iter()
    }
}

impl<'a> ReIndexable<Function<'a>> for Functions<'a> {
    /// Get the number of functions
    fn len(&self) -> usize {
        self.functions.len()
    }
    fn remove(&mut self, function_id: u32) -> Function<'a> {
        self.functions.remove(function_id as usize)
    }

    fn insert(&mut self, function_id: u32, func: Function<'a>) {
        self.functions.insert(function_id as usize, func);
    }
    /// Add a new function
    fn push(&mut self, func: Function<'a>) {
        self.functions.push(func);
    }
}

impl<'a> Functions<'a> {
    /// Create a new functions section
    pub fn new(functions: Vec<Function<'a>>) -> Self {
        Functions {
            functions,
            recalculate_ids: false,
        }
    }

    /// Get a function by its FunctionID
    pub fn get_fn_by_id(&self, function_id: FunctionID) -> Option<&Function<'a>> {
        if *function_id < self.functions.len() as u32 {
            return Some(&self.functions[*function_id as usize]);
        }
        None
    }

    /// Checks if there are no functions
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    // =======================
    // ==== FIELD GETTERS ====
    // =======================

    /// Get kind of function
    pub fn get_kind(&self, function_id: FunctionID) -> &FuncKind<'a> {
        &self.functions[*function_id as usize].kind
    }

    /// Get kind of function
    // TODO -- can this be removed?
    pub fn get_kind_mut(&mut self, function_id: FunctionID) -> &mut FuncKind<'a> {
        &mut self.functions[*function_id as usize].kind
    }

    /// Get the name of a function
    pub fn get_name(&self, function_id: FunctionID) -> &Option<String> {
        &self.functions[*function_id as usize].name
    }

    /// Check if a function is a local
    pub fn is_local(&self, function_id: FunctionID) -> bool {
        self.functions[*function_id as usize].is_local()
    }

    /// Check if a function is an import
    pub fn is_import(&self, function_id: FunctionID) -> bool {
        self.functions[*function_id as usize].is_import()
    }

    /// Get the type ID of a function
    pub fn get_type_id(&self, id: FunctionID) -> TypeID {
        self.functions[*id as usize].get_type_id()
    }

    /// Check if it's deleted
    pub fn is_deleted(&self, function_id: FunctionID) -> bool {
        self.functions[*function_id as usize].is_deleted()
    }

    // ======================
    // ==== FUNC GETTERS ====
    // ======================

    /// Get by ID
    pub fn get(&self, function_id: FunctionID) -> &Function<'a> {
        &self.functions[*function_id as usize]
    }

    /// Get mutable function by ID
    pub fn get_mut(&mut self, function_id: FunctionID) -> &mut Function<'a> {
        &mut self.functions[*function_id as usize]
    }

    /// Unwrap local function.
    pub fn unwrap_local(&mut self, function_id: FunctionID) -> &mut LocalFunction<'a> {
        self.functions[*function_id as usize].unwrap_local_mut()
    }

    /// Get local Function ID by name
    pub fn get_local_fid_by_name(&self, name: &str) -> Option<FunctionID> {
        for (idx, func) in self.functions.iter().enumerate() {
            if let FuncKind::Local(l) = &func.kind {
                if let Some(n) = &l.body.name {
                    if n == name {
                        return Some(FunctionID(idx as u32));
                    }
                }
            }
        }
        None
    }

    // =======================
    // ==== MANIPULATIONS ====
    // =======================

    /// Get a function modifier from a function index
    pub fn get_fn_modifier<'b>(
        &'b mut self,
        func_id: FunctionID,
    ) -> Option<FunctionModifier<'b, 'a>> {
        // grab type and section and code section
        match &mut self.functions.get_mut(*func_id as usize)?.kind {
            FuncKind::Local(ref mut l) => {
                // the instrflag should be reset!
                l.instr_flag.finish_instr();
                Some(FunctionModifier::init(
                    &mut l.instr_flag,
                    &mut l.body,
                    &mut l.args,
                ))
            }
            _ => None,
        }
    }

    /// Delete a function
    pub(crate) fn delete(&mut self, id: FunctionID) {
        self.recalculate_ids = true;
        if *id < self.functions.len() as u32 {
            self.functions[*id as usize].delete();
        }
    }

    fn next_id(&self) -> FunctionID {
        FunctionID(self.functions.len() as u32)
    }

    pub(crate) fn add_local_func(
        &mut self,
        mut local_function: LocalFunction<'a>,
        name: Option<String>,
    ) -> FunctionID {
        self.recalculate_ids = true;
        // fix the ID of the function
        let id = self.next_id();
        local_function.func_id = id;

        self.push(Function::new(
            FuncKind::Local(Box::new(local_function)),
            name.clone(),
        ));
        if let Some(name) = name {
            self.set_local_fn_name(id, name);
        }
        id
    }

    pub(crate) fn add_import_func(
        &mut self,
        imp_id: ImportsID,
        ty_id: TypeID,
        name: Option<String>,
        // The id of the function we're using (at least until re-indexing)
        imp_fn_id: u32,
    ) {
        self.recalculate_ids = true;
        assert_eq!(*self.next_id(), imp_fn_id);
        self.functions.push(Function::new(
            FuncKind::Import(ImportedFunction::new(imp_id, ty_id, FunctionID(imp_fn_id))),
            name,
        ));
    }

    pub(crate) fn add_local(&mut self, func_idx: FunctionID, ty: DataType) -> LocalID {
        let local_func = self.functions[*func_idx as usize].unwrap_local_mut();
        local_func.add_local(ty)
    }

    /// Set the name for a local function. Returns false if it is an imported function.
    pub fn set_local_fn_name(&mut self, func_idx: FunctionID, name: String) -> bool {
        match &mut self.functions[*func_idx as usize].kind {
            FuncKind::Import(_) => {
                warn!("is an imported function!");
                return false;
            }
            FuncKind::Local(ref mut l) => l.body.name = Some(name.clone()),
        }
        self.functions[*func_idx as usize].name = Some(name);
        true
    }

    /// Set the name for an imported function. Returns false if it is a local function.
    pub(crate) fn set_imported_fn_name(&mut self, func_idx: FunctionID, name: String) -> bool {
        if self.functions[*func_idx as usize].is_local() {
            warn!("is a local function!");
            return false;
        }
        self.functions[*func_idx as usize].name = Some(name);
        true
    }
}
