use crate::ir::component::idx_spaces::{IndexSpaceOf, Space};
use crate::ir::component::refs::{IndexedRef, RefKind};
use crate::ir::component::visitor::driver::{drive_event, VisitEvent};
use crate::ir::component::visitor::events_structural::get_structural_events;
use crate::ir::component::visitor::events_topological::get_topological_events;
use crate::ir::component::visitor::utils::VisitCtxInner;
use crate::ir::types::CustomSection;
use crate::{Component, Module};
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance,
    ComponentStartFunction, ComponentType, ComponentTypeDeclaration, CoreType, Instance,
    InstanceTypeDeclaration, ModuleTypeDeclaration, SubType,
};

pub(crate) mod driver;
mod events_structural;
pub(crate) mod events_topological;
#[cfg(test)]
mod resolution_tests;
#[cfg(test)]
mod tests;
pub(crate) mod utils;

/// Walk a [`Component`] using its *structural* (in-file) order.
///
/// This traversal visits items in the same order they appear in the
/// component’s section layout. Nested components are entered and exited
/// according to their lexical structure, and all visitor callbacks are
/// invoked in a manner consistent with the original encoding order.
///
/// # Semantics
///
/// - Preserves section order exactly as defined in the component.
/// - Nested components are visited depth-first.
/// - Scope management and ID resolution are handled internally.
/// - No reordering is performed to satisfy reference dependencies.
///
/// This is the most appropriate traversal for:
///
/// - Analysis passes
/// - Pretty-printing
/// - Validation-like inspections
/// - Any logic that expects lexical ordering
///
/// # Guarantees
///
/// - No forward-reference elimination is attempted.
/// - The visitor observes the same structural hierarchy as encoded.
/// - `enter_component` / `exit_component` callbacks are properly paired.
///
/// See also [`walk_topological`] for a dependency-ordered traversal.
pub fn walk_structural<'ir, V: ComponentVisitor<'ir>>(root: &'ir Component<'ir>, visitor: &mut V) {
    walk(get_structural_events, root, visitor);
}

/// Walk a [`Component`] in *topological* (dependency) order.
///
/// This traversal reorders items such that definitions are visited
/// before any items that reference them. The resulting visitation
/// order contains no forward references, making it suitable for
/// encoding or transforming components that require dependency-safe
/// emission.
///
/// # Semantics
///
/// - Items are visited in a dependency-respecting order.
/// - Nested components are still entered/exited with correct scope
///   management.
/// - All visitor callbacks observe valid, already-declared references.
/// - Structural layout order is **not** preserved.
///
/// # When to Use
///
/// This traversal is intended for:
///
/// - Component encoding
/// - Instrumentation passes
/// - Lowering or rewriting IR where forward references are illegal
/// - Any pass that requires reference-safe emission order
///
/// # Guarantees
///
/// - No visitor callback observes an unresolved forward reference.
/// - Scope handling and ID resolution remain logically consistent.
/// - `enter_component` / `exit_component` callbacks are properly paired.
///
/// See also [`walk_structural`] for lexical-order traversal.
pub fn walk_topological<'ir, V: ComponentVisitor<'ir>>(root: &'ir Component<'ir>, visitor: &mut V) {
    walk(get_topological_events, root, visitor);
}

fn walk<'ir, V: ComponentVisitor<'ir>>(
    get_evts: fn(&'ir Component<'ir>, &mut VisitCtx<'ir>, &mut Vec<VisitEvent<'ir>>),
    root: &'ir Component<'ir>,
    visitor: &mut V,
) {
    let mut ctx = VisitCtx::new(root);
    let mut events = Vec::new();
    get_evts(root, &mut ctx, &mut events);

    for event in events.iter() {
        drive_event(event, visitor, &mut ctx);
    }
}

