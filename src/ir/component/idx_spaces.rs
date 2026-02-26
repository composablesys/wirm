use crate::ir::component::refs::IndexedRef;
use crate::ir::component::section::ComponentSection;
use crate::ir::types::CustomSection;
use crate::{Component, Module};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Debug;
use std::rc::Rc;
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentExternalKind, ComponentImport,
    ComponentInstance, ComponentOuterAliasKind, ComponentStartFunction, ComponentType,
    ComponentTypeDeclaration, ComponentTypeRef, CoreType, ExternalKind, Import, Instance,
    InstanceTypeDeclaration, InstantiationArgKind, ModuleTypeDeclaration, OuterAliasKind, RecGroup,
    SubType, TypeRef,
};

pub(crate) type ScopeId = usize;

/// Every IR node can have a reference to this to allow for instrumentation
/// to have access to the index stores and perform manipulations!
pub(crate) type StoreHandle = Rc<RefCell<IndexStore>>;

#[derive(Default, Debug)]
pub(crate) struct IndexStore {
    pub scopes: HashMap<ScopeId, IndexScope>,
    next_id: usize,
}
impl IndexStore {
    /// Create a new scope in the scope store.
    pub fn new_scope(&mut self) -> ScopeId {
        let id = self.use_next_id();
        self.scopes.insert(id, IndexScope::new(id));

        id
    }
    /// Lookup where to find an item in the component IR based on its assumed ID
    /// (the ID given to the item at parse and IR-injection time). This is done WITHOUT
    /// caching the found result, which is helpful when performing an operation when the
    /// IndexStore cannot be mutable.
    /// Returns:
    /// - .0,SpaceSubtype: the space vector to look up this index in
    /// - .1,usize: the index of the vector in the IR to find the item
    /// - .2,Option<usize>: the index within the node to find the item (as in pointing to a certain subtype in a recgroup)
    pub fn index_from_assumed_id_no_cache(
        &self,
        id: &ScopeId,
        r: &IndexedRef,
    ) -> (SpaceSubtype, usize, Option<usize>) {
        self.get(id).index_from_assumed_id_no_cache(r)
    }
    /// Lookup where to find an item in the component IR based on its assumed ID
    /// (the ID given to the item at parse and IR-injection time). The found result will
    /// then be cached for faster future lookups.
    /// Returns:
    /// - .0,SpaceSubtype: the space vector to look up this index in
    /// - .1,usize: the index of the vector in the IR to find the item
    /// - .2,Option<usize>: the index within the node to find the item (as in pointing to a certain subtype in a recgroup)
    pub fn index_from_assumed_id(
        &mut self,
        id: &ScopeId,
        r: &IndexedRef,
    ) -> (SpaceSubtype, usize, Option<usize>) {
        self.get_mut(id).index_from_assumed_id(r)
    }
    /// Give an assumed ID for some IR item (done at parse and IR-injection time).
    pub fn assign_assumed_id(
        &mut self,
        id: &ScopeId,
        space: &Space,
        section: &ComponentSection,
        curr_idx: usize,
    ) -> Option<usize> {
        self.get_mut(id).assign_assumed_id(space, section, curr_idx)
    }

    /// Iterate over a list of items to assign an assumed ID for.
    pub fn assign_assumed_id_for<I: Debug + IndexSpaceOf>(
        &mut self,
        id: &ScopeId,
        items: &[I],
        curr_idx: usize,
        sections: &Vec<ComponentSection>,
    ) {
        self.get_mut(id)
            .assign_assumed_id_for(items, curr_idx, sections)
    }
    /// Iterate over a list of _boxed_ items to assign an assumed ID for.
    pub fn assign_assumed_id_for_boxed<I: Debug + IndexSpaceOf>(
        &mut self,
        id: &ScopeId,
        items: &[Box<I>],
        curr_idx: usize,
        sections: &Vec<ComponentSection>,
    ) {
        self.get_mut(id)
            .assign_assumed_id_for_boxed(items, curr_idx, sections)
    }
    /// Use up the next ID to assign in the tracker.
    fn use_next_id(&mut self) -> ScopeId {
        let next = self.next_id;
        self.next_id += 1;

        next
    }

