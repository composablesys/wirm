use crate::ir::function::FunctionModifier;
use crate::ir::id::{FunctionID, LocalID, TypeID};
use crate::ir::module::module_functions::{add_local, FuncKind};
use crate::ir::module::side_effects::{InjectType, Injection};
use crate::ir::module::{AsVec, FlagsIter};
use crate::ir::types;
use crate::ir::types::InstrumentationMode::{
    BackEdge, BlockAlt, BlockEntry, BlockExit, SemanticAfter,
};
use crate::ir::types::{BlockType, InjectedInstrs, InstrumentationFlag, InstrumentationMode};
use crate::opcode::{Inject, Instrumenter};
use crate::{DataType, Location, Module, Opcode};
use std::collections::HashMap;
use wasmparser::Operator;

impl<'a> Module<'a> {
    /// Visits the Wirm Module and resolves the special instrumentation by
    /// translating them into the straightforward before/after/alt modes.
    pub(crate) fn resolve_special_instrumentation(
        &mut self,
        func_mapping: &HashMap<u32, u32>,
        global_mapping: &HashMap<u32, u32>,
        memory_mapping: &HashMap<u32, u32>,
        pull_side_effects: bool,
        side_effects: &mut HashMap<InjectType, Vec<Injection<'a>>>,
    ) -> types::Result<()> {
        if !self.num_local_functions > 0 {
            for rel_func_idx in (self.imports.num_funcs - self.imports.num_funcs_added) as usize
                ..self.functions.as_vec().len()
            {
                let func_idx = FunctionID(rel_func_idx as u32);
                if let FuncKind::Import(..) = &self.functions.get_kind(func_idx) {
                    // skip imports
                    continue;
                }

                let mut instr_func_on_entry = None;
                let mut instr_func_on_exit = None;
                if let FuncKind::Local(func) = self.functions.get_kind_mut(func_idx) {
                    if !func.instr_flag.has_special_instr {
                        // skip functions without special instrumentation!
                        continue;
                    }
                    if !func.instr_flag.entry.instrs.is_empty() {
                        instr_func_on_entry = Some(func.instr_flag.entry.clone());
                    }
                    if !func.instr_flag.exit.instrs.is_empty() {
                        instr_func_on_exit = Some(func.instr_flag.exit.clone());
                    }

                    // save off the function entry/exit special mode bodies
                    // NOTE: We have the following logic (which is inefficient, but protects us from
                    // remapping opcode IDs incorrectly by doing it again on an already remapped opcode)
                    // 1. clone special instrumentation, append THAT copy to non-special mode lists
                    // 2. remap the IDs of the original copy of the special instrumentation
                    // 3. append THAT copy of the injections that now have corrected IDs to the
                    //    side effects list.
                    if pull_side_effects {
                        func.add_corrected_special_injections(
                            rel_func_idx as u32,
                            func_mapping,
                            global_mapping,
                            memory_mapping,
                            side_effects,
                        )?;
                    }

                    func.instr_flag.exit.instrs.clear();
                    func.instr_flag.entry.instrs.clear();
                }

                // initialize with 0 to store the func block!
                let mut block_stack: Vec<BlockID> = vec![0];
                let mut delete_block: Option<BlockID> = None;
                let mut retain_end = true;
                let mut resolve_on_else_or_end: HashMap<InstrumentationMode, InstrToInject> =
                    HashMap::new();
                let mut resolve_on_end: HashMap<
                    BlockID,
                    HashMap<InstrumentationMode, InstrToInject>,
                > = HashMap::new();
                let mut backedge_loops: HashMap<BlockID, Vec<Operator>> = HashMap::new();

                if let Some(on_exit) = &mut instr_func_on_exit {
                    if !on_exit.instrs.is_empty() {
                        let on_entry = instr_func_on_entry.get_or_insert_with(|| {
                            // NOTE: This retains the tag information for function exit just in case
                            // that's necessary for the library user. This may need to be handled on
                            // the user side.
                            InjectedInstrs {
                                tag: on_exit.tag.clone(),
                                ..Default::default()
                            }
                        });

                        let func_ty = self.functions.get_type_id(func_idx);
                        let func_results = self.types.get(func_ty).unwrap().results()?;

                        // NOTE: This retains the tag information for function exit just in case
                        // that's necessary for the library user. This may need to be handled on
                        // the user side.
                        let block_ty = self.types.add_func_type_internal(
                            &[],
                            &func_results,
                            on_exit.tag.clone(),
                        );
                        resolve_function_exit_with_block_wrapper(&mut on_entry.instrs, block_ty);
                    }
                }
                let mut builder = self.functions.get_fn_modifier(func_idx)?;

                let flags = if let Some(flags) = builder.body.instructions.get_flags() {
                    #[allow(clippy::unnecessary_to_owned)]
                    FlagsIter::Flags(flags.to_vec().into_iter())
                } else {
                    FlagsIter::Default
                };
                // Must make copy to be able to iterate over body while calling builder.* methods that mutate the instrumentation flag!
                let readable_copy_of_body = builder
                    .body
                    .instructions
                    .get_ops()
                    .to_vec()
                    .into_iter()
                    .zip(flags);

                let mut scratch_locals = ScratchLocals::default();
                for (idx, (op, instrumentation)) in readable_copy_of_body.enumerate() {
                    // resolve function-level instrumentation
                    if let Some(on_entry) = &mut instr_func_on_entry {
                        if !on_entry.instrs.is_empty() {
                            resolve_function_entry(&mut builder, &mut on_entry.instrs, idx);
                        }
                    }
                    if let Some(on_exit) = &mut instr_func_on_exit {
                        if !on_exit.instrs.is_empty() {
                            resolve_function_exit(&mut on_exit.instrs, &mut builder, &op, idx);
                        }
                    }

                    // resolve instruction-level instrumentation
                    match op {
                        Operator::Block { .. }
                        | Operator::Loop { .. }
                        | Operator::If { .. }
                        | Operator::TryTable { .. }
                        | Operator::Try { .. } => {
                            // The block ID will just be the curr len of the stack!
                            block_stack.push(block_stack.len() as u32);

                            // Handle block alt
                            if let Some(block_alt) = &instrumentation.block_alt {
                                // only plan to handle if we're not already removing the block this instr is in
                                if delete_block.is_none()
                                    && plan_resolution_block_alt(
                                        &block_alt.instrs,
                                        &mut builder,
                                        &mut retain_end,
                                        &op,
                                        idx,
                                    )
                                {
                                    // clear any special instr at this location (alt is prioritized)
                                    clear_special_instr_at(&mut builder, idx);
                                    // we've got a match, which injected the alt body. continue to the next instruction
                                    delete_block = Some(*block_stack.last().unwrap());
                                    continue;
                                }
                            }

                            if delete_block.is_some() {
                                // delete this block and skip all instrumentation handling (like below)
                                clear_special_instr_at(&mut builder, idx);
                                builder.empty_alternate_at(Location::Module {
                                    func_idx: FunctionID(0), // not used
                                    instr_idx: idx,
                                });
                                continue;
                            }

                            if let Operator::Loop { .. } = op {
                                if !instrumentation.back_edge.instrs.is_empty() {
                                    backedge_loops.insert(
                                        *block_stack.last().unwrap(),
                                        instrumentation.back_edge.instrs.clone(),
                                    );
                                    builder.clear_instr_at(
                                        Location::Module {
                                            func_idx: FunctionID(0), // not used
                                            instr_idx: idx,
                                        },
                                        BackEdge,
                                    );
                                }
                            }
                        }
                        Operator::Else => {
                            // necessary for if statements with block_exit instrumentation
                            for (mode, instr_to_inject) in resolve_on_else_or_end.iter() {
                                // resolve bodies at the else
                                resolve_bodies(&mut builder, mode, instr_to_inject, idx);
                            }
                            resolve_on_else_or_end.clear();

                            // Handle block alt
                            if let Some(block_alt) = &instrumentation.block_alt {
                                // only plan to handle if we're not already removing the block this instr is in
                                if delete_block.is_none()
                                    && plan_resolution_block_alt(
                                        &block_alt.instrs,
                                        &mut builder,
                                        &mut retain_end,
                                        &op,
                                        idx,
                                    )
                                {
                                    // clear any special instr at this location (alt is prioritized)
                                    clear_special_instr_at(&mut builder, idx);
                                    // we've got a match, which injected the alt body. continue to the next instruction
                                    delete_block = Some(*block_stack.last().unwrap());
                                    continue;
                                }
                            }

                            if delete_block.is_some() {
                                // delete this block and skip all instrumentation handling (like below)
                                clear_special_instr_at(&mut builder, idx);
                                builder.empty_alternate_at(Location::Module {
                                    func_idx: FunctionID(0), // not used
                                    instr_idx: idx,
                                });
                                continue;
                            }
                        }
                        Operator::End => {
                            // Pop the stack and check to see if we have instrumentation to inject!
                            if let Some(block_id) = block_stack.pop() {
                                if let Some(delete_block_id) = delete_block.as_mut() {
                                    // Delete the block, but don't remove the end if we say not to
                                    // should still process instrumentation on the end though...
                                    // (consider if/else where the else has an alt block)
                                    if (*delete_block_id).eq(&block_id) {
                                        // completing the alt block logic, clear state
                                        delete_block = None;
                                        if !retain_end {
                                            // delete this end and skip all instrumentation handling (like below)
                                            clear_special_instr_at(&mut builder, idx);
                                            builder.empty_alternate_at(Location::Module {
                                                func_idx: FunctionID(0), // not used
                                                instr_idx: idx,
                                            });
                                            retain_end = true;
                                            continue;
                                        }
                                        // fall through to the instrumentation handling
                                        retain_end = true;
                                    } else {
                                        // delete this instruction and skip all instrumentation handling (like below)
                                        clear_special_instr_at(&mut builder, idx);
                                        builder.empty_alternate_at(Location::Module {
                                            func_idx: FunctionID(0), // not used
                                            instr_idx: idx,
                                        });
                                        continue;
                                    }
                                }

                                // we've reached an end, make sure resolve_on_else is cleared!
                                // resolve bodies for else OR end
                                for (mode, instr_to_inject) in resolve_on_else_or_end.iter() {
                                    resolve_bodies(&mut builder, mode, instr_to_inject, idx);
                                }
                                resolve_on_else_or_end.clear();

                                // remove top of stack! (end of vec)
                                // remove it, so we don't try to re-inject!
                                if let Some(to_resolve) = resolve_on_end.remove(&block_id) {
                                    for (mode, instr_to_inject) in to_resolve.iter() {
                                        // resolve bodies at the end
                                        resolve_bodies(&mut builder, mode, instr_to_inject, idx);
                                    }
                                    for instr_to_inject in to_resolve.values() {
                                        for flagged in &instr_to_inject.flagged {
                                            // we've finished this flag's read now, release it
                                            scratch_locals.release_read(flagged.bool_flag);
                                        }
                                    }
                                }

                                backedge_loops.remove(&block_id);
                            }
                        }
                        Operator::Br { .. }
                        | Operator::BrIf { .. }
                        | Operator::BrTable { .. }
                        | Operator::BrOnCast { .. }
                        | Operator::BrOnCastFail { .. }
                        | Operator::BrOnNull { .. }
                        | Operator::BrOnNonNull { .. } => {
                            if delete_block.is_some() {
                                // delete this instruction and skip all instrumentation handling (like below)
                                clear_special_instr_at(&mut builder, idx);
                                builder.empty_alternate_at(Location::Module {
                                    func_idx: FunctionID(0), // not used
                                    instr_idx: idx,
                                });
                                continue;
                            }

                            if !backedge_loops.is_empty() {
                                plan_resolution_back_edge(
                                    &mut builder,
                                    &mut scratch_locals,
                                    &backedge_loops,
                                    &block_stack,
                                    &op,
                                    idx,
                                );
                            }
                        }
                        _ => {
                            // non block-structured opcodes
                            if delete_block.is_some() {
                                // delete this instruction and skip all instrumentation handling (like below)
                                clear_special_instr_at(&mut builder, idx);
                                builder.empty_alternate_at(Location::Module {
                                    func_idx: FunctionID(0), // not used
                                    instr_idx: idx,
                                });
                                continue;
                            }
                        }
                    }

                    // plan instruction-level instrumentation resolution
                    // this must go after the above logic to ensure the block_id is on the top of the stack!
                    if instrumentation.has_instr() {
                        // this instruction has instrumentation, check if there is any to resolve!
                        let InstrumentationFlag {
                            semantic_after,
                            block_entry,
                            block_exit,
                            block_alt: _, // handled before here!
                            back_edge: _, // handled before here!
                            before: _,
                            after: _,
                            alternate: _,
                            current_mode: _,
                            // exhaustive to help identify where to add code to handle other special modes.
                        } = instrumentation;

                        // Handle block entry
                        if !block_entry.instrs.is_empty() {
                            resolve_block_entry(&block_entry.instrs, &mut builder, &op, idx);
                            builder.clear_instr_at(
                                Location::Module {
                                    func_idx: FunctionID(0), // not used
                                    instr_idx: idx,
                                },
                                BlockEntry,
                            );
                        }

                        // Handle block exit
                        if !block_exit.instrs.is_empty() {
                            plan_resolution_block_exit(
                                &block_exit.instrs,
                                &block_stack,
                                &mut resolve_on_else_or_end,
                                &mut resolve_on_end,
                                &op,
                            );
                            builder.clear_instr_at(
                                Location::Module {
                                    func_idx: FunctionID(0), // not used
                                    instr_idx: idx,
                                },
                                BlockExit,
                            );
                        }

                        // Handle semantic_after!
                        if !semantic_after.instrs.is_empty() {
                            plan_resolution_semantic_after(
                                &semantic_after.instrs,
                                &mut builder,
                                &mut scratch_locals,
                                &block_stack,
                                &mut resolve_on_end,
                                &op,
                                idx,
                            )?;
                            builder.clear_instr_at(
                                Location::Module {
                                    func_idx: FunctionID(0), // not used
                                    instr_idx: idx,
                                },
                                SemanticAfter,
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

// =========================================
// ==== Special Instrumentation Helpers ====
// =========================================

type BlockID = u32;
type InstrBody<'a> = Vec<Operator<'a>>;
struct InstrBodyFlagged<'a> {
    body: InstrBody<'a>,
    bool_flag: LocalID,
}
struct InstrToInject<'a> {
    flagged: Vec<InstrBodyFlagged<'a>>,
    not_flagged: Vec<InstrBody<'a>>,
}

#[derive(Default)]
struct ScratchLocals {
    /// locals available for reuse
    free: HashMap<DataType, Vec<LocalID>>,
    /// Type of each currently-acquired local, so release only needs the id.
    ty_of: HashMap<LocalID, DataType>,
    /// Locals that still have outstanding reads
    pending_reads: HashMap<LocalID, usize>,
}

impl ScratchLocals {
    fn acquire(&mut self, builder: &mut FunctionModifier, ty: DataType) -> LocalID {
        let id = match self.free.get_mut(&ty).and_then(Vec::pop) {
            Some(id) => id,
            None => Self::alloc(builder, ty),
        };
        self.ty_of.insert(id, ty);
        id
    }

    /// Return a local to the pool for reuse. Call only once its final read has been emitted.
    fn release(&mut self, id: LocalID) {
        if let Some(ty) = self.ty_of.remove(&id) {
            self.free.entry(ty).or_default().push(id);
        }
    }

    /// `id` will be read once more before it can be released
    fn add_read(&mut self, id: LocalID) {
        *self.pending_reads.entry(id).or_insert(0) += 1;
    }

    /// One of the reads has been done, release it once none remain.
    fn release_read(&mut self, id: LocalID) {
        if let Some(remaining) = self.pending_reads.get_mut(&id) {
            *remaining -= 1;
            if *remaining == 0 {
                self.pending_reads.remove(&id);
                self.release(id);
            }
        }
    }

    /// Allocate a brand-new local of the given type.
    fn alloc(builder: &mut FunctionModifier, ty: DataType) -> LocalID {
        add_local(
            ty,
            builder.args.len(),
            &mut builder.body.num_locals,
            &mut builder.body.locals,
        )
    }
}

fn inject_if_body<'a, 'b, 'c>(builder: &mut FunctionModifier<'a, 'b>, body: &[Operator<'c>])
where
    'c: 'b,
{
    builder.if_stmt(BlockType::Empty); // TODO -- breaks for instrumentation that returns stuff
    builder.inject_all(body);
    builder.end();
}

fn resolve_function_entry<'a, 'b, 'c>(
    builder: &mut FunctionModifier<'a, 'b>,
    instr_func_on_entry: &mut InstrBody<'c>,
    idx: usize,
) where
    'c: 'b,
{
    if idx == 0 {
        // we're at the function entry!
        builder.before_at(Location::Module {
            func_idx: FunctionID(0), // not used
            instr_idx: idx,
        });
        builder.inject_all(instr_func_on_entry);

        // remove the contents of the body now that it's been resolved
        instr_func_on_entry.clear();
    }
}

fn resolve_function_exit_with_block_wrapper<'a, 'b, 'c>(
    instr_func_on_entry: &mut InstrBody<'c>,
    block_ty: TypeID,
) where
    'c: 'b,
{
    // To handle `br*` AND fallthrough:
    // Since the relative depth of a branch target
    // cannot exceed its current depth, we can just
    // wrap the function body in a block and put the
    // `exit` instrumentation AFTER the block's `end`.

    // to be handled on resolving func_entry
    instr_func_on_entry.push(Operator::Block {
        blockty: wasmparser::BlockType::from(BlockType::FuncType(block_ty)),
    });
}
fn resolve_function_exit<'a, 'b, 'c>(
    instr_func_on_exit: &mut InstrBody<'c>,
    builder: &mut FunctionModifier<'a, 'b>,
    op: &Operator,
    idx: usize,
) where
    'c: 'b,
{
    // To handle `return`:
    // Place a copy of `exit` BEFORE the `return`
    // Place a copy of `exit` BEFORE the `return_call`
    // Place a copy of `exit` BEFORE the `return_call_indirect`
    // Place a copy of `exit` BEFORE the `return_call_ref`

    // To handle `traps`:
    // Place a copy of `exit` BEFORE the `unreachable`
    // Place a copy of `exit` BEFORE the `throw`
    // Place a copy of `exit` BEFORE the `rethrow`
    // Place a copy of `exit` BEFORE the `throw_ref`
    // Place a copy of `exit` BEFORE the `resume_throw`

    // convert instr to simple before/after/alt
    match op {
        // handle returns
        Operator::Return { .. } |
        Operator::ReturnCall {..} |
        Operator::ReturnCallIndirect {..} |
        Operator::ReturnCallRef {..} |

        // handle traps
        Operator::Unreachable |
        Operator::Throw {..} |
        Operator::Rethrow {..} |
        Operator::ThrowRef |
        Operator::ResumeThrow {..} => {
            // just inject immediately before the instruction
            builder.before_at(Location::Module {
                func_idx: FunctionID(0), // not used
                instr_idx: idx,
            });
            builder.inject_all(instr_func_on_exit);

            // no need to do next part if we've injected!
            return
        }
        _ => {}
    }

    // Handles the actual injection of the added block's `end`
    // and places instr block afterward!
    if idx == builder.body.instructions.len() - 1 {
        // we're at the end of the function!
        builder.before_at(Location::Module {
            func_idx: FunctionID(0), // not used
            instr_idx: idx,
        });
        builder.end(); // end the added wrapper block!
        builder.inject_all(instr_func_on_exit);

        // remove the contents of the body now that it's been resolved
        instr_func_on_exit.clear();
    }
}

fn resolve_block_entry<'a, 'b, 'c>(
    block_entry: &InstrBody<'c>,
    builder: &mut FunctionModifier<'a, 'b>,
    op: &Operator,
    idx: usize,
) where
    'c: 'b,
{
    // convert instr to simple before/after/alt
    match op {
        Operator::Block { .. }
        | Operator::Loop { .. }
        | Operator::If { .. }
        | Operator::Else { .. }
        | Operator::TryTable { .. } => {
            // just inject immediately after the start of the block
            builder.after_at(Location::Module {
                func_idx: FunctionID(0), // not used
                instr_idx: idx,
            });
            builder.inject_all(block_entry);

            // no need to remove the contents of block_entry since we're actually
            // using a read-only copy!
        }
        _ => {
            // no need to remove the contents of block_entry since we're actually
            // using a read-only copy!
        }
    }
}