/// A structured, read-only visitor over a [`Component`] tree.
///
/// All methods have default no-op implementations. Override only the
/// callbacks relevant to your use case.
///
/// # Guarantees
///
/// - `enter_component` and `exit_component` are always properly paired.
/// - Nested components are visited in a well-structured manner.
/// - IDs are resolved and stable within a single traversal.
///
/// # ID Semantics
///
/// - `id: None` is used only for the root component.
/// - All other items receive a resolved `u32` ID corresponding to their
///   index within the appropriate namespace at that depth.
/// - For items that may belong to multiple namespaces (e.g. imports,
///   exports, aliases, canonical functions), the `ItemKind` parameter
///   indicates the resolved kind of the item.
///
/// # Mutation
///
/// This visitor is strictly read-only. Implementations must not mutate
/// the underlying component structure.
pub trait ComponentVisitor<'a> {
    /// Invoked when entering the outermost, root component to enable special handling.
    ///
    /// This is the earliest hook available for a component.
    fn enter_root_component(&mut self, _cx: &VisitCtx<'a>, _component: &Component<'a>) {}
    /// Invoked after all items within the root component have been visited.
    ///
    /// Always paired with a prior `enter_root_component` call.
    fn exit_root_component(&mut self, _cx: &VisitCtx<'a>, _component: &Component<'a>) {}
    /// Invoked when entering a subcomponent within the root.
    ///
    /// The `id` corresponds to the resolved component index within the
    /// current namespace. This callback is paired with `exit_component`
    /// once traversal of the component’s body has completed.
    fn enter_component(&mut self, _cx: &VisitCtx<'a>, _id: u32, _component: &Component<'a>) {}
    /// Invoked after all items within a subcomponent have been visited.
    ///
    /// Always paired with a prior `enter_component` call.
    fn exit_component(&mut self, _cx: &VisitCtx<'a>, _id: u32, _component: &Component<'a>) {}
    /// Invoked for each core WebAssembly module defined in the component.
    ///
    /// The `id` corresponds to the module’s resolved index within the
    /// current core module namespace.
    fn visit_module(&mut self, _cx: &VisitCtx<'a>, _id: u32, _module: &Module<'a>) {}

    // ------------------------
    // Component-level items
    // ------------------------

    /// Invoked for each leaf component type (`Defined`, `Func`, `Resource`).
    ///
    /// The `id` corresponds to the resolved type index within the
    /// component type namespace.
    ///
    /// For `Instance` and `Component` types, which carry inner declaration
    /// bodies, use [`ComponentVisitor::enter_component_type_inst`] /
    /// [`ComponentVisitor::enter_component_type_comp`] instead.
    fn visit_comp_type(&mut self, _cx: &VisitCtx<'a>, _id: u32, _comp_type: &ComponentType<'a>) {}

    /// Invoked when entering an instance type (`ComponentType::Instance`).
    ///
    /// The `id` is the resolved type index.  Instance type declarations
    /// are reported via [`ComponentVisitor::visit_inst_type_decl`] between
    /// this call and [`ComponentVisitor::exit_component_type_inst`].
    fn enter_component_type_inst(
        &mut self,
        _cx: &VisitCtx<'a>,
        _id: u32,
        _comp_type: &ComponentType<'a>,
    ) {
    }

    /// Invoked after all declarations within an instance type have been
    /// visited.  Always paired with
    /// [`ComponentVisitor::enter_component_type_inst`].
    fn exit_component_type_inst(
        &mut self,
        _cx: &VisitCtx<'a>,
        _id: u32,
        _comp_type: &ComponentType<'a>,
    ) {
    }

    /// Invoked when entering a component type body
    /// (`ComponentType::Component`).
    ///
    /// The `id` is the resolved type index.  Component type declarations
    /// are reported via [`ComponentVisitor::visit_comp_type_decl`] between
    /// this call and [`ComponentVisitor::exit_component_type_comp`].
    fn enter_component_type_comp(
        &mut self,
        _cx: &VisitCtx<'a>,
        _id: u32,
        _comp_type: &ComponentType<'a>,
    ) {
    }

    /// Invoked after all declarations within a component type body have
    /// been visited.  Always paired with
    /// [`ComponentVisitor::enter_component_type_comp`].
    fn exit_component_type_comp(
        &mut self,
        _cx: &VisitCtx<'a>,
        _id: u32,
        _comp_type: &ComponentType<'a>,
    ) {
    }

    /// Invoked for each declaration within a `ComponentType::Component`.
    ///
    /// The `decl_idx` is the index of this declaration within the parent
    /// component type’s declaration list. The `parent` is the enclosing
    /// `ComponentType`, and `decl` is the specific declaration.
    ///
    /// These callbacks are emitted between `enter_comp_type` and
    /// `exit_comp_type` for the enclosing type.
    fn visit_comp_type_decl(
        &mut self,
        _cx: &VisitCtx<'a>,
        _decl_idx: usize,
        _id: u32,
        _parent: &ComponentType<'a>,
        _decl: &ComponentTypeDeclaration<'a>,
    ) {
    }

    /// Invoked for each declaration within a `ComponentType::Instance`.
    ///
    /// The `decl_idx` is the index of this declaration within the parent
    /// instance type’s declaration list. The `parent` is the enclosing
    /// `ComponentType`, and `decl` is the specific instance type
    /// declaration.
    ///
    /// These callbacks are emitted between `enter_comp_type` and
    /// `exit_comp_type` for the enclosing type.
    fn visit_inst_type_decl(
        &mut self,
        _cx: &VisitCtx<'a>,
        _decl_idx: usize,
        _id: u32,
        _parent: &ComponentType<'a>,
        _decl: &InstanceTypeDeclaration<'a>,
    ) {
    }

    /// Invoked for each component instance.
    ///
    /// The `id` corresponds to the resolved instance index within the
    /// component instance namespace.
    fn visit_comp_instance(
        &mut self,
        _cx: &VisitCtx<'a>,
        _id: u32,
        _instance: &ComponentInstance<'a>,
    ) {
    }

    // ------------------------------------------------
    // Items with multiple possible resolved namespaces
    // ------------------------------------------------

    /// Invoked for canonical functions.
    ///
    /// The `kind` parameter indicates the resolved namespace of this item
    /// (e.g. component function vs. core function).
    ///
    /// The `id` is the resolved index within the namespace identified
    /// by `kind`.
    fn visit_canon(
        &mut self,
        _cx: &VisitCtx<'a>,
        _kind: ItemKind,
        _id: u32,
        _canon: &CanonicalFunction,
    ) {
    }

    /// Invoked for component aliases.
    ///
    /// The `kind` parameter indicates the resolved target namespace
    /// referenced by the alias.
    ///
    /// The `id` is the resolved index of the alias within its namespace.
    fn visit_alias(
        &mut self,
        _cx: &VisitCtx<'a>,
        _kind: ItemKind,
        _id: u32,
        _alias: &ComponentAlias<'a>,
    ) {
    }

    /// Invoked for component imports.
    ///
    /// The `kind` parameter identifies the imported item category
    /// (e.g. type, function, instance).
    ///
    /// The `id` is the resolved index assigned to the imported item
    /// within the corresponding namespace.
    fn visit_comp_import(
        &mut self,
        _cx: &VisitCtx<'a>,
        _kind: ItemKind,
        _id: u32,
        _import: &ComponentImport<'a>,
    ) {
    }

    /// Invoked for component exports.
    ///
    /// The `kind` parameter identifies the exported item category.
    ///
    /// The `id` is the resolved index of the exported item within the
    /// corresponding namespace.
    fn visit_comp_export(
        &mut self,
        _cx: &VisitCtx<'a>,
        _kind: ItemKind,
        _id: u32,
        _export: &ComponentExport<'a>,
    ) {
    }

    // ============================================================
    // Core Recursion Groups (`core rec`)
    // ============================================================

    /// Called when entering a core recursion group (`core rec`).
    ///
    /// A recursion group defines one or more mutually recursive core
    /// subtypes that are allocated as a unit in the core type index
    /// space. All subtypes belonging to this group will be reported
    /// via subsequent `visit_core_subtype` calls, followed by a single
    /// `exit_core_rec_group`.
    ///
    /// Parameters:
    /// - `count`: The total number of subtypes in this recursion group.
    /// - `core_type`: The enclosing `CoreType` that owns this group.
    ///
    /// Ordering guarantees:
    /// - Exactly `count` calls to `visit_core_subtype` will occur
    ///   before `exit_core_rec_group` is invoked.
    /// - No other recursion group callbacks will be interleaved.
    ///
    /// Indexing semantics:
    /// - Each subtype reported within this group corresponds to a
    ///   consecutive allocation in the core type index space.
    fn enter_core_rec_group(
        &mut self,
        _cx: &VisitCtx<'a>,
        _count: usize,
        _core_type: &CoreType<'a>,
    ) {
    }

    /// Called for each subtype within the current recursion group.
    ///
    /// This callback is emitted between `enter_core_rec_group` and
    /// `exit_core_rec_group`.
    ///
    /// Parameters:
    /// - `id`: The resolved core type index assigned to this subtype.
    ///   These indices are contiguous within the enclosing recursion group.
    /// - `subtype`: The subtype definition, including finality,
    ///   supertype information, and its composite type.
    ///
    /// Invariants:
    /// - This is only invoked while a recursion group is active.
    /// - The `id` is stable and corresponds to the canonical core
    ///   type namespace for the enclosing module.
    fn visit_core_subtype(&mut self, _cx: &VisitCtx<'a>, _id: u32, _subtype: &SubType) {}

    /// Called after all subtypes in the current recursion group
    /// have been reported.
    ///
    /// Always paired with a prior `enter_core_rec_group`. No additional
    /// `visit_core_subtype` calls will occur after this callback.
    ///
    /// At this point, the full set of types in the group is known and
    /// may be finalized or encoded as a unit.
    fn exit_core_rec_group(&mut self, _cx: &VisitCtx<'a>) {}

    // ============================================================
    // Core Type Definitions
    // ============================================================

    /// Called when entering a core module type (`CoreType::Module`).
    ///
    /// The `id` is the resolved index in the core type namespace.
    /// Module type declarations are reported via
    /// [`ComponentVisitor::visit_module_type_decl`] between this call and
    /// [`ComponentVisitor::exit_core_module_type`].
    fn enter_core_module_type(&mut self, _cx: &VisitCtx<'a>, _id: u32, _core_type: &CoreType<'a>) {}

    /// Called for each declaration inside a core module type.
    ///
    /// Emitted only while visiting a core type whose underlying
    /// definition is a module type.
    ///
    /// Parameters:
    /// - `decl_idx`: The declaration’s ordinal position within the
    ///   parent module type.
    /// - `id`: The resolved core type index of the enclosing type.
    /// - `parent`: The enclosing `CoreType`.
    /// - `decl`: The specific module type declaration.
    ///
    /// Ordering guarantees:
    /// - These callbacks occur strictly between `enter_core_type`
    ///   and `exit_core_type` for the same `id`.
    /// - Declarations are visited in source order.
    ///
    /// Indexing semantics:
    /// - `decl_idx` is local to the parent type and does not refer
    ///   to a global index space.
    fn visit_module_type_decl(
        &mut self,
        _cx: &VisitCtx<'a>,
        _decl_idx: usize,
        _id: u32,
        _parent: &CoreType<'a>,
        _decl: &ModuleTypeDeclaration<'a>,
    ) {
    }

    /// Called after all declarations within a core module type have been
    /// visited.  Always paired with
    /// [`ComponentVisitor::enter_core_module_type`].
    fn exit_core_module_type(&mut self, _cx: &VisitCtx<'a>, _id: u32, _core_type: &CoreType<'a>) {}

    /// Invoked for each core WebAssembly instance.
    ///
    /// The `id` corresponds to the resolved instance index within the
    /// core instance namespace.
    fn visit_core_instance(&mut self, _cx: &VisitCtx<'a>, _id: u32, _inst: &Instance<'a>) {}

    // ------------------------
    // Sections
    // ------------------------

    /// Invoked for each custom section encountered during traversal.
    ///
    /// Custom sections are visited in traversal order and are not
    /// associated with structured enter/exit pairing.
    fn visit_custom_section(&mut self, _cx: &VisitCtx<'a>, _sect: &CustomSection<'a>) {}

    /// Invoked if the component defines a start function.
    ///
    /// This callback is emitted at the point in traversal where the
    /// start section appears.
    fn visit_start_section(&mut self, _cx: &VisitCtx<'a>, _start: &ComponentStartFunction) {}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemKind {
    Comp,
    CompFunc,
    CompVal,
    CompType,
    CompInst,
    CoreInst,
    CoreModule,
    CoreType,
    CoreFunc,
    CoreMemory,
    CoreTable,
    CoreGlobal,
    CoreTag,
    NA,
}
impl From<Space> for ItemKind {
    fn from(space: Space) -> Self {
        match space {
            Space::Comp => Self::Comp,
            Space::CompFunc => Self::CompFunc,
            Space::CompVal => Self::CompVal,
            Space::CompType => Self::CompType,
            Space::CompInst => Self::CompInst,
            Space::CoreInst => Self::CoreInst,
            Space::CoreModule => Self::CoreModule,
            Space::CoreType => Self::CoreType,
            Space::CoreFunc => Self::CoreFunc,
            Space::CoreMemory => Self::CoreMemory,
            Space::CoreTable => Self::CoreTable,
            Space::CoreGlobal => Self::CoreGlobal,
            Space::CoreTag => Self::CoreTag,
            Space::NA => Self::NA,
        }
    }
}

/// Context provided during component traversal.
///
/// `VisitCtx` allows resolution of referenced indices (such as type,
/// function, instance, or module indices) relative to the current
/// traversal position.
///
/// The context:
///
/// - Tracks nested component boundaries
/// - Tracks nested index scopes
/// - Correctly resolves `(outer ...)` references
/// - Resolves references across component and core index spaces
///
/// This type is opaque and cannot be constructed by users. It is only
/// available during traversal via [`walk_topological`] or [`walk_structural`].
///
/// All resolution operations are read-only and reflect the *semantic*
/// structure of the component, not its internal storage layout.
#[derive(Clone)]
pub struct VisitCtx<'a> {
    pub(crate) inner: VisitCtxInner<'a>,
}
impl<'a> VisitCtx<'a> {
    /// Returns the component currently being visited.
    pub fn curr_component(&self) -> &Component<'_> {
        self.inner.curr_component()
    }

    /// Top-level section ordinal of the event currently being driven, relative to the
    /// immediately-containing component (i.e. the index of the section in
    /// `component.sections`). Returns `None` while inside the root-component
    /// enter/exit callbacks, which are not associated with any section.
    ///
    /// This is most useful for visitors that need to correlate IR items back to their
    /// originating binary section — for example, deciding which raw section bytes to
    /// copy when filtering or rewriting a component.
    pub fn curr_section_idx(&self) -> Option<usize> {
        self.inner.current_section_idx
    }

    pub(crate) fn new(component: &'a Component<'a>) -> Self {
        Self {
            inner: VisitCtxInner::new(component),
        }
    }
    /// Resolves a single [`IndexedRef`] into a fully resolved semantic item.
    ///
    /// This applies:
    ///
    /// - Depth resolution (`outer` / nested scopes)
    /// - Index space resolution
    /// - Component vs core namespace resolution
    ///
    /// The returned [`ResolvedItem`] represents the semantic target
    /// referenced by the index.
    ///
    /// To pull such references from an IR node, use one of the following traits
    /// (only the applicable traits have been defined per node):
    /// - [`crate::ir::component::refs::ReferencedIndices`]: to pull ALL refs
    /// - [`crate::ir::component::refs::GetCompRefs`]: to pull component refs
    /// - [`crate::ir::component::refs::GetModuleRefs`]: to pull module refs
    /// - [`crate::ir::component::refs::GetTypeRefs`]: to pull type refs
    /// - [`crate::ir::component::refs::GetFuncRefs`]: to pull func refs
    /// - [`crate::ir::component::refs::GetFuncRef`]: if a node only has a single func ref
    /// - [`crate::ir::component::refs::GetMemRefs`]: to pull memory refs
    /// - [`crate::ir::component::refs::GetTableRefs`]: to pull table refs
    /// - [`crate::ir::component::refs::GetItemRefs`]: to pull refs to items
    /// - [`crate::ir::component::refs::GetItemRef`]: if a node only has a single item ref
    /// - [`crate::ir::component::refs::GetParamRefs`]: to pull refs of parameters
    /// - [`crate::ir::component::refs::GetResultRefs`]: to pull refs of results
    /// - [`crate::ir::component::refs::GetArgRefs`]: to pull refs of args
    /// - [`crate::ir::component::refs::GetDescriptorRefs`]: to pull refs of descriptors
    /// - [`crate::ir::component::refs::GetDescribesRefs`]: to pull refs of describes
    pub fn resolve(&self, ref_: &IndexedRef) -> ResolvedItem<'a, 'a> {
        self.inner.resolve(ref_)
    }

    /// Resolves a collection of [`RefKind`] values into their semantic targets.
    ///
    /// This is a convenience helper for bulk resolution when a node exposes
    /// multiple referenced indices.
    ///
    /// Read through [`VisitCtx::resolve`] for how to pull such references from IR nodes.
    pub fn resolve_all(&self, refs: &[RefKind]) -> Vec<ResolvedItem<'a, 'a>> {
        self.inner.resolve_all(refs)
    }
    /// Looks up the name (if any) of the root component.
    ///
    /// Returns `None` if the component has no name.
    pub fn lookup_root_comp_name(&self) -> Option<&str> {
        self.inner.lookup_root_comp_name()
    }
    /// Looks up the name (if any) of a component by its ID.
    ///
    /// Returns `None` if:
    /// - The component has no name
    /// - The ID is not valid in the current context
    pub fn lookup_comp_name(&self, id: u32) -> Option<&str> {
        self.inner.lookup_comp_name(id)
    }
    /// Looks up the name (if any) of a component instance by its ID.
    ///
    /// Returns `None` if:
    /// - The instance has no name
    /// - The ID is not valid in the current context
    pub fn lookup_comp_inst_name(&self, id: u32) -> Option<&str> {
        self.inner.lookup_comp_inst_name(id)
    }
    /// Looks up the name (if any) of a component type by its ID.
    ///
    /// Returns `None` if:
    /// - The type has no name
    /// - The ID is not valid in the current context
    pub fn lookup_comp_type_name(&self, id: u32) -> Option<&str> {
        self.inner.lookup_comp_type_name(id)
    }
    /// Looks up the name (if any) of a component func by its ID.
    ///
    /// Returns `None` if:
    /// - The func has no name
    /// - The ID is not valid in the current context
    pub fn lookup_comp_func_name(&self, id: u32) -> Option<&str> {
        self.inner.lookup_comp_func_name(id)
    }
    /// Looks up the name (if any) of a module by its ID.
    ///
    /// Returns `None` if:
    /// - The module has no name
    /// - The ID is not valid in the current context
    pub fn lookup_module_name(&self, id: u32) -> Option<&str> {
        self.inner.lookup_module_name(id)
    }
    /// Looks up the name (if any) of a core instance by its ID.
    ///
    /// Returns `None` if:
    /// - The instance has no name
    /// - The ID is not valid in the current context
    pub fn lookup_core_inst_name(&self, id: u32) -> Option<&str> {
        self.inner.lookup_core_inst_name(id)
    }
    /// Looks up the name (if any) of a core type by its ID.
    ///
    /// Returns `None` if:
    /// - The type has no name
    /// - The ID is not valid in the current context
    pub fn lookup_core_type_name(&self, id: u32) -> Option<&str> {
        self.inner.lookup_core_type_name(id)
    }
    /// Looks up the name (if any) of a core function by its ID.
    ///
    /// Returns `None` if:
    /// - The function has no name
    /// - The ID is not valid in the current context
    pub fn lookup_core_func_name(&self, id: u32) -> Option<&str> {
        self.inner.lookup_core_func_name(id)
    }
    /// Looks up the name (if any) of a global by its ID.
    ///
    /// Returns `None` if:
    /// - The global has no name
    /// - The ID is not valid in the current context
    pub fn lookup_global_name(&self, id: u32) -> Option<&str> {
        self.inner.lookup_global_name(id)
    }
    /// Looks up the name (if any) of a memory by its ID.
    ///
    /// Returns `None` if:
    /// - The memory has no name
    /// - The ID is not valid in the current context
    pub fn lookup_memory_name(&self, id: u32) -> Option<&str> {
        self.inner.lookup_memory_name(id)
    }
    /// Looks up the name (if any) of a tag by its ID.
    ///
    /// Returns `None` if:
    /// - The tag has no name
    /// - The ID is not valid in the current context
    pub fn lookup_tag_name(&self, id: u32) -> Option<&str> {
        self.inner.lookup_tag_name(id)
    }
    /// Looks up the name (if any) of a table by its ID.
    ///
    /// Returns `None` if:
    /// - The table has no name
    /// - The ID is not valid in the current context
    pub fn lookup_table_name(&self, id: u32) -> Option<&str> {
        self.inner.lookup_table_name(id)
    }
    /// Looks up the name (if any) of a value by its ID.
    ///
    /// Returns `None` if:
    /// - The value has no name
    /// - The ID is not valid in the current context
    pub fn lookup_value_name(&self, id: u32) -> Option<&str> {
        self.inner.lookup_value_name(id)
    }
}