    /// Get an index scope that can be mutated.
    fn get_mut(&mut self, id: &ScopeId) -> &mut IndexScope {
        self.scopes.get_mut(id).unwrap()
    }
    /// Get an immutable ref to an index scope.
    fn get(&self, id: &ScopeId) -> &IndexScope {
        self.scopes.get(id).unwrap()
    }
}

/// A single lexical index scope in a WebAssembly component.
///
/// An `IndexScope` contains all index spaces that are *visible at one level*
/// of the component hierarchy. Each scope corresponds to a lexical boundary
/// introduced by constructs such as:
///
/// - a `component`
/// - a `component type`
/// - a `component instance`
///
/// Within a scope, indices are allocated monotonically and are only valid
/// relative to that scope. Nested constructs introduce *new* `IndexScope`s,
/// which may reference items in outer scopes via `(outer N ...)` declarations.
///
/// ## Relationship to the Component Model
///
/// In the WebAssembly Component Model, index spaces are *lexically scoped*.
/// For example:
///
/// - Component functions, values, instances, and types each have their own
///   index spaces.
/// - Core index spaces (functions, types, memories, etc.) are also scoped when
///   introduced at the component level.
/// - Entering a nested component (or component type / instance) creates a new
///   set of index spaces that shadow outer ones.
///
/// `IndexScope` models exactly one such lexical level.
///
/// ## Scope Stack Usage
///
/// `IndexScope` is intended to be used in conjunction with a stack structure
/// (e.g. `ScopeStack`), where:
///
/// - entering a nested construct pushes a new `IndexScope`
/// - exiting the construct pops it
/// - resolving `(outer depth ...)` references indexes into the stack by depth
///
/// This design allows encode-time traversal to correctly reindex references
/// even when IR nodes are visited in an arbitrary order (e.g. during
/// instrumentation).
///
/// ## Encode-Time Semantics
///
/// During encoding, the active `IndexScope` determines:
///
/// - where newly declared items are allocated
/// - how referenced indices are remapped
/// - which outer scope to consult for `(outer ...)` references
///
/// `IndexScope` does **not** represent all index spaces in the component;
/// it represents only those visible at a single lexical level.
///
/// We build these index spaces following the order of the original IR, then traverse the IR out-of-order
/// based on the instrumentation injections, we must enable the lookup of spaces through assigned IDs. This
/// ensures that we do not use the wrong index space for a node in a reordered list of IR nodes.
///
///
/// ## Design Note
///
/// This type intentionally separates *scope structure* from *IR structure*.
/// IR nodes do not own scopes; instead, scopes are entered and exited explicitly
/// during traversal. This keeps index resolution explicit, debuggable, and
/// faithful to the specification.
#[derive(Clone, Debug, Default)]
pub(crate) struct IndexScope {
    pub(crate) id: ScopeId,

    // Component-level spaces
    pub comp: IdxSpace,
    pub comp_func: IdxSpace,
    pub comp_val: IdxSpace,
    pub comp_type: IdxSpace,
    pub comp_inst: IdxSpace,

    // Core space (added by component model)
    pub core_inst: IdxSpace, // (these are module instances)
    pub module: IdxSpace,

