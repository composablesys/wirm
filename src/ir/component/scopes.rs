//! ## Scope Tracking and Stable Identity
//!
//! This module defines the infrastructure used to safely track **nested index
//! spaces** across parsing, instrumentation, and encoding phases of a
//! WebAssembly *component*.
//!
//! WebAssembly components introduce hierarchical index scopes: components may
//! contain subcomponents, instances, types, and other constructs that form
//! their own index spaces. Additionally, `(outer ...)` references allow inner
//! scopes to refer to indices defined in enclosing scopes. Correctly resolving
//! these relationships at encode time therefore requires an explicit model of
//! scope nesting rather than a single flat index map.
//!
//! At the same time, this crate supports **component instrumentation**, meaning
//! the IR may be visited, transformed, and encoded in an order that does *not*
//! correspond to the original parse order. As a result, index resolution cannot
//! rely on traversal order alone.
//!
//! To address these constraints, this module separates **identity** from
//! **ownership** using a central registry and a small set of carefully enforced
//! invariants.
//!
//! ---
//!
//! ### `ScopeRegistry`
//!
//! `ScopeRegistry` is a shared registry that maps *IR node identity* to the index
//! scope (`SpaceId`) that the node owns or inhabits. This mapping is established
//! during parsing and maintained throughout the lifetime of the IR.
//!
//! The registry supports **two identity mechanisms**, depending on the kind of
//! node being tracked:
//!
//! #### Component scopes (special-cased)
//!
//! Components are identified by a stable `ComponentId`, assigned when the
//! component is parsed or created. Component scopes are registered and looked
//! up **by `ComponentId`**, rather than by pointer.
//!
//! This reflects the fact that components:
//! - May be stored in a central registry
//! - Are visited via an explicit *component ID stack* during traversal
//! - Do not rely on memory address stability for identity
//!
//! By using `ComponentId` as the identity key, component scope lookup remains
//! robust even as components are nested, traversed out of order, or referenced
//! indirectly.
//!
//! #### All other scoped IR nodes
//!
//! All non-component nodes that introduce or inhabit scopes (e.g. component types,
//! core types, etc.) are tracked using **raw pointers**
//! (`*const T`) as identity keys.
//!
//! These nodes are stored in append-only, stable allocations (`Box<T>`
//! inside append-only vectors), ensuring that their addresses remain
//! valid for the lifetime of the component graph.
//!
//! Raw pointers are used **only for identity comparison**; they are never
//! dereferenced.
//!
//! ---
//!
//! ### Scope Resolution During Encoding
//!
//! During encoding, scopes are resolved dynamically using two stacks:
//!
//! - A **component ID stack**, tracking which component is currently being
//!   traversed
//! - A **scope stack**, tracking nested index spaces within that component
//!
//! When an IR node needs to resolve its associated scope:
//!
//! - If the node is a component, the current `ComponentId` is used to query the
//!   registry
//! - Otherwise, the node’s pointer identity is used to retrieve its `SpaceId`
//!
//! This design allows correct resolution of arbitrarily nested constructs such
//! as deeply nested components, instances, and `(outer ...)` references without
//! encoding traversal order into the registry itself.
//!
//! ---
//!
//! ### Safety and Invariants
//!
//! This design relies on the following invariants:
//!
//! - Each component is assigned a unique `ComponentId` that remains stable for
//!   its lifetime.
//! - All non-component IR nodes that participate in scoping are allocated in
//!   stable memory (e.g. boxed and stored in append-only vectors).
//! - IR nodes are never moved or removed after registration with the
//!   `ScopeRegistry`.
//! - `ScopeRegistry` entries are created during parsing and may be extended
//!   during instrumentation, but are never removed.
//! - Raw pointer usage is confined strictly to identity comparison; no pointer
//!   is ever dereferenced.
//!
//! These constraints allow the system to use low-level identity mechanisms in a
//! controlled, domain-specific way while preserving correctness and debuggability.
//!
//! ---
//!
//! ### Design Tradeoffs
//!
//! This approach deliberately favors:
//!
//! - Explicit scope modeling over implicit traversal order
//! - Stable identity over borrow-driven lifetimes
//! - Append-only IR construction over in-place mutation
//!
//! While this introduces some bookkeeping and indirection, it ensures that index
//! correctness is enforced structurally and remains robust in the presence of
//! instrumentation, reordering, and future extensions to the component model.
//!
//! In short: **index correctness is enforced structurally, not procedurally**.
//!
//! ## Why `ScopeOwnerKind` Exists
//!
//! In the IR, multiple wrapper structs may reference the same underlying
//! scoped node. For example, a user-facing struct might contain a field
//! pointing to a `CoreType` that is also stored directly in a component's
//! internal vectors. Without additional tracking, the scope resolution logic
//! would see two references to the same pointer and mistakenly treat them as
//! separate scopes.
//!
//! `ScopeOwnerKind` is used to **disambiguate these cases**. Each node in the
//! scope registry records whether it is:
//! - An **original owner** of the scope (the canonical IR node), or
//! - A **derived/alias** that references an existing scope
//!
//! This ensures that the same scope is **never entered twice**, preventing
//! double-counting or incorrect index resolution during encoding.