/// A resolved component item.
///
/// This represents the semantic target of a reference after index
/// resolution has been performed.
///
/// Each variant contains:
///
/// - A `u32` representing the **resolved index of the item within its
///   corresponding namespace**, and
/// - A reference to the underlying IR node.
///
/// The `u32` is *not* a syntactic index from the binary. Instead, it is
/// the canonical, namespace-specific ID assigned during resolution. For
/// example, a component type's `u32` is its resolved index in the
/// component type namespace, and a core instance's `u32` is its resolved
/// index in the core instance namespace.
///
/// This enum allows callers to uniformly handle any reference target
/// without needing to separately track both namespace and ID.
///
/// # Reading the index and namespace uniformly
///
/// Some variants (e.g. [`ResolvedItem::CompType`]) imply a single
/// namespace from the variant tag alone. Others ([`ResolvedItem::Alias`],
/// [`ResolvedItem::Import`], [`ResolvedItem::Export`],
/// [`ResolvedItem::Func`], and the `*TyDecl*` body-local variants) wrap
/// items that may belong to *any* of several namespaces — the index lives
/// in the namespace of the originating ref, not in some "alias space" or
/// "import space".
///
/// To avoid forcing consumers to remember which variants are which,
/// [`ResolvedItem::idx`] and [`ResolvedItem::space`] dispatch internally
/// (using each item's [`IndexSpaceOf`] impl for the ambiguous variants)
/// and return the canonical (space, idx) pair regardless of which variant
/// you got back.
///
/// # Invariant
///
/// The `u32` stored in each variant **must** equal the resolved index in
/// the namespace returned by [`ResolvedItem::space`]. For example,
/// `ResolvedItem::CompType(idx, _)` must always have `idx` equal to the
/// resolved index in [`Space::CompType`].
#[derive(Clone, Debug)]
pub enum ResolvedItem<'a, 'b> {
    /// A resolved subcomponent. Always in [`Space::Comp`].
    Component(u32, &'a Component<'b>),