fn plan_resolution_block_exit<'a, 'b, 'c>(
    block_exit: &InstrBody<'c>,
    block_stack: &[BlockID],
    resolve_on_else_or_end: &mut HashMap<InstrumentationMode, InstrToInject<'c>>,
    resolve_on_end: &mut HashMap<BlockID, HashMap<InstrumentationMode, InstrToInject<'c>>>,
    op: &Operator,
) where
    'c: 'b,
{
    // save instrumentation to be converted to simple before/after/alt
    match op {
        Operator::If { .. } => {
            save_not_flagged_body_to_resolve_inner(
                resolve_on_else_or_end,
                InstrumentationMode::Before,
                block_exit,
            );
        }
        Operator::Block { .. }
        | Operator::Loop { .. }
        | Operator::Else { .. }
        | Operator::TryTable { .. } => {
            // add body-to-inject as non-flagged
            let block_id = block_stack.last().unwrap(); // should always have something (e.g. func block)
            save_not_flagged_body_to_resolve(
                resolve_on_end,
                InstrumentationMode::Before,
                block_exit,
                *block_id,
            );
        }
        _ => {} // skip all other opcodes
    }
}

fn clear_special_instr_at<'a, 'b>(builder: &mut FunctionModifier<'a, 'b>, idx: usize) {
    let loc = Location::Module {
        func_idx: FunctionID(0), // not used
        instr_idx: idx,
    };
    builder.clear_instr_at(loc, BlockAlt);
    builder.clear_instr_at(loc, BlockEntry);
    builder.clear_instr_at(loc, BlockExit);
    builder.clear_instr_at(loc, SemanticAfter);
    builder.clear_instr_at(loc, BackEdge);
}