use crate::ir::component::idx_spaces::ScopeId;
use crate::ir::id::CompUniqueId;
use crate::ir::types::CustomSection;
use crate::{Component, Module};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::rc::Rc;
use wasmparser::{
    CanonicalFunction, CanonicalOption, ComponentAlias, ComponentDefinedType, ComponentExport,
    ComponentFuncType, ComponentImport, ComponentInstance, ComponentInstantiationArg,
    ComponentStartFunction, ComponentType, ComponentTypeDeclaration, ComponentTypeRef,
    ComponentValType, CompositeInnerType, CompositeType, CoreType, Export, FieldType, FuncType,
    Import, Instance, InstanceTypeDeclaration, InstantiationArg, ModuleTypeDeclaration,
    PrimitiveValType, RecGroup, RefType, StorageType, StructType, SubType, TypeRef, ValType,
    VariantCase,
};

/// ## Scope Tracking and Index Resolution
///
/// WebAssembly components introduce **nested index spaces**: components may
/// contain subcomponents, instances, types, and other constructs that define
/// their own indices. Inner scopes may also reference indices defined in
/// enclosing scopes via `(outer ...)`.
///
/// Because this crate supports **instrumentation and transformation** of
/// components, the order in which the IR is visited and encoded may differ from
/// the original parse order. As a result, index resolution cannot rely on
/// traversal order alone.
///
/// This module provides the infrastructure that ensures **correct and stable
/// index resolution** across parsing, instrumentation, and encoding.
///
/// ---
///
/// ### The Core Idea
///
/// Each IR node that participates in indexing is associated with a logical
/// **scope**. These associations are recorded once and later queried during
/// encoding.
///
/// The system guarantees that:
///
/// - Index scopes are assigned explicitly, not inferred from traversal order
/// - Nested scopes are resolved correctly, even under reordering or
///   instrumentation
/// - Encoding always uses the correct index space for the node being emitted
///
/// ---
///
/// ### Component Scopes
///
/// Components are identified by a stable **component ID** assigned when the
/// component is created or parsed.
///
/// Component scopes are registered and resolved using this ID rather than by
/// memory identity. During traversal, encoding maintains a **stack of component
/// IDs** representing the current nesting of components.
///
/// This makes component scope resolution:
///
/// - Independent of ownership or storage layout
/// - Robust to reordering and nested traversal
/// - Explicit and easy to reason about
///
/// ---
///
/// ### Scopes Within Components
///
/// All other scoped IR nodes—such as instances, type declarations, aliases, and
/// similar constructs—are associated with scopes relative to their enclosing
/// component.
///
/// During encoding, a **scope stack** tracks the currently active index spaces
/// as traversal enters and exits nested constructs. When an IR node needs to
/// resolve an index, its associated scope is retrieved and interpreted relative
/// to the current stack.
///
/// This allows deeply nested structures and `(outer ...)` references to be
/// encoded correctly without baking traversal assumptions into the IR.
///
/// ---
///
/// ### What This Enables
///
/// This design ensures that:
///
/// - Instrumentation can reorder or inject IR nodes without breaking index
///   correctness
/// - Encoding logic remains simple and declarative
/// - Index resolution remains correct for arbitrarily nested components
///
/// Users of the library do not need to manage scopes manually—scope tracking is
/// handled transparently as part of parsing and encoding.
///
/// ---
///
/// ### Design Philosophy
///
/// The scope system is intentionally explicit and conservative. Rather than
/// inferring meaning from traversal order, it records the structure of index
/// spaces directly and resolves them mechanically at encode time.
///
/// In short: **index correctness is enforced structurally, not procedurally**.
/// ```
#[derive(Default, Debug)]
pub(crate) struct IndexScopeRegistry {
    pub(crate) node_scopes: HashMap<NonNull<()>, ScopeEntry>,
    pub(crate) comp_scopes: HashMap<CompUniqueId, ScopeId>,
    /// Monotonic counter for `CompUniqueId` allocation. Shared across the
    /// entire IR tree (the registry is wrapped in `Rc<RefCell<>>`), so both
    /// the parser and `Component::add_component*` allocate fresh, globally
    /// unique component identities through `alloc_uid`.
    pub(crate) next_uid: u32,
}
impl IndexScopeRegistry {
    pub fn register<T: GetScopeKind>(&mut self, node: &T, space: ScopeId) {
        let ptr = NonNull::from(node).cast::<()>();
        let kind = node.scope_kind();
        debug_assert_ne!(
            kind,
            ScopeOwnerKind::Unregistered,
            "attempted to register an unscoped node"
        );

        let old = self.node_scopes.insert(ptr, ScopeEntry { space, kind });

        debug_assert!(old.is_none(), "node registered twice: {:p}", node);
    }