    // Core spaces that exist at the component-level
    pub core_type: IdxSpace,
    pub core_func: IdxSpace, // these are canonical function decls!
    pub core_memory: IdxSpace,
    pub core_table: IdxSpace,
    pub core_global: IdxSpace,
    pub core_tag: IdxSpace,
}
impl IndexScope {
    pub fn new(id: ScopeId) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }

    /// This function is called as I parse a component. This is necessary since different items encoded
    /// in a component index into different namespaces. There is not a one-to-one relationship between
    /// those items' indices in a vector to the index space it manipulates!
    ///
    /// Consider a canonical function, this can take place of an index in the core-function OR the
    /// component-function index space!
    pub fn assign_assumed_id_for<I: Debug + IndexSpaceOf>(
        &mut self,
        items: &[I],
        curr_idx: usize,
        sections: &Vec<ComponentSection>, // one per item
    ) {
        debug_assert_eq!(items.len(), sections.len());
        for ((i, item), section) in items.iter().enumerate().zip(sections) {
            self.assign_assumed_id(&item.index_space_of(), section, curr_idx + i);
        }
    }
    pub fn assign_assumed_id_for_boxed<I: Debug + IndexSpaceOf>(
        &mut self,
        items: &[Box<I>],
        curr_idx: usize,
        sections: &Vec<ComponentSection>, // one per item
    ) {
        debug_assert_eq!(items.len(), sections.len());
        for ((i, item), section) in items.iter().enumerate().zip(sections) {
            self.assign_assumed_id(&item.index_space_of(), section, curr_idx + i);
        }
    }

    /// This is also called as I parse a component for the same reason mentioned above in the documentation for [`IdxSpaces.assign_assumed_id_for`].
    pub fn assign_assumed_id(
        &mut self,
        space: &Space,
        section: &ComponentSection,
        curr_idx: usize,
    ) -> Option<usize> {
        self.get_space_mut(space)
            .map(|space| space.assign_assumed_id(section, curr_idx))
    }

    pub fn lookup_assumed_id(
        &self,
        space: &Space,
        section: &ComponentSection,
        vec_idx: usize,
    ) -> usize {
        self.get_space(space)
            .and_then(|s| s.lookup_assumed_id(section, vec_idx))
            .unwrap_or_else(|| {
                panic!("[{space:?}] Internal error: No assumed ID for index: {vec_idx}")
            })
    }

    pub fn lookup_assumed_id_with_subvec(
        &self,
        space: &Space,
        section: &ComponentSection,
        vec_idx: usize,
        subvec_idx: usize,
    ) -> usize {
        self.get_space(space)
            .and_then(|space| space.lookup_assumed_id_with_subvec(section, vec_idx, subvec_idx))
            .unwrap_or_else(|| {
                panic!("[{space:?}] Internal error: No assumed ID for index: {vec_idx}, subvec index: {subvec_idx}")
            })
    }

    pub fn index_from_assumed_id(
        &mut self,
        r: &IndexedRef,
    ) -> (SpaceSubtype, usize, Option<usize>) {
        self.get_space_mut(&r.space)
            .and_then(|space| space.index_from_assumed_id(r.index as usize))
            .unwrap_or_else(|| {
                panic!(
                    "[{:?}@scope{}] Internal error: No index for assumed ID: {}",
                    r.space, self.id, r.index
                )
            })
    }

    pub fn index_from_assumed_id_no_cache(
        &self,
        r: &IndexedRef,
    ) -> (SpaceSubtype, usize, Option<usize>) {
        self.get_space(&r.space)
            .and_then(|space| space.index_from_assumed_id_no_cache(r.index as usize))
            .unwrap_or_else(|| {
                panic!(
                    "[{:?}@scope{}] Internal error: No index for assumed ID: {}",
                    r.space, self.id, r.index
                )
            })
    }

    // ===================
    // ==== UTILITIES ====
    // ===================

    fn get_space_mut(&mut self, space: &Space) -> Option<&mut IdxSpace> {
        let s = match space {
            Space::Comp => &mut self.comp,
            Space::CompFunc => &mut self.comp_func,
            Space::CompVal => &mut self.comp_val,
            Space::CompType => &mut self.comp_type,
            Space::CompInst => &mut self.comp_inst,
            Space::CoreInst => &mut self.core_inst,
            Space::CoreModule => &mut self.module,
            Space::CoreType => &mut self.core_type,
            Space::CoreFunc => &mut self.core_func,
            Space::CoreMemory => &mut self.core_memory,
            Space::CoreTable => &mut self.core_table,
            Space::CoreGlobal => &mut self.core_global,
            Space::CoreTag => &mut self.core_tag,
            Space::NA => return None,
        };
        Some(s)
    }

    fn get_space(&self, space: &Space) -> Option<&IdxSpace> {
        let s = match space {
            Space::Comp => &self.comp,
            Space::CompFunc => &self.comp_func,
            Space::CompVal => &self.comp_val,
            Space::CompType => &self.comp_type,
            Space::CompInst => &self.comp_inst,
            Space::CoreInst => &self.core_inst,
            Space::CoreModule => &self.module,
            Space::CoreType => &self.core_type,
            Space::CoreFunc => &self.core_func,
            Space::CoreMemory => &self.core_memory,
            Space::CoreTable => &self.core_table,
            Space::CoreGlobal => &self.core_global,
            Space::CoreTag => &self.core_tag,
            Space::NA => return None,
        };
        Some(s)
    }
}

