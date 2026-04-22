//! Traits that define the injection behavior for wasm opcodes.
//!
//! The [`Opcode`] trait is the primary interface for injecting instructions.
//! Its methods are generated automatically from `wasmparser::for_each_operator!`,
//! so new opcodes added to wasmparser are automatically available here without
//! any manual additions.

use crate::ir::module::module_types::HeapType;
use crate::ir::types::{BlockType, FuncInstrMode, InstrumentationMode};
use crate::Location;
use wasmparser::Operator;

// ── Type-substitution helpers ─────────────────────────────────────────────────
//
// `wirm_ty!(field_name, original_wasmparser_type)` maps known index field names
// to their wirm typed-ID equivalents. Every other field falls back to the
// original wasmparser type unchanged.
macro_rules! wirm_ty {
    (function_index, $orig:ty) => {
        crate::ir::id::FunctionID
    };
    (local_index,    $orig:ty) => {
        crate::ir::id::LocalID
    };
    (global_index,   $orig:ty) => {
        crate::ir::id::GlobalID
    };
    (type_index,     $orig:ty) => {
        crate::ir::id::TypeID
    };
    (table_index,    $orig:ty) => {
        crate::ir::id::TableID
    };
    (table,          $orig:ty) => {
        crate::ir::id::TableID
    };
    (dst_table,      $orig:ty) => {
        crate::ir::id::TableID
    };
    (src_table,      $orig:ty) => {
        crate::ir::id::TableID
    };
    (mem,            $orig:ty) => {
        crate::ir::id::MemoryID
    };
    (dst_mem,        $orig:ty) => {
        crate::ir::id::MemoryID
    };
    (src_mem,        $orig:ty) => {
        crate::ir::id::MemoryID
    };
    (data_index,         $orig:ty) => {
        crate::ir::id::DataSegmentID
    };
    (array_data_index,   $orig:ty) => {
        crate::ir::id::DataSegmentID
    };
    (elem_index,         $orig:ty) => {
        crate::ir::id::ElementID
    };
    (array_elem_index,   $orig:ty) => {
        crate::ir::id::ElementID
    };
    (struct_type_index,      $orig:ty) => {
        crate::ir::id::TypeID
    };
    (array_type_index,       $orig:ty) => {
        crate::ir::id::TypeID
    };
    (array_type_index_dst,   $orig:ty) => {
        crate::ir::id::TypeID
    };
    (array_type_index_src,   $orig:ty) => {
        crate::ir::id::TypeID
    };
    (field_index,    $orig:ty) => {
        crate::ir::id::FieldID
    };
    // Fall-through: non-index fields keep their original wasmparser type.
    ($name:ident,    $orig:ty) => {
        $orig
    };
}

// `wirm_val!(field_name, $ident)` produces `*$ident` for ID-typed fields (deref
// to u32) and `$ident` unchanged for everything else.
//
// The second argument MUST be the same `$arg:ident` metavariable that the outer
// macro bound at the call site, so that the generated expression carries the
// correct macro hygiene context for the method parameter it refers to.
macro_rules! wirm_val {
    (function_index,      $arg:ident) => {
        *$arg
    };
    (local_index,         $arg:ident) => {
        *$arg
    };
    (global_index,        $arg:ident) => {
        *$arg
    };
    (type_index,          $arg:ident) => {
        *$arg
    };
    (table_index,         $arg:ident) => {
        *$arg
    };
    (table,               $arg:ident) => {
        *$arg
    };
    (dst_table,           $arg:ident) => {
        *$arg
    };
    (src_table,           $arg:ident) => {
        *$arg
    };
    (mem,                 $arg:ident) => {
        *$arg
    };
    (dst_mem,             $arg:ident) => {
        *$arg
    };
    (src_mem,             $arg:ident) => {
        *$arg
    };
    (data_index,          $arg:ident) => {
        *$arg
    };
    (array_data_index,    $arg:ident) => {
        *$arg
    };
    (elem_index,          $arg:ident) => {
        *$arg
    };
    (array_elem_index,    $arg:ident) => {
        *$arg
    };
    (struct_type_index,   $arg:ident) => {
        *$arg
    };
    (array_type_index,    $arg:ident) => {
        *$arg
    };
    (array_type_index_dst, $arg:ident) => {
        *$arg
    };
    (array_type_index_src, $arg:ident) => {
        *$arg
    };
    (field_index,         $arg:ident) => {
        *$arg
    };
    // Fall-through: non-index fields are passed as-is.
    ($name:ident,         $arg:ident) => {
        $arg
    };
}