    pub fn scope_entry<T: GetScopeKind>(&self, node: &T) -> Option<ScopeEntry> {
        let ptr = NonNull::from(node).cast::<()>();

        if let Some(entry) = self.node_scopes.get(&ptr) {
            if entry.kind == node.scope_kind() {
                return Some(*entry);
            }
        }
        None
    }
    pub fn register_comp(&mut self, comp_id: CompUniqueId, space: ScopeId) {
        self.comp_scopes.insert(comp_id, space);
    }
    pub fn scope_of_comp(&self, comp_id: CompUniqueId) -> Option<ScopeId> {
        self.comp_scopes.get(&comp_id).copied()
    }
    /// Allocate the next globally-unique `CompUniqueId` and advance the
    /// counter. Both the parser and `add_component*` go through this so
    /// IDs never collide regardless of how the tree was built.
    pub fn alloc_uid(&mut self) -> CompUniqueId {
        let uid = CompUniqueId(self.next_uid);
        self.next_uid += 1;
        uid
    }
}

/// Every IR node can have a reference to this to allow for instrumentation
/// to have access to the index scope mappings and perform manipulations!
pub(crate) type RegistryHandle = Rc<RefCell<IndexScopeRegistry>>;

#[derive(Debug, Clone, Copy)]
pub struct ScopeEntry {
    pub space: ScopeId,
    pub kind: ScopeOwnerKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeOwnerKind {
    /// A `(component ...)`
    Component,

    /// A core `(core type (module ...))`
    CoreTypeModule,

    /// A `(component type (component ...))`
    ComponentTypeComponent,
    /// A `(component type (instance ...))`
    ComponentTypeInstance,

    // Extend as needed
    Unregistered,
}

pub trait GetScopeKind {
    fn scope_kind(&self) -> ScopeOwnerKind {
        ScopeOwnerKind::Unregistered
    }
}
impl GetScopeKind for Component<'_> {
    fn scope_kind(&self) -> ScopeOwnerKind {
        ScopeOwnerKind::Component
    }
}
impl GetScopeKind for CoreType<'_> {
    fn scope_kind(&self) -> ScopeOwnerKind {
        match self {
            CoreType::Module(_) => ScopeOwnerKind::CoreTypeModule,
            // other variants that do NOT introduce scopes should never be registered
            _ => ScopeOwnerKind::Unregistered,
        }
    }
}
impl GetScopeKind for ComponentType<'_> {
    fn scope_kind(&self) -> ScopeOwnerKind {
        match self {
            ComponentType::Component(_) => ScopeOwnerKind::ComponentTypeComponent,
            ComponentType::Instance(_) => ScopeOwnerKind::ComponentTypeInstance,
            ComponentType::Defined(_) | ComponentType::Func(_) | ComponentType::Resource { .. } => {
                ScopeOwnerKind::Unregistered
            }
        }
    }
}
impl GetScopeKind for Module<'_> {}
impl GetScopeKind for ComponentTypeRef {}
impl GetScopeKind for ComponentDefinedType<'_> {}
impl GetScopeKind for ComponentFuncType<'_> {}
impl GetScopeKind for ComponentTypeDeclaration<'_> {}
impl GetScopeKind for InstanceTypeDeclaration<'_> {}
impl GetScopeKind for ComponentInstance<'_> {}
impl GetScopeKind for CanonicalFunction {}
impl GetScopeKind for ComponentAlias<'_> {}
impl GetScopeKind for ComponentImport<'_> {}
impl GetScopeKind for ComponentExport<'_> {}
impl GetScopeKind for Instance<'_> {}
impl GetScopeKind for ComponentStartFunction {}
impl GetScopeKind for CustomSection<'_> {}
impl GetScopeKind for ValType {}
impl GetScopeKind for ComponentInstantiationArg<'_> {}
impl GetScopeKind for CanonicalOption {}
impl GetScopeKind for ComponentValType {}
impl GetScopeKind for InstantiationArg<'_> {}
impl GetScopeKind for Export<'_> {}
impl GetScopeKind for PrimitiveValType {}
impl GetScopeKind for VariantCase<'_> {}
impl GetScopeKind for CompositeInnerType {}
impl GetScopeKind for FuncType {}
impl GetScopeKind for FieldType {}
impl GetScopeKind for StructType {}
impl GetScopeKind for CompositeType {}
impl GetScopeKind for StorageType {}
impl GetScopeKind for RefType {}
impl GetScopeKind for RecGroup {}
impl GetScopeKind for ModuleTypeDeclaration<'_> {}
impl GetScopeKind for Import<'_> {}
impl GetScopeKind for TypeRef {}
impl GetScopeKind for SubType {}