fn plan_resolution_block_alt<'a, 'b, 'c>(
    block_alt: &InstrBody<'c>,
    builder: &mut FunctionModifier<'a, 'b>,
    retain_end: &mut bool,
    op: &Operator,
    idx: usize,
) -> bool
where
    'c: 'b,
{
    // convert instr to simple before/after/alt
    let mut matched = false;
    match op {
        Operator::Block { .. }
        | Operator::Loop { .. }
        | Operator::If { .. }
        | Operator::Else { .. }
        | Operator::TryTable { .. } => {
            let loc = Location::Module {
                func_idx: FunctionID(0), // not used
                instr_idx: idx,
            };
            if !block_alt.is_empty() {
                // just inject immediately after the start of the block
                builder.alternate_at(loc);
                builder.inject_all(block_alt);
            } else {
                // remove the instruction!
                builder.empty_alternate_at(loc);
            }

            // no need to remove the contents of block_alt since we're actually
            // using a read-only copy!

            matched = true;
            *retain_end = false;
        }
        _ => {}
    }
    if let Operator::Else { .. } = op {
        // We want to keep the end for the module to still be valid (the if will be dangling)
        *retain_end = true;
    }
    matched
}

fn plan_resolution_semantic_after<'a, 'b, 'c>(
    semantic_after: &InstrBody<'c>,
    builder: &mut FunctionModifier<'a, 'b>,
    scratch_locals: &mut ScratchLocals,
    block_stack: &[BlockID],
    resolve_on_end: &mut HashMap<BlockID, HashMap<InstrumentationMode, InstrToInject<'c>>>,
    op: &Operator,
    idx: usize,
) -> types::Result<()>
where
    'c: 'b,
{
    // save instrumentation to be converted to simple before/after/alt
    match op {
        Operator::Block { .. }
        | Operator::Loop { .. }
        | Operator::If { .. }
        | Operator::Else { .. }
        | Operator::TryTable { .. } => {
            // add body-to-inject as non-flagged
            let block_id = block_stack.last().unwrap(); // should always have something (e.g. func block)
            save_not_flagged_body_to_resolve(
                resolve_on_end,
                InstrumentationMode::After,
                semantic_after,
                *block_id,
            );
        }
        Operator::BrTable { targets } => {
            let bool_flag_id = create_bool_flag(builder, scratch_locals, idx, op, semantic_after)?;
            for relative_depth in targets.targets().flatten() {
                save_flagged_body_to_resolve(
                    resolve_on_end,
                    scratch_locals,
                    InstrumentationMode::After,
                    semantic_after,
                    bool_flag_id,
                    relative_depth,
                    *block_stack.last().unwrap(),
                );
            }
            // handle the default as well
            save_flagged_body_to_resolve(
                resolve_on_end,
                scratch_locals,
                InstrumentationMode::After,
                semantic_after,
                bool_flag_id,
                targets.default(),
                *block_stack.last().unwrap(),
            );
        }
        Operator::Br { .. } => {
            // Branch is always taken, just put the body before the `br`
            builder.before_at(Location::Module {
                func_idx: FunctionID(0), // not used
                instr_idx: idx,
            });
            builder.inject_all(semantic_after.as_slice());
        }
        Operator::BrIf { relative_depth }
        | Operator::BrOnCast { relative_depth, .. }
        | Operator::BrOnCastFail { relative_depth, .. }
        | Operator::BrOnNonNull { relative_depth }
        | Operator::BrOnNull { relative_depth } => {
            let bool_flag_id = create_bool_flag(builder, scratch_locals, idx, op, semantic_after)?;
            save_flagged_body_to_resolve(
                resolve_on_end,
                scratch_locals,
                InstrumentationMode::After,
                semantic_after,
                bool_flag_id,
                *relative_depth,
                *block_stack.last().unwrap(),
            );
        }
        _ => {} // skip all other opcodes
    }
    Ok(())
}