// ── Primary opcode-generation macro ──────────────────────────────────────────
//
// Driven by `wasmparser::for_each_operator!`.  Each operator that isn't
// explicitly skipped below gets a trait method whose name is the snake_case
// form of the operator's PascalCase name (e.g. `I32Add` → `i32_add`).
//
// Skipped categories:
//   • Operators that are Rust keywords when lowercased (Block/Loop/If/Else/Return)
//     → implemented manually with wirm-style names.
//   • Operators whose argument types need wirm-specific conversion
//     (BlockType, Ieee32/64, BrTable, HeapType for ref ops)
//     → implemented manually below.
//   • Proposals with argument types that have no current wirm support
//     (@stack_switching, @shared_everything_threads).
macro_rules! define_opcode_methods {
    // Entry: receive the full operator list, dispatch one at a time.
    ($( @$proposal:ident $op:ident $({ $($arg:ident: $argty:ty),* })? => $visit:ident ($($ann:tt)*))*) => {
        $( define_opcode_methods!(one @$proposal $op $({ $($arg: $argty),* })? => $visit); )*
    };

    // ── Skip entire proposals ────────────────────────────────────────────────
    (one @stack_switching          $($rest:tt)*) => {};
    (one @shared_everything_threads $($rest:tt)*) => {};

    // ── Skip operators requiring manual implementations ──────────────────────
    //
    // Each skipped op below has a hand-written counterpart further down in
    // this file (look for `fn <name>` in `impl Opcode`), typically because
    // the argument type needs a wirm-side wrapper (BlockType, HeapType) or
    // the op's name collides with a Rust keyword.
    //
    // Keyword conflicts / BlockType conversions:
    (one @mvp Block    $($rest:tt)*) => {};
    (one @mvp Loop     $($rest:tt)*) => {};
    (one @mvp If       $($rest:tt)*) => {};
    (one @mvp Else     $($rest:tt)*) => {};
    (one @mvp Return   $($rest:tt)*) => {};
    // Complex argument types:
    (one @mvp BrTable          $($rest:tt)*) => {};
    (one @mvp F32Const         $($rest:tt)*) => {};
    (one @mvp F64Const         $($rest:tt)*) => {};
    (one @reference_types RefNull          $($rest:tt)*) => {};
    (one @reference_types TypedSelect      $($rest:tt)*) => {};
    (one @reference_types TypedSelectMulti $($rest:tt)*) => {};
    (one @exceptions    TryTable           $($rest:tt)*) => {};
    (one @legacy_exceptions Try            $($rest:tt)*) => {};
    // GC: HeapType / RefType arguments (provided manually with wirm types):
    (one @gc RefTestNonNull  $($rest:tt)*) => {};
    (one @gc RefTestNullable $($rest:tt)*) => {};
    (one @gc RefCastNonNull  $($rest:tt)*) => {};
    (one @gc RefCastNullable $($rest:tt)*) => {};
    (one @gc BrOnCast        $($rest:tt)*) => {};
    (one @gc BrOnCastFail    $($rest:tt)*) => {};
    (one @custom_descriptors RefCastDescNonNull  $($rest:tt)*) => {};
    (one @custom_descriptors RefCastDescNullable $($rest:tt)*) => {};
    (one @custom_descriptors BrOnCastDesc        $($rest:tt)*) => {};
    (one @custom_descriptors BrOnCastDescFail    $($rest:tt)*) => {};

    // ── Auto-generate: operator with no arguments ────────────────────────────
    (one @$_proposal:ident $op:ident => $visit:ident) => {
        paste::paste! {
            fn [<$op:snake>](&mut self) -> &mut Self {
                self.inject(Operator::$op);
                self
            }
        }
    };

    // ── Auto-generate: operator with one or more arguments ───────────────────
    // For each argument, `wirm_ty!` substitutes known index names with typed IDs;
    // `wirm_val!` dereferences those IDs back to u32 for the Operator constructor.
    // `wirm_val!` receives `($arg, $arg)` so the emitted expression uses the
    // call-site `$arg` metavariable (correct hygiene context, not a literal).
    (one @$_proposal:ident $op:ident { $($arg:ident: $argty:ty),* } => $visit:ident) => {
        paste::paste! {
            fn [<$op:snake>](&mut self, $($arg: wirm_ty!($arg, $argty)),*) -> &mut Self {
                self.inject(Operator::$op { $($arg: wirm_val!($arg, $arg)),* });
                self
            }
        }
    };
}

