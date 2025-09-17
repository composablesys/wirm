use std::collections::HashMap;
use wasmparser::{ComponentAlias, ComponentExternalKind, ComponentOuterAliasKind, ComponentTypeRef, ExternalKind};
use crate::ir::section::ComponentSection;

#[derive(Clone, Debug, Default)]
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

    // General trackers for indices of item vectors
    last_processed_module: usize,
    last_processed_alias: usize,
    last_processed_core_ty: usize,
    last_processed_comp_ty: usize,
    last_processed_imp: usize,
    last_processed_exp: usize,
    last_processed_core_inst: usize,
    last_processed_comp_inst: usize,
    last_processed_canon: usize,
    last_processed_component: usize,
    last_processed_custom: usize,
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

    pub fn is_encoded(&self, outer: &ComponentSection, inner: &ExternalItemKind, vec_idx: usize) -> bool {
        let assumed_id = self.lookup_assumed_id(outer, inner, vec_idx);
        if let Some(space) = self.get_space(outer, inner) {
            let res = space.is_encoded(assumed_id);
            println!("[{:?}::{:?}] is_encoded? {vec_idx} -- {res}", outer, inner);

            res
        } else {
            panic!("[{:?}::{:?}] Should be able to find a matching space for this!", outer, inner);
        }
    }

    pub fn assign_assumed_id(&mut self, outer: &ComponentSection, inner: &ExternalItemKind, curr_idx: usize) -> Option<usize> {
        if let Some(space) = self.get_space_mut(outer, inner) {
            Some(space.assign_assumed_id(outer, curr_idx))
        } else {
            None
        }
    }

    pub fn lookup_assumed_id(&self, outer: &ComponentSection, inner: &ExternalItemKind, vec_idx: usize) -> usize {
        if let Some(space) = self.get_space(outer, inner) {
            if let Some(assumed_id) = space.lookup_assumed_id(outer, vec_idx) {
                return *assumed_id
            }
        }
        panic!("[{:?}::{:?}] No assumed ID for index: {}", outer, inner, vec_idx)
    }

    pub fn index_from_assumed_id(&self, outer: &ComponentSection, inner: &ExternalItemKind, assumed_id: usize) -> (SpaceSubtype, usize) {
        // TODO -- this is incredibly inefficient...i just want to move on with my life...
        if let Some(space) = self.get_space(outer, inner) {
            if let Some((ty, idx)) = space.index_from_assumed_id(outer, assumed_id) {
                return (ty, idx)
            } else {
                println!("couldn't find idx");
            }
        } else {
            println!("couldn't find space");
        }
        panic!("[{:?}::{:?}] No index for assumed ID: {}", outer, inner, assumed_id)
    }

    pub fn assign_actual_id(&mut self, outer: &ComponentSection, inner: &ExternalItemKind, vec_idx: usize) {
        let assumed_id = self.lookup_assumed_id(outer, inner, vec_idx);
        if let Some(space) = self.get_space_mut(outer, inner) {
            space.assign_actual_id(assumed_id);
        }
    }
    
    pub fn lookup_actual_id(&self, outer: &ComponentSection, inner: &ExternalItemKind, assumed_id: usize) -> Option<&usize> {
        if let Some(space) = self.get_space(outer, inner) {
            space.lookup_actual_id(assumed_id)
        } else {
            None
        }
    }

    pub fn lookup_actual_id_or_panic(&self, outer: &ComponentSection, inner: &ExternalItemKind, assumed_id: usize) -> usize {
        if let Some(space) = self.get_space(outer, inner) {
            if let Some(actual_id) = space.lookup_actual_id(assumed_id) {
                return *actual_id;
            }
        }
        panic!("[{:?}::{:?}] Can't find assumed id {assumed_id} in id-tracker", outer, inner);
    }

    pub fn visit_section(&mut self, section: &ComponentSection, num: usize) -> usize {
        let tracker = match section {
            ComponentSection::Module => &mut self.last_processed_module,
            ComponentSection::Alias => &mut self.last_processed_alias,
            ComponentSection::CoreType => &mut self.last_processed_core_ty,
            ComponentSection::ComponentType => &mut self.last_processed_comp_ty,
            ComponentSection::ComponentImport => &mut self.last_processed_imp,
            ComponentSection::ComponentExport => &mut self.last_processed_exp,
            ComponentSection::CoreInstance => &mut self.last_processed_core_inst,
            ComponentSection::ComponentInstance => &mut self.last_processed_comp_inst,
            ComponentSection::Canon => &mut self.last_processed_canon,
            ComponentSection::CustomSection => &mut self.last_processed_custom,
            ComponentSection::Component => &mut self.last_processed_component,
            ComponentSection::ComponentStartSection => panic!("No need to call this function for the start section!")
        };

        let curr = *tracker;
        *tracker += num;
        curr
    }

    pub fn reset_ids(&mut self) {
        self.comp_func.reset_ids();
        self.comp_val.reset_ids();
        self.comp_type.reset_ids();
        self.comp_inst.reset_ids();
        self.comp.reset_ids();

        self.core_inst.reset_ids();
        self.module.reset_ids();

        self.core_type.reset_ids();
        self.core_func.reset_ids();
    }

    // ===================
    // ==== UTILITIES ====
    // ===================

    fn get_space_mut(&mut self, outer: &ComponentSection, inner: &ExternalItemKind) -> Option<&mut IdxSpace> {
        let space = match outer {
            ComponentSection::Module => &mut self.module,
            ComponentSection::CoreType => &mut self.core_type,
            ComponentSection::ComponentType => &mut self.comp_type,
            ComponentSection::CoreInstance => &mut self.core_inst,
            ComponentSection::ComponentInstance => &mut self.comp_inst,
            ComponentSection::Canon => &mut self.core_func,
            ComponentSection::Component => &mut self.comp,

            // These manipulate other index spaces!
            ComponentSection::Alias |
            ComponentSection::ComponentImport |
            ComponentSection::ComponentExport => match inner {
                ExternalItemKind::CompFunc => &mut self.comp_func,
                ExternalItemKind::CompVal => &mut self.comp_val,
                ExternalItemKind::CompType => &mut self.comp_type,
                ExternalItemKind::CompInst => &mut self.comp_inst,
                ExternalItemKind::Comp => &mut self.comp,
                ExternalItemKind::CoreInst => &mut self.core_inst,
                ExternalItemKind::Module => &mut self.module,
                ExternalItemKind::CoreType => &mut self.core_type,
                ExternalItemKind::CoreFunc => &mut self.core_func,
                ExternalItemKind::NA => return None // nothing to do
            }
            ComponentSection::ComponentStartSection |
            ComponentSection::CustomSection => return None // nothing to do for custom or start sections
        };
        Some(space)
    }

    fn get_space(&self, outer: &ComponentSection, inner: &ExternalItemKind) -> Option<&IdxSpace> {
        let space = match outer {
            ComponentSection::Module => &self.module,
            ComponentSection::CoreType => &self.core_type,
            ComponentSection::ComponentType => &self.comp_type,
            ComponentSection::CoreInstance => &self.core_inst,
            ComponentSection::ComponentInstance => &self.comp_inst,
            ComponentSection::Canon => &self.core_func,
            ComponentSection::Component => &self.comp,

            // These manipulate other index spaces!
            ComponentSection::Alias |
            ComponentSection::ComponentImport |
            ComponentSection::ComponentExport => match inner {
                ExternalItemKind::CompFunc => &self.comp_func,
                ExternalItemKind::CompVal => &self.comp_val,
                ExternalItemKind::CompType => &self.comp_type,
                ExternalItemKind::CompInst => &self.comp_inst,
                ExternalItemKind::Comp => &self.comp,
                ExternalItemKind::CoreInst => &self.core_inst,
                ExternalItemKind::Module => &self.module,
                ExternalItemKind::CoreType => &self.core_type,
                ExternalItemKind::CoreFunc => &self.core_func,
                ExternalItemKind::NA => return None // nothing to do
            }
            ComponentSection::ComponentStartSection |
            ComponentSection::CustomSection => return None // nothing to do for custom or start sections
        };
        Some(space)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct IdxSpace {
    /// The name of this index space (primarily for debugging purposes)
    name: String,
    /// This is the current ID that we've reached associated with this index space.
    current_id: usize,
    // TODO: we might not need the below if we just track the current_id
    //       at both parse and instrument time!
    // /// This represents the number of items from the main vector that
    // /// contribute to this index space.
    // /// (e.g. the number of (type ...) items we've encountered for the component type index space.)
    // num_main: usize,
    // /// This represents the number of external structures that contribute to
    // /// the current ID
    // /// (e.g. component type indices come from the (type ...) AND the (export ...) expressions
    // num_external: usize,

    /// This is used at encode time. It tracks the actual ID that has been assigned
    /// to some item by allowing for lookup of the assumed ID: `assumed_id -> actual_id`
    /// This is important since we know what ID should be associated with something only at encode time,
    /// since instrumentation has finished at that point and encoding of component items
    /// can be done out-of-order to satisfy possible forward-references injected during instrumentation.
    actual_ids: HashMap<usize, usize>,

    /// Tracks the index in the MAIN item vector to the ID we've assumed for it: `main_idx -> assumed_id`
    /// This ID will be used to reference that item in the IR.
    main_assumed_ids: HashMap<usize, usize>,

    // The below maps are to track assumed IDs for item vectors that index into this index space.

    /// Tracks the index in the ALIAS item vector to the ID we've assumed for it: `alias_idx -> assumed_id`
    /// This ID will be used to reference that item in the IR.
    alias_assumed_ids: HashMap<usize, usize>,
    /// Tracks the index in the IMPORT item vector to the ID we've assumed for it: `imports_idx -> assumed_id`
    /// This ID will be used to reference that item in the IR.
    imports_assumed_ids: HashMap<usize, usize>,
    /// Tracks the index in the EXPORT item vector to the ID we've assumed for it: `exports_idx -> assumed_id`
    /// This ID will be used to reference that item in the IR.
    exports_assumed_ids: HashMap<usize, usize>,
}
impl IdxSpace {
    pub fn new(name: String) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }

    pub fn reset_ids(&mut self) {
        self.current_id = 0;
    }

    pub fn curr_id(&self) -> usize {
        // This returns the ID that we've reached thus far while encoding
        self.current_id
    }

    pub fn assign_actual_id(&mut self, assumed_id: usize) {
        let id = self.curr_id();
        println!("[{}] assigning {} to {}", self.name, assumed_id, id);

        self.actual_ids.insert(assumed_id, id);
        self.next();
    }

    fn next(&mut self) -> usize {
        println!("[{}] {} >> {}", self.name, self.current_id, self.current_id + 1);
        let curr = self.current_id;
        self.current_id += 1;
        curr
    }

    pub fn lookup_assumed_id(&self, section: &ComponentSection, vec_idx: usize) -> Option<&usize> {
        let (group, vector) = match section {
            ComponentSection::ComponentImport => ("imports", &self.imports_assumed_ids),
            ComponentSection::ComponentExport => ("exports", &self.exports_assumed_ids),
            ComponentSection::Alias => ("aliases", &self.alias_assumed_ids),

            ComponentSection::Module |
            ComponentSection::CoreType |
            ComponentSection::ComponentType |
            ComponentSection::CoreInstance |
            ComponentSection::ComponentInstance |
            ComponentSection::Canon |
            ComponentSection::CustomSection |
            ComponentSection::Component |
            ComponentSection::ComponentStartSection => ("main", &self.main_assumed_ids)
        };

        let assumed = vector.get(&vec_idx);

        println!("[{}::{group}] idx: {}, assumed_id: {}", self.name, vec_idx, if let Some(a) = assumed {
            &format!("{}", a)
        } else {
            "none"
        });
        assumed
    }

    pub fn index_from_assumed_id(&self, section: &ComponentSection, assumed_id: usize) -> Option<(SpaceSubtype, usize)> {
        let (subty, map) = match section {
            ComponentSection::ComponentImport => (SpaceSubtype::Import, &self.imports_assumed_ids),
            ComponentSection::ComponentExport => (SpaceSubtype::Export, &self.exports_assumed_ids),
            ComponentSection::Alias => (SpaceSubtype::Alias, &self.alias_assumed_ids),

            ComponentSection::Module |
            ComponentSection::CoreType |
            ComponentSection::ComponentType |
            ComponentSection::CoreInstance |
            ComponentSection::ComponentInstance |
            ComponentSection::Canon |
            ComponentSection::CustomSection |
            ComponentSection::Component |
            ComponentSection::ComponentStartSection => (SpaceSubtype::Main, &self.main_assumed_ids)
        };

        for (idx, assumed) in map.iter() {
            // println!("{group} checking: {} -> {}", idx, assumed);
            if *assumed == assumed_id {
                return Some((subty, *idx));
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

            ComponentSection::Module |
            ComponentSection::CoreType |
            ComponentSection::ComponentType |
            ComponentSection::CoreInstance |
            ComponentSection::ComponentInstance |
            ComponentSection::Canon |
            ComponentSection::CustomSection |
            ComponentSection::Component |
            ComponentSection::ComponentStartSection => &mut self.main_assumed_ids
        };
        // println!("[{}] idx: {}, assumed_id: {}", self.name, vec_idx, assumed_id);
        to_update.insert(vec_idx, assumed_id);

        assumed_id
    }

    pub fn is_encoded(&self, assumed_id: usize) -> bool {
        self.actual_ids.contains_key(&assumed_id)
    }

    pub fn lookup_actual_id(&self, id: usize) -> Option<&usize> {
        // account for the zero-based indexing
        // if let Some(to) = self.map.get(&(id + 1)) {
        // if let Some(to) = self.map.get(&(id)) {
        //     *to
        // } else {
        //     panic!("[{}] Can't find id {} in id-tracker...current: {}", self.name, id, self.current);
        // }
        let res = self.actual_ids.get(&id);
        println!("[{}] actual id for {}?? --> {:?}", self.name, id, res);

        res
    }
}

pub(crate) enum SpaceSubtype {
    Export,
    Import,
    Alias,
    Main
}

#[derive(Debug)]
pub(crate) enum ExternalItemKind {
    // Component-level spaces
    CompFunc,
    CompVal,
    CompType,
    CompInst,
    Comp,

    // Core space (added by component model)
    CoreInst,
    Module,

    // Core spaces that exist at the component-level
    CoreType,
    CoreFunc,

    // Does not impact an index space
    NA
}

impl From<&ComponentTypeRef> for ExternalItemKind {
    fn from(value: &ComponentTypeRef) -> Self {
        match value {
            ComponentTypeRef::Module(_) => Self::Module,
            ComponentTypeRef::Func(_) => Self::CompFunc,
            ComponentTypeRef::Value(_) => Self::CompVal,
            ComponentTypeRef::Type(_) => Self::CompType,
            ComponentTypeRef::Instance(_) => Self::CompInst,
            ComponentTypeRef::Component(_) => Self::Comp
        }
    }
}
impl From<&ExternalKind> for ExternalItemKind {
    fn from(value: &ExternalKind) -> Self {
        match value {
            ExternalKind::Func => ExternalItemKind::CoreFunc,
            ExternalKind::Table |
            ExternalKind::Memory |
            ExternalKind::Global |
            ExternalKind::Tag => todo!("I have no idea what to do for this")
        }
    }
}
impl From<&ComponentExternalKind> for ExternalItemKind {
    fn from(value: &ComponentExternalKind) -> Self {
        match value {
            ComponentExternalKind::Module => Self::Module,
            ComponentExternalKind::Func => Self::CompFunc,
            ComponentExternalKind::Value => Self::CompVal,
            ComponentExternalKind::Type => Self::CompType,
            ComponentExternalKind::Instance => Self::CompInst,
            ComponentExternalKind::Component => Self::Comp
        }
    }
}
impl From<&Option<ComponentTypeRef>> for ExternalItemKind {
    fn from(value: &Option<ComponentTypeRef>) -> Self {
        if let Some(value) = value {
            Self::from(value)
        } else {
            Self::NA
        }
    }
}
impl From<&ComponentAlias<'_>> for ExternalItemKind {
    fn from(value: &ComponentAlias) -> Self {
        match value {
            ComponentAlias::InstanceExport { kind, .. } => match kind {
                ComponentExternalKind::Module => Self::Module,
                ComponentExternalKind::Func => {
                    println!("Assigned to comp-func");
                    Self::CompFunc
                },
                ComponentExternalKind::Value => Self::CompVal,
                ComponentExternalKind::Type => {
                    println!("Assigned to comp-type");
                    Self::CompType
                },
                ComponentExternalKind::Instance => Self::CompInst,
                ComponentExternalKind::Component => Self::Comp
            },
            ComponentAlias::Outer { kind, .. } => match kind {
                ComponentOuterAliasKind::CoreModule => Self::Module,
                ComponentOuterAliasKind::CoreType => Self::CoreType,
                ComponentOuterAliasKind::Type => Self::CompType,
                ComponentOuterAliasKind::Component => Self::Comp
            },
            ComponentAlias::CoreInstanceExport { kind, .. } => {
                match kind {
                    ExternalKind::Func => {
                        println!("[CoreInstanceExport] Assigned to core-func");
                        Self::CoreFunc
                    },
                    ExternalKind::Table |
                    ExternalKind::Memory |
                    ExternalKind::Global |
                    ExternalKind::Tag => {
                        println!("[CoreInstanceExport] Assigned to core-type");
                        Self::CoreType
                    },
                }
            }
        }
    }
}