/// Assert that a node is registered in the `ScopeRegistry` at this point.
/// Panics if the node is not found.
/// This helps with debugging issues where a node may have been moved and
/// no longer upholds the invariants required by the scope lookup mechanism.
/// These checks will not be present in a release build, only debug builds, since
/// the check is encapsulated inside a `debug_assert_eq`.
#[macro_export]
macro_rules! assert_registered {
    ($registry:expr, $node:expr) => {{
        debug_assert!(
            $registry.borrow().scope_entry($node).is_some(),
            // concat!(
            "Debug assertion failed: node is not registered in ScopeRegistry: {:?}",
            $node // )
        );
    }};
}
#[macro_export]
macro_rules! assert_registered_with_id {
    ($registry:expr, $node:expr, $scope_id:expr) => {{
        debug_assert_eq!(
            $scope_id,
            $registry
                .borrow()
                .scope_entry($node)
                .expect(concat!(
                    "Debug assertion failed: node is not registered in ScopeRegistry: ",
                    stringify!($node)
                ))
                .space
        );
    }};
}

#[derive(Clone, Debug)]
pub struct ComponentStore<'a> {
    components: HashMap<CompUniqueId, &'a Component<'a>>,
}
impl<'a> ComponentStore<'a> {
    pub fn get(&self, id: &CompUniqueId) -> &'a Component<'a> {
        self.components.get(id).unwrap()
    }
}

pub fn build_component_store<'a>(root: &'a Component<'a>) -> ComponentStore<'a> {
    let mut map = HashMap::new();

    fn walk<'a>(comp: &'a Component<'a>, map: &mut HashMap<CompUniqueId, &'a Component<'a>>) {
        map.insert(comp.uid, comp);
        for child in comp.components.iter() {
            walk(child, map);
        }
    }

    walk(root, &mut map);

    ComponentStore { components: map }
}