fn create_bool_flag<'a, 'b, 'c>(
    builder: &mut FunctionModifier<'a, 'b>,
    scratch_locals: &mut ScratchLocals,
    idx: usize,
    op: &Operator,
    semantic_after: &Vec<Operator<'c>>,
) -> types::Result<LocalID>
where
    'c: 'b,
{
    let bool_flag_id = scratch_locals.acquire(builder, DataType::I32);

    // set flag to true before the opcode
    builder
        .before_at(Location::Module {
            func_idx: FunctionID(0), // not used
            instr_idx: idx,
        })
        .i32_const(1)
        .local_set(bool_flag_id);

    // set flag to false after the opcode
    builder
        .after_at(Location::Module {
            func_idx: FunctionID(0), // not used
            instr_idx: idx,
        })
        .i32_const(0)
        .local_set(bool_flag_id);

    // BrIf, BrOnCast, BrOnNonNull, BrOnNull
    // the bodies should be inserted immediately after too!
    // This is because there is a possibility of fallthrough.
    // The body will not be executed 2x since the flag is set
    // to `false` on fallthrough!
    match op {
        Operator::BrIf { .. }
        | Operator::BrOnCast { .. }
        | Operator::BrOnCastFail { .. }
        | Operator::BrOnNonNull { .. }
        | Operator::BrOnNull { .. } => {
            builder.inject_all(semantic_after.as_slice());
        }
        _ => {}
    }
    Ok(bool_flag_id)
}

