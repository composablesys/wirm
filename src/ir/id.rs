#![allow(dead_code)]
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

/// ComponentId of a Component
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ComponentId(pub u32);
impl std::ops::Deref for ComponentId {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// ComponentTypeId in a Component
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

/// ComponentTypeInstanceId in a Component
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ComponentTypeInstanceId(pub u32);
impl std::ops::Deref for ComponentTypeInstanceId {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ComponentTypeInstanceId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// ComponentTypeFuncId in a Component
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ComponentTypeFuncId(pub u32);
impl std::ops::Deref for ComponentTypeFuncId {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ComponentTypeFuncId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// CanonicalFuncId in a Component
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CanonicalFuncId(pub u32);
impl std::ops::Deref for CanonicalFuncId {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for CanonicalFuncId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// ComponentExportId in a Component
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ComponentExportId(pub u32);
impl std::ops::Deref for ComponentExportId {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ComponentExportId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// AliasId in a Component
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AliasId(pub u32);
impl std::ops::Deref for AliasId {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for AliasId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// The ID of an aliased function in a Component
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AliasFuncId(pub u32);
impl std::ops::Deref for AliasFuncId {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for AliasFuncId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// The ID of an aliased function in a Component
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