    /// A resolved core WebAssembly module. Always in [`Space::CoreModule`].
    Module(u32, &'a Module<'b>),

    /// A resolved canonical function. Index is in [`Space::CompFunc`] or
    /// [`Space::CoreFunc`] depending on the function variant — see the
    /// `CanonicalFunction` [`IndexSpaceOf`] impl.
    Func(u32, &'a CanonicalFunction),

    /// A resolved component-level type. Always in [`Space::CompType`].
    CompType(u32, &'a ComponentType<'b>),

    /// A resolved component instance. Always in [`Space::CompInst`].
    CompInst(u32, &'a ComponentInstance<'b>),

    /// A resolved core WebAssembly instance. Always in [`Space::CoreInst`].
    CoreInst(u32, &'a Instance<'b>),

    /// A resolved core WebAssembly type. Always in [`Space::CoreType`].
    CoreType(u32, &'a CoreType<'b>),

    /// A resolved component alias. Aliases produce items in whichever
    /// namespace the originating ref was in (CompType, CompInst, CompFunc,
    /// …); use [`ResolvedItem::space`] to get the namespace.
    Alias(u32, &'a ComponentAlias<'b>),

    /// A resolved component import. Same disambiguation rules as
    /// [`ResolvedItem::Alias`].
    Import(u32, &'a ComponentImport<'b>),

    /// A resolved component export. Same disambiguation rules as
    /// [`ResolvedItem::Alias`].
    Export(u32, &'a ComponentExport<'b>),
    /// A resolved declaration from inside a [`ComponentType::Component`] body.
    /// The index is in the parent body's namespace; use
    /// [`ResolvedItem::space`] to get it.
    CompTyDeclExport(u32, &'a ComponentTypeDeclaration<'b>),
    /// A resolved declaration from inside a [`ComponentType::Instance`] body.
    /// The index is in the parent body's namespace; use
    /// [`ResolvedItem::space`] to get it.
    InstTyDeclExport(u32, &'a InstanceTypeDeclaration<'b>),
    /// A resolved declaration from inside a [`CoreType::Module`] body.
    /// The index is in the parent body's namespace; use
    /// [`ResolvedItem::space`] to get it.
    ModuleTyDecl(u32, &'a ModuleTypeDeclaration<'b>),
}

impl<'a, 'b> ResolvedItem<'a, 'b> {
    /// The resolved index of this item, in the namespace returned by
    /// [`ResolvedItem::space`]. Returning the index this way means
    /// consumers don't have to pattern-match on the variant just to extract
    /// the index — relevant when you've already dispatched on `space` to
    /// pick the right namespace-specific lookup.
    pub fn idx(&self) -> u32 {
        match self {
            ResolvedItem::Component(i, _)
            | ResolvedItem::Module(i, _)
            | ResolvedItem::Func(i, _)
            | ResolvedItem::CompType(i, _)
            | ResolvedItem::CompInst(i, _)
            | ResolvedItem::CoreInst(i, _)
            | ResolvedItem::CoreType(i, _)
            | ResolvedItem::Alias(i, _)
            | ResolvedItem::Import(i, _)
            | ResolvedItem::Export(i, _)
            | ResolvedItem::CompTyDeclExport(i, _)
            | ResolvedItem::InstTyDeclExport(i, _)
            | ResolvedItem::ModuleTyDecl(i, _) => *i,
        }
    }