/// How we represent the assumed IDs at some index location in the IR
#[derive(Clone, Debug)]
enum AssumedIdForIdx {
    /// This can be mapped to a SINGLE assumed ID
    Single(usize),
    /// OR multiple IDs for an index in the IR (rec groups take up a single
    /// index in the core_types vector, but can have multiple core type IDs. One
    /// for each rec group subtype!)
    Multiple(Vec<usize>),
}
impl AssumedIdForIdx {
    /// Returns whether this is a match for the passed assumed_id AND
    /// the optional index in the IR's subvec
    fn matches(&self, assumed_id: usize) -> (bool, Option<usize>) {
        match self {
            AssumedIdForIdx::Single(my_id) => return (*my_id == assumed_id, None),
            AssumedIdForIdx::Multiple(sub_ids) => {
                for (idx, id) in sub_ids.iter().enumerate() {
                    if *id == assumed_id {
                        return (true, Some(idx));
                    }
                }
            }
        }
        (false, None)
    }
    fn append(&mut self, assumed_id: usize) {
        match self {
            Self::Single(my_id) => *self = AssumedIdForIdx::Multiple(vec![*my_id, assumed_id]),
            Self::Multiple(sub_ids) => sub_ids.push(assumed_id),
        }
    }
    fn unwrap_single(&self) -> usize {
        match self {
            AssumedIdForIdx::Single(my_id) => *my_id,
            _ => unreachable!(),
        }
    }
    fn unwrap_for_idx(&self, subvec_idx: usize) -> usize {
        match self {
            AssumedIdForIdx::Single(my_id) => {
                debug_assert_eq!(subvec_idx, 0);
                *my_id
            }
            AssumedIdForIdx::Multiple(subvec) => subvec[subvec_idx],
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct IdxSpace {
    /// This is the current ID that we've reached associated with this index space.
    current_id: usize,

    /// Tracks the index in the MAIN item vector to the ID we've assumed for it: `main_idx -> assumed_id`
    /// This ID will be used to reference that item in the IR.
    main_assumed_ids: HashMap<usize, AssumedIdForIdx>,

    // The below maps are to track assumed IDs for item vectors that index into this index space.
    /// Tracks the index in the ALIAS item vector to the ID we've assumed for it: `alias_idx -> assumed_id`
    /// This ID will be used to reference that item in the IR.
    alias_assumed_ids: HashMap<usize, AssumedIdForIdx>,
    /// Tracks the index in the IMPORT item vector to the ID we've assumed for it: `imports_idx -> assumed_id`
    /// This ID will be used to reference that item in the IR.
    imports_assumed_ids: HashMap<usize, AssumedIdForIdx>,
    /// Tracks the index in the EXPORT item vector to the ID we've assumed for it: `exports_idx -> assumed_id`
    /// This ID will be used to reference that item in the IR.
    exports_assumed_ids: HashMap<usize, AssumedIdForIdx>,

    index_from_assumed_id_cache: HashMap<usize, (SpaceSubtype, usize, Option<usize>)>,
}
impl IdxSpace {
    pub fn curr_id(&self) -> usize {
        // This returns the ID that we've reached thus far while encoding
        self.current_id
    }

    fn next(&mut self) -> usize {
        let curr = self.current_id;
        self.current_id += 1;
        curr
    }

    pub fn lookup_assumed_id(&self, section: &ComponentSection, vec_idx: usize) -> Option<usize> {
        let (_group, vector) = match section {
            ComponentSection::ComponentImport => ("imports", &self.imports_assumed_ids),
            ComponentSection::ComponentExport => ("exports", &self.exports_assumed_ids),
            ComponentSection::Alias => ("aliases", &self.alias_assumed_ids),

            ComponentSection::Component
            | ComponentSection::Module
            | ComponentSection::CoreType
            | ComponentSection::ComponentType
            | ComponentSection::CoreInstance
            | ComponentSection::ComponentInstance
            | ComponentSection::Canon
            | ComponentSection::CustomSection
            | ComponentSection::ComponentStartSection => ("main", &self.main_assumed_ids),
        };

        vector.get(&vec_idx).map(|res| res.unwrap_single())
    }

    pub fn lookup_assumed_id_with_subvec(
        &self,
        section: &ComponentSection,
        vec_idx: usize,
        subvec_idx: usize,
    ) -> Option<usize> {
        let (_group, vector) = match section {
            ComponentSection::ComponentImport => ("imports", &self.imports_assumed_ids),
            ComponentSection::ComponentExport => ("exports", &self.exports_assumed_ids),
            ComponentSection::Alias => ("aliases", &self.alias_assumed_ids),

            ComponentSection::Component
            | ComponentSection::Module
            | ComponentSection::CoreType
            | ComponentSection::ComponentType
            | ComponentSection::CoreInstance
            | ComponentSection::ComponentInstance
            | ComponentSection::Canon
            | ComponentSection::CustomSection
            | ComponentSection::ComponentStartSection => ("main", &self.main_assumed_ids),
        };

        vector
            .get(&vec_idx)
            .map(|res| res.unwrap_for_idx(subvec_idx))
    }

    /// Returns:
    /// - .0,SpaceSubtype: the space vector to look up this index in
    /// - .1,usize: the index of the vector in the IR to find the item
    /// - .2,Option<usize>: the index within the node to find the item (as in pointing to a certain subtype in a recgroup)
    pub fn index_from_assumed_id(
        &mut self,
        assumed_id: usize,
    ) -> Option<(SpaceSubtype, usize, Option<usize>)> {
        if let Some(cached_data) = self.index_from_assumed_id_cache.get(&assumed_id) {
            return Some(*cached_data);
        }

        // We haven't cached this yet, we must do the less efficient logic and do a full lookup,
        // then we can cache what we find!
        let maps = [
            (SpaceSubtype::Main, &self.main_assumed_ids),
            (SpaceSubtype::Import, &self.imports_assumed_ids),
            (SpaceSubtype::Export, &self.exports_assumed_ids),
            (SpaceSubtype::Alias, &self.alias_assumed_ids),
        ];

        for (subty, map) in maps.iter() {
            for (idx, assumed) in map.iter() {
                let (matches, opt_subidx) = assumed.matches(assumed_id);
                if matches {
                    let result = (*subty, *idx, opt_subidx);
                    // cache what we found
                    self.index_from_assumed_id_cache.insert(assumed_id, result);

                    return Some(result);
                }
            }
        }
        None
    }
    /// Returns:
    /// - .0,SpaceSubtype: the space vector to look up this index in
    /// - .1,usize: the index of the vector in the IR to find the item
    /// - .2,Option<usize>: the index within the node to find the item (as in pointing to a certain subtype in a recgroup)
    pub fn index_from_assumed_id_no_cache(
        &self,
        assumed_id: usize,
    ) -> Option<(SpaceSubtype, usize, Option<usize>)> {
        if let Some(cached_data) = self.index_from_assumed_id_cache.get(&assumed_id) {
            return Some(*cached_data);
        }

        // We haven't cached this yet, we must do the less efficient logic and do a full lookup,
        // then we can cache what we find!
        let maps = [
            (SpaceSubtype::Main, &self.main_assumed_ids),
            (SpaceSubtype::Import, &self.imports_assumed_ids),
            (SpaceSubtype::Export, &self.exports_assumed_ids),
            (SpaceSubtype::Alias, &self.alias_assumed_ids),
        ];

        for (subty, map) in maps.iter() {
            for (idx, assumed) in map.iter() {
                let (matches, opt_subidx) = assumed.matches(assumed_id);
                if matches {
                    let result = (*subty, *idx, opt_subidx);
                    return Some(result);
                }
            }
        }
        None
    }

    pub fn assign_assumed_id(&mut self, section: &ComponentSection, vec_idx: usize) -> usize {
        let assumed_id = self.curr_id();
        self.next();
        let to_update = match section {
            ComponentSection::ComponentImport => &mut self.imports_assumed_ids,
            ComponentSection::ComponentExport => &mut self.exports_assumed_ids,
            ComponentSection::Alias => &mut self.alias_assumed_ids,

            ComponentSection::Component
            | ComponentSection::Module
            | ComponentSection::CoreType
            | ComponentSection::ComponentType
            | ComponentSection::CoreInstance
            | ComponentSection::ComponentInstance
            | ComponentSection::Canon
            | ComponentSection::CustomSection
            | ComponentSection::ComponentStartSection => &mut self.main_assumed_ids,
        };
        to_update
            .entry(vec_idx)
            .and_modify(|entry| {
                entry.append(assumed_id);
            })
            .or_insert(AssumedIdForIdx::Single(assumed_id));

        assumed_id
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SpaceSubtype {
    Export,
    Import,
    Alias,
    Main,
}

// Logic to figure out which index space is being manipulated
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Space {
    // Component-level spaces
    Comp,
    CompFunc,
    CompVal,
    CompType,
    CompInst,

    // Core-level spaces
    CoreInst,
    CoreModule,
    CoreType,
    CoreFunc,
    CoreMemory,
    CoreTable,
    CoreGlobal,
    CoreTag,

    // isn't part of an index space
    NA,
}

// Trait for centralizing index space mapping
pub trait IndexSpaceOf {
    fn index_space_of(&self) -> Space;
}

impl IndexSpaceOf for CustomSection<'_> {
    fn index_space_of(&self) -> Space {
        Space::NA
    }
}

impl IndexSpaceOf for ComponentStartFunction {
    fn index_space_of(&self) -> Space {
        Space::NA
    }
}

impl IndexSpaceOf for ComponentTypeRef {
    fn index_space_of(&self) -> Space {
        // This is the index space to use when looking up
        // the IDs in this ref.
        match self {
            Self::Value(_) => Space::CompVal,
            Self::Instance(_) => Space::CompInst,
            Self::Component(_) => Space::CompType,
            Self::Module(_) => Space::CoreModule,
            Self::Func(_) | Self::Type(_) => Space::CompType,
        }
    }
}

impl IndexSpaceOf for ComponentImport<'_> {
    fn index_space_of(&self) -> Space {
        // This is the index space of THIS IMPORT!
        // Not what space to use for the IDs of the typeref!
        match self.ty {
            ComponentTypeRef::Func(_) => Space::CompFunc,
            ComponentTypeRef::Value(_) => Space::CompVal,
            ComponentTypeRef::Type(_) => Space::CompType,
            ComponentTypeRef::Instance(_) => Space::CompInst,
            ComponentTypeRef::Component(_) => Space::Comp, // verified in wat (instantiate.wast)
            ComponentTypeRef::Module(_) => Space::CoreModule,
        }
    }
}

impl IndexSpaceOf for ComponentExport<'_> {
    fn index_space_of(&self) -> Space {
        // This is the index space of THIS EXPORT!
        // Not what space to use for the IDs of the typeref!
        match self.kind {
            ComponentExternalKind::Module => Space::CoreModule,
            ComponentExternalKind::Func => Space::CompFunc,
            ComponentExternalKind::Value => Space::CompVal,
            ComponentExternalKind::Type => Space::CompType,
            ComponentExternalKind::Instance => Space::CompInst,
            ComponentExternalKind::Component => Space::CompInst, // verified in alias.wast
        }
    }
}

impl IndexSpaceOf for Instance<'_> {
    fn index_space_of(&self) -> Space {
        Space::CoreInst
    }
}

impl<'a> IndexSpaceOf for ComponentAlias<'a> {
    fn index_space_of(&self) -> Space {
        match self {
            // Aliasing an export of a component instance
            ComponentAlias::InstanceExport { kind, .. } => match kind {
                ComponentExternalKind::Func => Space::CompFunc,
                ComponentExternalKind::Value => Space::CompVal,
                ComponentExternalKind::Type => Space::CompType,
                ComponentExternalKind::Instance => Space::CompInst,
                ComponentExternalKind::Component => Space::Comp, // verified in alias.wast
                ComponentExternalKind::Module => Space::CoreModule,
            },

            // Aliasing an export of a core instance
            ComponentAlias::CoreInstanceExport { kind, .. } => match kind {
                ExternalKind::Func => Space::CoreFunc,
                ExternalKind::Memory => Space::CoreMemory,
                ExternalKind::Table => Space::CoreTable,
                ExternalKind::Global => Space::CoreGlobal,
                ExternalKind::Tag => Space::CoreTag,
                ExternalKind::FuncExact => Space::CoreFunc,
            },

            // Aliasing an outer item
            ComponentAlias::Outer { kind, .. } => match kind {
                ComponentOuterAliasKind::CoreModule => Space::CoreModule,
                ComponentOuterAliasKind::CoreType => Space::CoreType,
                ComponentOuterAliasKind::Type => Space::CompType,
                ComponentOuterAliasKind::Component => Space::Comp, // verified in alias.wast
            },
        }
    }
}

impl IndexSpaceOf for CanonicalFunction {
    fn index_space_of(&self) -> Space {
        match self {
            CanonicalFunction::Lower { .. } => Space::CoreFunc,
            CanonicalFunction::Lift { .. } => Space::CompFunc,

            // Resource-related functions reference a resource type
            CanonicalFunction::ResourceNew { .. }
            | CanonicalFunction::ResourceDrop { .. }
            | CanonicalFunction::ResourceDropAsync { .. }
            | CanonicalFunction::ResourceRep { .. } => Space::CoreFunc,

            // Thread spawn / new indirect → function type
            CanonicalFunction::ThreadSpawnRef { .. }
            | CanonicalFunction::ThreadSpawnIndirect { .. } => Space::CompFunc,
            CanonicalFunction::ThreadNewIndirect { .. } => Space::CoreFunc,

            // Task-related functions operate on values
            CanonicalFunction::TaskReturn { .. }
            | CanonicalFunction::TaskCancel
            | CanonicalFunction::SubtaskDrop
            | CanonicalFunction::SubtaskCancel { .. } => Space::CoreFunc,

            // Context access
            CanonicalFunction::ContextGet(_) | CanonicalFunction::ContextSet(_) => Space::CoreFunc,

            // Stream / Future functions operate on types
            CanonicalFunction::StreamCancelRead { .. }
            | CanonicalFunction::StreamCancelWrite { .. }
            | CanonicalFunction::FutureCancelRead { .. }
            | CanonicalFunction::FutureCancelWrite { .. }
            | CanonicalFunction::FutureNew { .. }
            | CanonicalFunction::FutureRead { .. }
            | CanonicalFunction::FutureWrite { .. }
            | CanonicalFunction::FutureDropReadable { .. }
            | CanonicalFunction::FutureDropWritable { .. }
            | CanonicalFunction::StreamNew { .. }
            | CanonicalFunction::StreamRead { .. }
            | CanonicalFunction::StreamWrite { .. }
            | CanonicalFunction::StreamDropReadable { .. }
            | CanonicalFunction::StreamDropWritable { .. } => Space::CoreFunc,

            // Error context → operate on values
            CanonicalFunction::ErrorContextNew { .. }
            | CanonicalFunction::ErrorContextDebugMessage { .. }
            | CanonicalFunction::ErrorContextDrop => Space::CoreFunc,

            // Waitable set → memory
            CanonicalFunction::WaitableSetWait { .. }
            | CanonicalFunction::WaitableSetPoll { .. }
            | CanonicalFunction::WaitableSetNew
            | CanonicalFunction::WaitableSetDrop
            | CanonicalFunction::WaitableJoin => Space::CoreFunc,

            // Thread functions
            CanonicalFunction::ThreadIndex
            | CanonicalFunction::ThreadSwitchTo { .. }
            | CanonicalFunction::ThreadSuspend { .. }
            | CanonicalFunction::ThreadResumeLater
            | CanonicalFunction::ThreadYieldTo { .. }
            | CanonicalFunction::ThreadYield { .. }
            | CanonicalFunction::ThreadAvailableParallelism => Space::CoreFunc,

            CanonicalFunction::BackpressureInc | CanonicalFunction::BackpressureDec => {
                Space::CoreFunc
            }
        }
    }
}

impl IndexSpaceOf for Module<'_> {
    fn index_space_of(&self) -> Space {
        Space::CoreModule
    }
}