fn plan_resolution_back_edge<'a, 'b, 'c>(
    builder: &mut FunctionModifier<'a, 'b>,
    scratch_locals: &mut ScratchLocals,
    backedge_loops: &HashMap<BlockID, Vec<Operator<'c>>>,
    block_stack: &[BlockID],
    op: &Operator,
    idx: usize,
) where
    'c: 'b,
{
    let curr_block = *block_stack.last().unwrap(); // should always have something (e.g. func block)
    let loc = Location::Module {
        func_idx: FunctionID(0), // not used
        instr_idx: idx,
    };

    match op {
        Operator::Br { relative_depth } => {
            if let Some(body) = backedge_loops.get(&(curr_block - *relative_depth)) {
                // back edge unconditionally taken
                builder.before_at(loc);
                builder.inject_all(body.as_slice());
            }
        }
        Operator::BrIf { relative_depth } => {
            if let Some(body) = backedge_loops.get(&(curr_block - *relative_depth)) {
                // back edge taken if ToS is non-zero
                let scratch = scratch_locals.acquire(builder, DataType::I32);
                builder.before_at(loc);
                builder.local_tee(scratch);
                inject_if_body(builder, body.as_slice());
                builder.local_get(scratch);
                scratch_locals.release(scratch);
            }
        }
        Operator::BrTable { targets } => {
            // back edge taken if ToS points to a loop ID target
            let targets_a_back_edge = targets
                .targets()
                .flatten()
                .chain(std::iter::once(targets.default()))
                .any(|rel| backedge_loops.contains_key(&(curr_block - rel)));
            if targets_a_back_edge {
                let scratch = scratch_locals.acquire(builder, DataType::I32);
                builder.before_at(loc);
                builder.local_set(scratch);
                // one guard per table slot that targets a back-edge loop
                for (slot, t) in targets.targets().enumerate() {
                    if let Ok(relative_depth) = t {
                        if let Some(body) = backedge_loops.get(&(curr_block - relative_depth)) {
                            builder.local_get(scratch);
                            builder.i32_const(slot as i32);
                            builder.i32_eq();
                            inject_if_body(builder, body.as_slice());
                        }
                    }
                }
                // ...and one for the default target
                if let Some(body) = backedge_loops.get(&(curr_block - targets.default())) {
                    builder.local_get(scratch);
                    builder.i32_const(targets.len() as i32);
                    builder.i32_ge_u();
                    inject_if_body(builder, body.as_slice());
                }
                builder.local_get(scratch);
                scratch_locals.release(scratch);
            }
        }
        Operator::BrOnCast {
            relative_depth,
            from_ref_type,
            to_ref_type,
        } => {
            if let Some(body) = backedge_loops.get(&(curr_block - *relative_depth)) {
                inject_ref_backedge(
                    builder,
                    scratch_locals,
                    loc,
                    body.as_slice(),
                    from_ref_type,
                    to_ref_type,
                    false,
                );
            }
        }
        Operator::BrOnCastFail {
            relative_depth,
            from_ref_type,
            to_ref_type,
        } => {
            // Taken if the cast FAILS, so negate the ref.test result.
            if let Some(body) = backedge_loops.get(&(curr_block - *relative_depth)) {
                inject_ref_backedge(
                    builder,
                    scratch_locals,
                    loc,
                    body.as_slice(),
                    from_ref_type,
                    to_ref_type,
                    true,
                );
            }
        }
        // NOTE: br_on_null / br_on_non_null use the same local scheme, but their operand's
        // reference type is not carried by the opcode (so we can't declare the scratch local's
        // type without tracking the value stack). They are left uninstrumented for now.
        _ => {}
    }
}