    /// The namespace this resolved item lives in.
    ///
    /// For variants whose tag implies a single namespace (Component,
    /// Module, CompType, CompInst, CoreInst, CoreType) the implied space
    /// is returned. For ambiguous variants — `Alias`, `Import`, `Export`,
    /// `Func`, and the body-local `*TyDecl*` variants — this dispatches to
    /// the underlying IR node's [`IndexSpaceOf`] impl, which examines the
    /// node's actual kind to derive the correct namespace (e.g. an alias
    /// of `kind: Type` returns [`Space::CompType`]).
    pub fn space(&self) -> Space {
        match self {
            ResolvedItem::Component(_, _) => Space::Comp,
            ResolvedItem::Module(_, _) => Space::CoreModule,
            ResolvedItem::CompType(_, _) => Space::CompType,
            ResolvedItem::CompInst(_, _) => Space::CompInst,
            ResolvedItem::CoreInst(_, _) => Space::CoreInst,
            ResolvedItem::CoreType(_, _) => Space::CoreType,
            ResolvedItem::Func(_, func) => func.index_space_of(),
            ResolvedItem::Alias(_, alias) => alias.index_space_of(),
            ResolvedItem::Import(_, import) => import.index_space_of(),
            ResolvedItem::Export(_, export) => export.index_space_of(),
            ResolvedItem::CompTyDeclExport(_, decl) => decl.index_space_of(),
            ResolvedItem::InstTyDeclExport(_, decl) => decl.index_space_of(),
            ResolvedItem::ModuleTyDecl(_, decl) => decl.index_space_of(),
        }
    }
}
