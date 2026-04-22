//! Wirm is a WebAssembly Transformation Library for core modules and the
//! Component Model. The design has been inspired by [Dfinity's IC] and
//! [Walrus].
//!
//! Most users want one of two things:
//!
//! - **Instrument** — rewrite a wasm binary (inject counters, wrap calls,
//!   substitute instructions). See [Instrumenting] below.
//! - **Analyze** — walk a binary's structure and extract information
//!   without mutating it. See [Analyzing a component] below.
//!
//! # Two IRs
//!
//! Wirm has two top-level IR types. Pick the one that matches your input:
//!
//! - [`Module`] — a core WebAssembly module. Single function index space,
//!   no nested scopes. Use this for anything under `.wat` / `.wasm` that
//!   isn't wrapped in `(component …)`.
//! - [`Component`] — a full Component Model binary with nested subcomponents,
//!   aliases, imports/exports at every level, and multiple index spaces.
//!
//! Both types have the same high-level lifecycle:
//!
//! ```text
//!   bytes  ──parse──▶  IR  ──(optionally instrument)──▶  IR  ──encode──▶  bytes
//! ```
//!
//! # Instrumenting
//!
//! Instrumentation is "parse, mutate, re-encode". Mutation happens through
//! an *iterator* positioned at the current instruction, plus the
//! [`Opcode`] trait whose methods inject new instructions relative to that
//! position (`before`, `after`, `alternate`).
//!
//! ## Example: inject an `i32.const 1` before every `i32.add`
//!
//! ```rust,ignore
//! use wirm::{Module, Opcode};
//! use wirm::ir::types::Location;
//! use wirm::iterator::iterator_trait::{IteratingInstrumenter, Iterator};
//! use wirm::iterator::module_iterator::ModuleIterator;
//! use wirm::wasmparser::Operator;
//!
//! let bytes = std::fs::read("input.wasm").unwrap();
//! let mut module = Module::parse(&bytes, false, false).unwrap();
//!
//! {
//!     // Iterator walks function bodies one instruction at a time.
//!     let mut mod_it = ModuleIterator::new(&mut module, &vec![]);
//!     loop {
//!         if matches!(mod_it.curr_op(), Some(Operator::I32Add)) {
//!             // `.before()` switches the next injection to "before this op";
//!             // `Opcode` methods push instructions into that slot.
//!             mod_it.before().i32_const(1).drop();
//!         }
//!         if mod_it.next().is_none() { break; }
//!     }
//! }
//!
//! let out = module.encode().unwrap();
//! std::fs::write("output.wasm", out).unwrap();
//! ```
//!
//! The key methods on [`Opcode`] are:
//!
//! - Injection position: [`before`](crate::iterator::iterator_trait::IteratingInstrumenter::before) /
//!   [`after`](crate::iterator::iterator_trait::IteratingInstrumenter::after) /
//!   [`alternate`](crate::iterator::iterator_trait::IteratingInstrumenter::alternate) — each
//!   returns `&mut Self` so you can chain opcode pushes.
//! - One method per wasm instruction: `i32_const`, `i32_add`, `local_get`,
//!   `call`, `br_if`, …. Typed-ID arguments (`FunctionID`, `LocalID`,
//!   `TypeID`) keep the API semantic.
//! - Convenience extras via [`MacroOpcode`](crate::opcode::MacroOpcode) —
//!   e.g. `u32_const` / `u64_const`.
//!
//! ## Components work the same
//!
//! For a Component Model binary, parse with [`Component::parse`] and
//! iterate with
//! [`ComponentIterator`](crate::iterator::component_iterator::ComponentIterator).
//! The `Opcode` method chain is identical — the iterator walks every
//! function body across every core module inside the component.
//!
//! ## Adding component-level IR nodes
//!
//! Beyond injecting instructions into function bodies, you can also add
//! new *component-level* nodes — imports, canonical functions, aliases,
//! instance and function types, core instances — directly to the
//! component's IR:
//!
//! - [`Component::add_import`]
//! - [`Component::add_canon_func`]
//! - [`Component::add_alias_func`] / [`Component::add_alias_core_memory`]
//! - [`Component::add_type_instance`] / [`Component::add_type_func`]
//! - [`Component::add_core_instance`]
//!
//! Each returns a typed ID you can reference from subsequent instruction
//! injections. You don't need to order these calls with respect to other
//! component items: [`Component::encode`] drives
//! [`walk_topological`](crate::ir::component::visitor::walk_topological)
//! internally and re-emits everything in dependency order, so newly
//! inserted nodes appear in the output binary before anything that
//! references them.
//!
//! If you need to inject a node kind that isn't covered by the `add_*`
//! methods above, please [open an issue][issues] or a PR — the list is
//! grown as real consumers hit gaps.
//!
//! [issues]: https://github.com/composablesys/wirm/issues
//!
//! **Roadmap:** the long-term plan is to expose a *mutable* visitor so
//! component-level injections can happen at a specific point in the
//! walk — mirroring how `ModuleIterator` / `ComponentIterator` let you
//! inject instructions at a specific position in a function body. That
//! API isn't there yet; for now, the free-standing `add_*` methods on
//! [`Component`] are the way to add component-level IR nodes, and the
//! topological encode ensures the result is valid regardless of call
//! order.
//!
//! # Concretizing component types
//!
//! If your pass needs to *reason about* an import or export's type —
//! e.g. code-gen that targets a specific WIT interface — use
//! [`Component::concretize_import`] or [`Component::concretize_export`].
//! Both walk every alias chain, outer reference, and index lookup for
//! you and hand back a
//! [`ConcreteType`](crate::ir::component::concrete::ConcreteType)
//! with no remaining indices. For function types you get the named
//! params and result type (plus its `async`-ness); for instance types
//! you get named `(function_name, signature)` pairs plus type exports.
//!
//! ```rust,ignore
//! use wirm::Component;
//! use wirm::ir::component::concrete::ConcreteType;
//!
//! let bytes = std::fs::read("example.wasm").unwrap();
//! let comp = Component::parse(&bytes, false, false).unwrap();
//!
//! if let Some(ConcreteType::Instance { funcs, .. }) =
//!     comp.concretize_import("wasi:http/handler@0.3.0-draft")
//! {
//!     for (name, sig) in funcs {
//!         println!("{name}: async={}", sig.is_async);
//!         for (pname, pty) in &sig.params {
//!             println!("  param {pname}: {pty:?}");
//!         }
//!     }
//! }
//! ```
//!
//! The current implementation is scoped to what WIT interfaces emit
//! (functions + named type exports); non-function instance exports are
//! silently skipped. See [`concrete`](crate::ir::component::concrete)
//! for the full shape.
//!
//! # Analyzing a component
//!
//! If you're reading component structure rather than rewriting
//! instructions — e.g. "which types does this import depend on?" —
//! the visitor infrastructure gives you a scope-aware walk with
//! auto-resolved indices.
//!
//! Four pieces wire together:
//!
//! 1. A [`ComponentVisitor`](crate::ir::component::visitor::ComponentVisitor)
//!    impl — override only the callbacks you care about. See the trait
//!    docs for the full dispatch table (leaf `visit_*` vs bracketed
//!    `enter_* / exit_*`).
//! 2. [`VisitCtx::resolve`](crate::ir::component::visitor::VisitCtx::resolve)
//!    — resolves an [`IndexedRef`] through outer-alias chains and returns
//!    a [`ResolvedItem`] pointing at the real underlying IR node.
//! 3. [`ReferencedIndices`](crate::ir::component::refs::ReferencedIndices)
//!    (or a narrower `Get*Refs` trait) — pulls every ref out of an IR node
//!    without matching on every variant by hand.
//! 4. [`ResolvedItem::space`](crate::ir::component::visitor::ResolvedItem::space)
//!    / [`IndexSpaceOf`](crate::ir::component::idx_spaces::IndexSpaceOf) —
//!    maps a resolved item back to the namespace it lives in, even for
//!    ambiguous variants like `Alias` / `Import` / `Export` / `Func`.
//!
//! ## Example: which component types does each import reference?
//!
//! ```rust,ignore
//! use std::collections::HashSet;
//! use wirm::Component;
//! use wirm::ir::component::{
//!     idx_spaces::Space,
//!     refs::ReferencedIndices,
//!     visitor::{walk_structural, ComponentVisitor, ItemKind, VisitCtx},
//! };
//! use wirm::wasmparser::ComponentImport;
//!
//! #[derive(Default)]
//! struct TypesFromImports {
//!     found: HashSet<u32>,
//! }
//!
//! impl<'a> ComponentVisitor<'a> for TypesFromImports {
//!     fn visit_comp_import(
//!         &mut self,
//!         cx: &VisitCtx<'a>,
//!         _kind: ItemKind,
//!         _id: u32,
//!         import: &ComponentImport<'a>,
//!     ) {
//!         for refk in import.referenced_indices() {
//!             let resolved = cx.resolve(&refk.ref_);
//!             if resolved.space() == Space::CompType {
//!                 self.found.insert(resolved.idx());
//!             }
//!         }
//!     }
//! }
//!
//! let bytes = std::fs::read("example.wasm").unwrap();
//! let comp = Component::parse(&bytes, false, false).unwrap();
//! let mut finder = TypesFromImports::default();
//! walk_structural(&comp, &mut finder);
//! ```
//!
//! Notes:
//!
//! - [`walk_structural`](crate::ir::component::visitor::walk_structural)
//!   walks lexical order (use for analysis / pretty-printing).
//!   [`walk_topological`](crate::ir::component::visitor::walk_topological)
//!   walks dependency order with no forward references (use for re-encoding).
//! - `cx.resolve` makes outer-alias transparent — if you're inside a nested
//!   subcomponent and a ref has `depth > 0`, it walks up the scope chain
//!   for you.
//! - For transitive queries, feed the refs you collect into a worklist.
//!   `resolve` returns typed references (`&ComponentType`, `&CoreType`, …)
//!   that you can call `referenced_indices()` on directly.
//!
//! [Instrumenting]: #instrumenting
//! [Analyzing a component]: #analyzing-a-component
//! [`IndexedRef`]: crate::ir::component::refs::IndexedRef
//! [`ResolvedItem`]: crate::ir::component::visitor::ResolvedItem
//!
//! # Features
//!
//! ## Parallel Parsing and Encoding
//!
//! Wirm supports parallel processing during module parsing and encoding to improve performance. Enable the `parallel` feature
//! to parse and encode function bodies concurrently:
//!
//! ```toml
//! [dependencies]
//! wirm = { version = "1.1.0", features = ["parallel"] }
//! ```
//!
//! [Dfinity's IC]: https://github.com/dfinity/ic/tree/master/rs/wasm_transform
//! [Walrus]: https://github.com/rustwasm/walrus/tree/main

mod encode;
pub mod error;
pub mod ir;
pub mod iterator;
pub mod module_builder;
pub mod opcode;
pub mod subiterator;

pub use crate::opcode::Opcode;

pub use crate::ir::component::Component;
// pub use crate::ir::function::FunctionBuilder;
pub use crate::ir::module::Module;
pub use crate::ir::types::DataSegment;
pub use crate::ir::types::DataSegmentKind;
pub use crate::ir::types::DataType;
pub use crate::ir::types::InitInstr;
pub use crate::ir::types::Location;

// Re-export wasmparser so users can have the same types for e.g.
// `wasmparser::Operator` as we use internally.
pub use wasmparser;