fn inject_ref_backedge<'a, 'b, 'c>(
    builder: &mut FunctionModifier<'a, 'b>,
    scratch_locals: &mut ScratchLocals,
    loc: Location,
    body: &[Operator<'c>],
    from: &wasmparser::RefType,
    to: &wasmparser::RefType,
    on_fail: bool,
) where
    'c: 'b,
{
    let scratch = scratch_locals.acquire(builder, DataType::from(wasmparser::ValType::Ref(*from)));
    builder.before_at(loc);
    builder.local_tee(scratch);
    if to.is_nullable() {
        builder.ref_test_null(to.heap_type().into());
    } else {
        builder.ref_test(to.heap_type().into());
    }
    if on_fail {
        builder.i32_eqz();
    }
    inject_if_body(builder, body);
    builder.local_get(scratch);
    scratch_locals.release(scratch);
}

fn save_not_flagged_body_to_resolve<'a>(
    resolve_on_end: &mut HashMap<BlockID, HashMap<InstrumentationMode, InstrToInject<'a>>>,
    mode: InstrumentationMode,
    body: &Vec<Operator<'a>>,
    block_id: BlockID,
) {
    resolve_on_end
        .entry(block_id)
        .and_modify(|mode_to_instrs| {
            save_not_flagged_body_to_resolve_inner(mode_to_instrs, mode, body);
        })
        .or_insert(HashMap::from([(
            mode,
            InstrToInject {
                flagged: vec![],
                not_flagged: vec![body.to_owned()],
            },
        )]));
}

