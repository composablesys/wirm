use crate::ir::component::idx_spaces::Space;
use std::fmt::Display;

/// LocalID in a function
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LocalID(pub u32);
impl From<usize> for LocalID {
    fn from(value: usize) -> Self {
        LocalID(value as u32)
    }
}

impl std::ops::Deref for LocalID {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for LocalID {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// TypeID in a module
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TypeID(pub u32);
impl std::ops::Deref for TypeID {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for TypeID {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// ModuleID in a Component
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ModuleID(pub u32);
impl std::ops::Deref for ModuleID {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ModuleID {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// The ID of a component-level function (component function index space).
/// Distinct from `FunctionID` (core function index space).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ComponentFunctionId(pub u32);
impl std::ops::Deref for ComponentFunctionId {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ComponentFunctionId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// The ID returned by `Component::add_canon_func`. Polymorphic because the
/// index space a canonical function lands in depends on its variant: `Lift`
/// and the thread-spawn variants land in the component function index space,
/// while `Lower`, the resource ops, and most async/streams variants land in
/// the core function index space.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CanonFuncId {
    Component(ComponentFunctionId),
    Core(FunctionID),
}
impl CanonFuncId {
    /// Construct a `CanonFuncId` from the index space the canon landed in
    /// plus its raw index. Source of truth: `IndexSpaceOf for
    /// CanonicalFunction`. Panics if `space` isn't a space a canon can
    /// produce.
    pub fn new(space: Space, id: u32) -> Self {
        match space {
            Space::CompFunc => CanonFuncId::Component(ComponentFunctionId(id)),
            Space::CoreFunc => CanonFuncId::Core(FunctionID(id)),
            other => panic!(
                "CanonicalFunction unexpectedly produced index space {:?}; \
                 wasmparser may have added a new variant — update CanonFuncId \
                 to cover it",
                other
            ),
        }
    }

    /// Returns the contained `ComponentFunctionId`. Panics if this is the
    /// `Core` variant.
    pub fn unwrap_component(self) -> ComponentFunctionId {
        match self {
            CanonFuncId::Component(id) => id,
            CanonFuncId::Core(id) => {
                panic!("expected component func, got core func {}", *id)
            }
        }
    }

    /// Returns the contained `FunctionID`. Panics if this is the `Component`
    /// variant.
    pub fn unwrap_core(self) -> FunctionID {
        match self {
            CanonFuncId::Core(id) => id,
            CanonFuncId::Component(id) => {
                panic!("expected core func, got component func {}", *id)
            }
        }
    }
}

/// The ID returned by the `Component::add_alias_*` methods. An alias lands
/// in whichever index space its target kind names — covering every space
/// reachable from `ComponentAlias::{InstanceExport, CoreInstanceExport,
/// Outer}`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AliasId {
    // Component-level spaces (InstanceExport / Outer Type / Outer Component)
    ComponentFunc(ComponentFunctionId),
    ComponentInstance(ComponentInstanceId),
    ComponentType(ComponentTypeId),
    ComponentValue(ValueID),
    Component(ComponentId),
    // Core spaces (CoreInstanceExport / Outer CoreModule / Outer CoreType)
    CoreFunc(FunctionID),
    CoreTable(TableID),
    CoreMemory(MemoryID),
    CoreGlobal(GlobalID),
    CoreTag(TagID),
    CoreModule(ModuleID),
    CoreType(CoreTypeId),
}
impl AliasId {
    /// Construct an `AliasId` from the index space the alias landed in plus
    /// its raw index. Source of truth: `IndexSpaceOf for ComponentAlias`.
    /// Panics if `space` isn't a space an alias can produce.
    pub fn new(space: Space, id: u32) -> Self {
        match space {
            Space::CompFunc => AliasId::ComponentFunc(ComponentFunctionId(id)),
            Space::CompInst => AliasId::ComponentInstance(ComponentInstanceId(id)),
            Space::CompType => AliasId::ComponentType(ComponentTypeId(id)),
            Space::CompVal => AliasId::ComponentValue(ValueID(id)),
            Space::Comp => AliasId::Component(ComponentId(id)),
            Space::CoreFunc => AliasId::CoreFunc(FunctionID(id)),
            Space::CoreTable => AliasId::CoreTable(TableID(id)),
            Space::CoreMemory => AliasId::CoreMemory(MemoryID(id)),
            Space::CoreGlobal => AliasId::CoreGlobal(GlobalID(id)),
            Space::CoreTag => AliasId::CoreTag(TagID(id)),
            Space::CoreModule => AliasId::CoreModule(ModuleID(id)),
            Space::CoreType => AliasId::CoreType(CoreTypeId(id)),
            other => panic!(
                "ComponentAlias unexpectedly produced index space {:?}; \
                 not in the AliasId variant set",
                other
            ),
        }
    }

    /// Returns the contained `ComponentFunctionId`. Panics if the alias
    /// landed in a different index space.
    pub fn unwrap_component_func(self) -> ComponentFunctionId {
        match self {
            AliasId::ComponentFunc(id) => id,
            other => panic!("expected component func, got {:?}", other),
        }
    }

    /// Returns the contained `ComponentInstanceId`. Panics if the alias
    /// landed in a different index space.
    pub fn unwrap_component_instance(self) -> ComponentInstanceId {
        match self {
            AliasId::ComponentInstance(id) => id,
            other => panic!("expected component instance, got {:?}", other),
        }
    }

    /// Returns the contained `ComponentTypeId`. Panics if the alias landed
    /// in a different index space.
    pub fn unwrap_component_type(self) -> ComponentTypeId {
        match self {
            AliasId::ComponentType(id) => id,
            other => panic!("expected component type, got {:?}", other),
        }
    }