impl IndexSpaceOf for Component<'_> {
    fn index_space_of(&self) -> Space {
        Space::Comp // verified
    }
}

impl IndexSpaceOf for CoreType<'_> {
    fn index_space_of(&self) -> Space {
        Space::CoreType
    }
}

impl IndexSpaceOf for RecGroup {
    fn index_space_of(&self) -> Space {
        Space::CoreType
    }
}

impl IndexSpaceOf for SubType {
    fn index_space_of(&self) -> Space {
        Space::CoreType
    }
}

impl IndexSpaceOf for ComponentType<'_> {
    fn index_space_of(&self) -> Space {
        Space::CompType
    }
}

impl IndexSpaceOf for ComponentInstance<'_> {
    fn index_space_of(&self) -> Space {
        Space::CompInst
    }
}

impl IndexSpaceOf for InstantiationArgKind {
    fn index_space_of(&self) -> Space {
        match self {
            InstantiationArgKind::Instance => Space::CoreInst,
        }
    }
}

impl IndexSpaceOf for ExternalKind {
    fn index_space_of(&self) -> Space {
        match self {
            ExternalKind::Func => Space::CoreFunc,
            ExternalKind::Table => Space::CoreTable,
            ExternalKind::Memory => Space::CoreMemory,
            ExternalKind::Global => Space::CoreGlobal,
            ExternalKind::Tag => Space::CoreTag,
            ExternalKind::FuncExact => Space::CoreFunc,
        }
    }
}