fn save_not_flagged_body_to_resolve_inner<'a>(
    inner: &mut HashMap<InstrumentationMode, InstrToInject<'a>>,
    mode: InstrumentationMode,
    body: &Vec<Operator<'a>>,
) {
    inner
        .entry(mode)
        .and_modify(|instr_to_inject| {
            instr_to_inject.not_flagged.push(body.to_owned());
        })
        .or_insert(InstrToInject {
            flagged: vec![],
            not_flagged: vec![body.to_owned()],
        });
}

fn save_flagged_body_to_resolve<'a>(
    to_resolve: &mut HashMap<BlockID, HashMap<InstrumentationMode, InstrToInject<'a>>>,
    scratch_locals: &mut ScratchLocals,
    mode: InstrumentationMode,
    body: &Vec<Operator<'a>>,
    bool_flag_id: LocalID,
    relative_depth: u32,
    curr_block: BlockID,
) {
    // one more block `end` will read this flag
    scratch_locals.add_read(bool_flag_id);
    let block_id = curr_block - relative_depth;
    to_resolve
        .entry(block_id)
        .and_modify(|mode_to_instrs| {
            mode_to_instrs
                .entry(mode)
                .and_modify(|instr_to_inject| {
                    instr_to_inject.flagged.push(InstrBodyFlagged {
                        body: body.to_owned(),
                        bool_flag: bool_flag_id,
                    });
                })
                .or_insert(InstrToInject {
                    flagged: vec![InstrBodyFlagged {
                        body: body.to_owned(),
                        bool_flag: bool_flag_id,
                    }],
                    not_flagged: vec![],
                });
        })
        .or_insert(HashMap::from([(
            mode,
            InstrToInject {
                flagged: vec![InstrBodyFlagged {
                    body: body.to_owned(),
                    bool_flag: bool_flag_id,
                }],
                not_flagged: vec![],
            },
        )]));
}