// ─────────────────────────────────────────────────────────────────────────────

/// Defines instrumentation behaviour.
pub trait Instrumenter<'a> {
    /// Can be called after finishing some instrumentation to reset the mode.
    fn finish_instr(&mut self);
    /// Get the InstrumentType of the current location
    fn curr_instrument_mode(&self) -> Option<InstrumentationMode>;

    /// Sets the type of Instrumentation Type of the specified location
    fn set_instrument_mode_at(&mut self, mode: InstrumentationMode, loc: Location);

    /// Get the InstrumentType of the current function
    fn curr_func_instrument_mode(&self) -> &Option<FuncInstrMode>;

    /// Sets the type of Instrumentation Type of the current function
    fn set_func_instrument_mode(&mut self, mode: FuncInstrMode);

    fn curr_instr_len(&self) -> usize;

    // ==== FUNC INSTR INJECTION ====

    /// Mark the current function to InstrumentFuncEntry
    fn func_entry(&mut self) -> &mut Self {
        self.set_func_instrument_mode(FuncInstrMode::Entry);
        self
    }

    /// Mark the current function to InstrumentFuncExit
    fn func_exit(&mut self) -> &mut Self {
        self.set_func_instrument_mode(FuncInstrMode::Exit);
        self
    }

    // ==== INSTR INJECTION ====
    /// Clears the instruction at a given Location
    fn clear_instr_at(&mut self, loc: Location, mode: InstrumentationMode);

    /// Splice a new instruction into a specific location
    fn add_instr_at(&mut self, loc: Location, instr: Operator<'a>);

    /// Injects an Instruction with InstrumentationMode `Before` at a given location
    fn before_at(&mut self, loc: Location) -> &mut Self {
        self.set_instrument_mode_at(InstrumentationMode::Before, loc);
        self
    }

    /// Injects an Instruction with InstrumentationMode `After` at a given location
    fn after_at(&mut self, loc: Location) -> &mut Self {
        self.set_instrument_mode_at(InstrumentationMode::After, loc);
        self
    }

    /// Injects an Instruction with InstrumentationMode `Alternate` at a given location
    fn alternate_at(&mut self, loc: Location) -> &mut Self {
        self.set_instrument_mode_at(InstrumentationMode::Alternate, loc);
        self
    }

    /// Injects an empty InstrumentationMode `Alternate` at a given location
    fn empty_alternate_at(&mut self, loc: Location) -> &mut Self;

    /// Injects a Semantic After at a given location
    fn semantic_after_at(&mut self, loc: Location) -> &mut Self {
        self.set_instrument_mode_at(InstrumentationMode::SemanticAfter, loc);
        self
    }

    /// Injects a block entry at a given location
    fn block_entry_at(&mut self, loc: Location) -> &mut Self {
        self.set_instrument_mode_at(InstrumentationMode::BlockEntry, loc);
        self
    }

    /// Injects a block exit at a given location
    fn block_exit_at(&mut self, loc: Location) -> &mut Self {
        self.set_instrument_mode_at(InstrumentationMode::BlockExit, loc);
        self
    }

    /// Injects a block alternate at a given location
    fn block_alt_at(&mut self, loc: Location) -> &mut Self {
        self.set_instrument_mode_at(InstrumentationMode::BlockAlt, loc);
        self
    }

    /// Injects an empty block alternate at a given location
    fn empty_block_alt_at(&mut self, loc: Location) -> &mut Self;

    fn append_tag_at(&mut self, data: Vec<u8>, loc: Location) -> &mut Self;
}