impl IndexSpaceOf for ComponentExternalKind {
    fn index_space_of(&self) -> Space {
        match self {
            ComponentExternalKind::Func => Space::CompFunc,
            ComponentExternalKind::Value => Space::CompVal,
            ComponentExternalKind::Type => Space::CompType,
            ComponentExternalKind::Instance => Space::CompInst,
            ComponentExternalKind::Component => Space::Comp, // verified in alias.wast
            ComponentExternalKind::Module => Space::CoreModule,
        }
    }
}

impl IndexSpaceOf for ComponentOuterAliasKind {
    fn index_space_of(&self) -> Space {
        match self {
            ComponentOuterAliasKind::CoreModule => Space::CoreModule,
            ComponentOuterAliasKind::CoreType => Space::CoreType,
            ComponentOuterAliasKind::Type => Space::CompType,
            ComponentOuterAliasKind::Component => Space::Comp, // verified in wat (alias.wast)
        }
    }
}

impl IndexSpaceOf for ComponentTypeDeclaration<'_> {
    fn index_space_of(&self) -> Space {
        match self {
            ComponentTypeDeclaration::CoreType(ty) => ty.index_space_of(),
            ComponentTypeDeclaration::Type(ty) => ty.index_space_of(),
            ComponentTypeDeclaration::Alias(alias) => alias.index_space_of(),
            ComponentTypeDeclaration::Export { ty, .. } => ty.index_space_of(),
            ComponentTypeDeclaration::Import(import) => import.index_space_of(),
        }
    }
}