fn resolve_bodies<'a, 'b, 'c>(
    builder: &mut FunctionModifier<'a, 'b>,
    mode: &InstrumentationMode,
    instr_to_inject: &InstrToInject<'c>,
    idx: usize,
) where
    'c: 'b,
{
    let InstrToInject {
        flagged,
        not_flagged,
    } = instr_to_inject;

    let mut is_first = true;
    // inject the bodies predicated with the flag
    for InstrBodyFlagged { body, bool_flag } in flagged.iter() {
        // Inject the bodies in the correct mode at the current END opcode
        let loc = Location::Module {
            func_idx: FunctionID(0), // not used
            instr_idx: idx,
        };
        match mode {
            InstrumentationMode::Before => builder.before_at(loc),
            InstrumentationMode::After => builder.after_at(loc),
            _ => unreachable!(),
        };

        if !is_first {
            // injecting multiple, already have an if statement
            builder.else_stmt();
        }

        // inject flag check
        builder.local_get(*bool_flag);
        builder.if_stmt(BlockType::Empty); // TODO -- This will break for instrumentation that returns stuff...

        // inject body
        builder.inject_all(body);
        is_first = false;
    }
    for _ in 0..flagged.len() {
        // inject end of flag checks
        builder.end();
    }

    // handle non-flagged bodies
    // Inject the bodies AFTER the current END opcode
    let loc = Location::Module {
        func_idx: FunctionID(0), // not used
        instr_idx: idx,
    };
    match mode {
        InstrumentationMode::Before => builder.before_at(loc),
        InstrumentationMode::After => builder.after_at(loc),
        _ => unreachable!(),
    };
    for body in not_flagged.iter() {
        // inject body
        builder.inject_all(body);
    }
}