/// Defines Injection behaviour at the current location of the Iterator
pub trait Inject<'a> {
    /// Inject an operator at the current location
    fn inject(&mut self, instr: Operator<'a>);

    /// Inject multiple operators at the current location
    fn inject_all(&mut self, instrs: &[Operator<'a>]) -> &mut Self {
        instrs.iter().for_each(|instr| {
            self.inject(instr.to_owned());
        });
        self
    }
}

/// Defines Injection Behaviour at a given location
pub trait InjectAt<'a> {
    /// Inject an Instruction at a given Location with a given `InstrumentationMode`
    fn inject_at(&mut self, idx: usize, mode: InstrumentationMode, instr: Operator<'a>);
}

#[allow(dead_code)]
/// Defines injection behaviour for every Wasm instruction.
///
/// Methods are generated automatically via [`wasmparser::for_each_operator`]; the
/// method name is the snake_case rendering of the operator's PascalCase variant
/// name (e.g. `I32Add` → `i32_add`, `V128Load8Lane` → `v128_load8_lane`).
///
/// Arguments that represent module-section indices accept wirm's typed ID wrappers
/// ([`FunctionID`], [`LocalID`], [`GlobalID`], [`TypeID`], [`TableID`],
/// [`MemoryID`], [`DataSegmentID`], [`ElementID`], [`FieldID`]) rather than bare
/// `u32`s, providing a semantic signal about what kind of index is expected.
///
/// A handful of operators require manual implementations due to keyword conflicts
/// or type-conversion ergonomics; those are listed at the bottom of this trait.
///
/// [`FunctionID`]: crate::ir::id::FunctionID
/// [`LocalID`]: crate::ir::id::LocalID
/// [`GlobalID`]: crate::ir::id::GlobalID
/// [`TypeID`]: crate::ir::id::TypeID
/// [`TableID`]: crate::ir::id::TableID
/// [`MemoryID`]: crate::ir::id::MemoryID
/// [`DataSegmentID`]: crate::ir::id::DataSegmentID
/// [`ElementID`]: crate::ir::id::ElementID
/// [`FieldID`]: crate::ir::id::FieldID
pub trait Opcode<'a>: Inject<'a> {
    // ── Auto-generated methods ────────────────────────────────────────────────
    // Produced by wasmparser::for_each_operator! for all operators not listed
    // in define_opcode_methods!'s skip rules.
    wasmparser::for_each_operator!(define_opcode_methods);

    // ── Keyword renames ───────────────────────────────────────────────────────
    // These operators have names that are Rust keywords when lowercased, so they
    // get a `_stmt` suffix (or keep their natural name if it isn't a keyword).

    /// Inject a `block` instruction.
    fn block(&mut self, block_type: BlockType) -> &mut Self {
        self.inject(Operator::Block {
            blockty: wasmparser::BlockType::from(block_type),
        });
        self
    }

    /// Inject a `loop` instruction (`loop` is a Rust keyword; use `loop_stmt`).
    fn loop_stmt(&mut self, block_type: BlockType) -> &mut Self {
        self.inject(Operator::Loop {
            blockty: wasmparser::BlockType::from(block_type),
        });
        self
    }

    /// Inject an `if` instruction (`if` is a Rust keyword; use `if_stmt`).
    fn if_stmt(&mut self, block_type: BlockType) -> &mut Self {
        self.inject(Operator::If {
            blockty: wasmparser::BlockType::from(block_type),
        });
        self
    }

    /// Inject an `else` instruction (`else` is a Rust keyword; use `else_stmt`).
    fn else_stmt(&mut self) -> &mut Self {
        self.inject(Operator::Else);
        self
    }

    /// Inject a `return` instruction (`return` is a Rust keyword; use `return_stmt`).
    fn return_stmt(&mut self) -> &mut Self {
        self.inject(Operator::Return);
        self
    }

    // ── Type-conversion wrappers ──────────────────────────────────────────────
    // These accept more ergonomic Rust types and convert internally.

    /// Inject an `f32.const` instruction from a Rust `f32`.
    fn f32_const(&mut self, val: f32) -> &mut Self {
        self.inject(Operator::F32Const {
            value: wasmparser::Ieee32::from(val),
        });
        self
    }

    /// Inject an `f64.const` instruction from a Rust `f64`.
    fn f64_const(&mut self, val: f64) -> &mut Self {
        self.inject(Operator::F64Const {
            value: wasmparser::Ieee64::from(val),
        });
        self
    }

    /// Inject a `br_table` instruction.
    fn br_table(&mut self, targets: wasmparser::BrTable<'a>) -> &mut Self {
        self.inject(Operator::BrTable { targets });
        self
    }

    /// Inject a `ref.null` instruction using wirm's [`HeapType`].
    fn ref_null(&mut self, heap_type: HeapType) -> &mut Self {
        self.inject(Operator::RefNull {
            hty: wasmparser::HeapType::from(heap_type),
        });
        self
    }

    // ── GC: ref.test / ref.cast (HeapType conversions) ───────────────────────

    /// Inject a `ref.test` (non-null) instruction.
    fn ref_test(&mut self, heap_type: HeapType) -> &mut Self {
        self.inject(Operator::RefTestNonNull {
            hty: wasmparser::HeapType::from(heap_type),
        });
        self
    }

    /// Inject a `ref.test null` (nullable) instruction.
    fn ref_test_null(&mut self, heap_type: HeapType) -> &mut Self {
        self.inject(Operator::RefTestNullable {
            hty: wasmparser::HeapType::from(heap_type),
        });
        self
    }

    /// Inject a `ref.cast` (non-null) instruction.
    fn ref_cast(&mut self, heap_type: HeapType) -> &mut Self {
        self.inject(Operator::RefCastNonNull {
            hty: wasmparser::HeapType::from(heap_type),
        });
        self
    }

    /// Inject a `ref.cast null` (nullable) instruction.
    fn ref_cast_null(&mut self, heap_type: HeapType) -> &mut Self {
        self.inject(Operator::RefCastNullable {
            hty: wasmparser::HeapType::from(heap_type),
        });
        self
    }

    /// Inject a `ref.cast_desc` (non-null) instruction. Custom-descriptors proposal.
    fn ref_cast_desc(&mut self, heap_type: HeapType) -> &mut Self {
        self.inject(Operator::RefCastDescNonNull {
            hty: wasmparser::HeapType::from(heap_type),
        });
        self
    }

    /// Inject a `ref.cast_desc null` (nullable) instruction. Custom-descriptors proposal.
    fn ref_cast_desc_null(&mut self, heap_type: HeapType) -> &mut Self {
        self.inject(Operator::RefCastDescNullable {
            hty: wasmparser::HeapType::from(heap_type),
        });
        self
    }

    // ── Reference types: typed select ────────────────────────────────────────

    /// Inject a `select` instruction annotated with a single value type.
    fn typed_select(&mut self, ty: wasmparser::ValType) -> &mut Self {
        self.inject(Operator::TypedSelect { ty });
        self
    }

    /// Inject a `select` instruction annotated with multiple value types.
    fn typed_select_multi(&mut self, tys: Vec<wasmparser::ValType>) -> &mut Self {
        self.inject(Operator::TypedSelectMulti { tys });
        self
    }

    // ── GC: br_on_cast / br_on_cast_fail ─────────────────────────────────────

    /// Inject a `br_on_cast` instruction.
    fn br_on_cast(
        &mut self,
        relative_depth: u32,
        from_ref_type: wasmparser::RefType,
        to_ref_type: wasmparser::RefType,
    ) -> &mut Self {
        self.inject(Operator::BrOnCast {
            relative_depth,
            from_ref_type,
            to_ref_type,
        });
        self
    }

    /// Inject a `br_on_cast_fail` instruction.
    fn br_on_cast_fail(
        &mut self,
        relative_depth: u32,
        from_ref_type: wasmparser::RefType,
        to_ref_type: wasmparser::RefType,
    ) -> &mut Self {
        self.inject(Operator::BrOnCastFail {
            relative_depth,
            from_ref_type,
            to_ref_type,
        });
        self
    }

    /// Inject a `br_on_cast_desc` instruction. Custom-descriptors proposal.
    fn br_on_cast_desc(
        &mut self,
        relative_depth: u32,
        from_ref_type: wasmparser::RefType,
        to_ref_type: wasmparser::RefType,
    ) -> &mut Self {
        self.inject(Operator::BrOnCastDesc {
            relative_depth,
            from_ref_type,
            to_ref_type,
        });
        self
    }

    /// Inject a `br_on_cast_desc_fail` instruction. Custom-descriptors proposal.
    fn br_on_cast_desc_fail(
        &mut self,
        relative_depth: u32,
        from_ref_type: wasmparser::RefType,
        to_ref_type: wasmparser::RefType,
    ) -> &mut Self {
        self.inject(Operator::BrOnCastDescFail {
            relative_depth,
            from_ref_type,
            to_ref_type,
        });
        self
    }

    // ── Exceptions: try_table + legacy try ───────────────────────────────────

    /// Inject a `try_table` instruction with the given block type and catch clauses.
    fn try_table(&mut self, ty: BlockType, catches: Vec<wasmparser::Catch>) -> &mut Self {
        self.inject(Operator::TryTable {
            try_table: wasmparser::TryTable {
                ty: wasmparser::BlockType::from(ty),
                catches,
            },
        });
        self
    }

    /// Inject a legacy `try` instruction (`try` is a Rust keyword; use `try_stmt`).
    /// Superseded by [`try_table`](Self::try_table) in the exception-handling proposal.
    fn try_stmt(&mut self, block_type: BlockType) -> &mut Self {
        self.inject(Operator::Try {
            blockty: wasmparser::BlockType::from(block_type),
        });
        self
    }
}

#[allow(dead_code)]
/// Additional convenience injection methods built on top of [`Opcode`].
pub trait MacroOpcode<'a>: Inject<'a> {
    /// Reinterpret a `u32` as `i32` and inject an `i32.const`.
    ///
    /// Useful for emitting memory addresses without a cast at the call site.
    /// See <https://github.com/thesuhas/wirm/issues/133>.
    fn u32_const(&mut self, value: u32) -> &mut Self {
        let i32_val = value as i32;
        self.inject(Operator::I32Const { value: i32_val });
        self
    }

    /// Reinterpret a `u64` as `i64` and inject an `i64.const`.
    ///
    /// See <https://github.com/thesuhas/wirm/issues/133>.
    fn u64_const(&mut self, value: u64) -> &mut Self {
        let i64_val = value as i64;
        self.inject(Operator::I64Const { value: i64_val });
        self
    }
}