impl IndexSpaceOf for InstanceTypeDeclaration<'_> {
    fn index_space_of(&self) -> Space {
        match self {
            InstanceTypeDeclaration::CoreType(ty) => ty.index_space_of(),
            InstanceTypeDeclaration::Type(ty) => ty.index_space_of(),
            InstanceTypeDeclaration::Alias(a) => a.index_space_of(),
            InstanceTypeDeclaration::Export { ty, .. } => ty.index_space_of(),
        }
    }
}

impl IndexSpaceOf for ModuleTypeDeclaration<'_> {
    fn index_space_of(&self) -> Space {
        match self {
            ModuleTypeDeclaration::Type(_) => Space::CoreType,
            ModuleTypeDeclaration::Export { ty, .. } => ty.index_space_of(),
            ModuleTypeDeclaration::OuterAlias { kind, .. } => kind.index_space_of(),
            ModuleTypeDeclaration::Import(Import { ty, .. }) => ty.index_space_of(),
        }
    }
}

impl IndexSpaceOf for TypeRef {
    fn index_space_of(&self) -> Space {
        Space::CoreType
    }
}

impl IndexSpaceOf for OuterAliasKind {
    fn index_space_of(&self) -> Space {
        match self {
            OuterAliasKind::Type => Space::CoreType,
        }
    }
}