    /// Returns the contained `ValueID`. Panics if the alias landed in a
    /// different index space.
    pub fn unwrap_component_value(self) -> ValueID {
        match self {
            AliasId::ComponentValue(id) => id,
            other => panic!("expected component value, got {:?}", other),
        }
    }

    /// Returns the contained `ComponentId`. Panics if the alias landed in a
    /// different index space.
    pub fn unwrap_component(self) -> ComponentId {
        match self {
            AliasId::Component(id) => id,
            other => panic!("expected component, got {:?}", other),
        }
    }

    /// Returns the contained core `FunctionID`. Panics if the alias landed
    /// in a different index space.
    pub fn unwrap_core_func(self) -> FunctionID {
        match self {
            AliasId::CoreFunc(id) => id,
            other => panic!("expected core func, got {:?}", other),
        }
    }

    /// Returns the contained `TableID`. Panics if the alias landed in a
    /// different index space.
    pub fn unwrap_core_table(self) -> TableID {
        match self {
            AliasId::CoreTable(id) => id,
            other => panic!("expected core table, got {:?}", other),
        }
    }

    /// Returns the contained `MemoryID`. Panics if the alias landed in a
    /// different index space.
    pub fn unwrap_core_memory(self) -> MemoryID {
        match self {
            AliasId::CoreMemory(id) => id,
            other => panic!("expected core memory, got {:?}", other),
        }
    }

    /// Returns the contained `GlobalID`. Panics if the alias landed in a
    /// different index space.
    pub fn unwrap_core_global(self) -> GlobalID {
        match self {
            AliasId::CoreGlobal(id) => id,
            other => panic!("expected core global, got {:?}", other),
        }
    }

    /// Returns the contained `TagID`. Panics if the alias landed in a
    /// different index space.
    pub fn unwrap_core_tag(self) -> TagID {
        match self {
            AliasId::CoreTag(id) => id,
            other => panic!("expected core tag, got {:?}", other),
        }
    }

    /// Returns the contained core `ModuleID`. Panics if the alias landed in
    /// a different index space.
    pub fn unwrap_core_module(self) -> ModuleID {
        match self {
            AliasId::CoreModule(id) => id,
            other => panic!("expected core module, got {:?}", other),
        }
    }

    /// Returns the contained `CoreTypeId`. Panics if the alias landed in a
    /// different index space.
    pub fn unwrap_core_type(self) -> CoreTypeId {
        match self {
            AliasId::CoreType(id) => id,
            other => panic!("expected core type, got {:?}", other),
        }
    }
}

/// FunctionID in a module
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FunctionID(pub u32);
impl std::ops::Deref for FunctionID {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for FunctionID {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<u32> for FunctionID {
    fn from(id: u32) -> Self {
        Self(id)
    }
}
impl Display for FunctionID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// DataSegmentID in a module
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DataSegmentID(pub u32);
impl std::ops::Deref for DataSegmentID {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for DataSegmentID {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// GlobalID in a module
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GlobalID(pub u32);
impl std::ops::Deref for GlobalID {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for GlobalID {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// ImportsID in a module
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ImportsID(pub u32);
impl std::ops::Deref for ImportsID {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ImportsID {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// ExportsID - Refers to an exports position in a module/component's list of exports
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExportsID(pub u32);

impl From<usize> for ExportsID {
    fn from(value: usize) -> Self {
        ExportsID(value as u32)
    }
}

impl std::ops::Deref for ExportsID {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ExportsID {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Custom Section ID in a module
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CustomSectionID(pub u32);
impl std::ops::Deref for CustomSectionID {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for CustomSectionID {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Table ID in a module
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TableID(pub u32);
impl std::ops::Deref for TableID {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for TableID {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Memory ID in a module
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MemoryID(pub u32);
impl std::ops::Deref for MemoryID {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for MemoryID {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Tag ID in a module
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TagID(pub u32);
impl std::ops::Deref for TagID {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for TagID {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Field ID in a Struct or Array
pub struct FieldID(pub u32);
impl std::ops::Deref for FieldID {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for FieldID {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// ID of an element in the Elements Section
pub struct ElementID(pub u32);
impl std::ops::Deref for ElementID {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ElementID {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// The id of a component nested within a component
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ComponentId(pub u32);
impl std::ops::Deref for ComponentId {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A wirm-internal unique identifier for a `Component` within an IR tree.
/// Distinct from [`ComponentId`], which is the wasm-spec component-index-space
/// position and only meaningful relative to a parent. `CompUniqueId` is
/// globally unique across the entire tree and used as the HashMap key for
/// scope-registry lookups (see `IndexScopeRegistry::comp_scopes`). Allocated
/// monotonically by the parser as it walks; not exposed in any public
/// component-model index reference.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct CompUniqueId(pub(crate) u32);
impl std::ops::Deref for CompUniqueId {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for CompUniqueId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// The id of a component type
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ComponentTypeId(pub u32);
impl std::ops::Deref for ComponentTypeId {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ComponentTypeId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// The ID of a component-level value in a Component
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ValueID(pub u32);
impl std::ops::Deref for ValueID {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ValueID {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// The ID of a core instance in a Component
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CoreInstanceId(pub u32);
impl std::ops::Deref for CoreInstanceId {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for CoreInstanceId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// The ID of a component instance in a Component
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ComponentInstanceId(pub u32);
impl std::ops::Deref for ComponentInstanceId {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ComponentInstanceId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// The ID of a core type in a Component
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CoreTypeId(pub u32);
impl std::ops::Deref for CoreTypeId {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for CoreTypeId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
