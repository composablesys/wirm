use crate::ir::id::AliasId;
use crate::ir::AppendOnlyVec;
use wasmparser::{ComponentAlias, ComponentExternalKind, ExternalKind};

#[derive(Debug, Default)]
pub struct Aliases<'a> {
    pub items: AppendOnlyVec<ComponentAlias<'a>>,

    num_core_funcs: usize,
    num_core_funcs_added: usize,

    num_core_memories: usize,
    num_core_memories_added: usize,

    num_funcs: usize,
    num_funcs_added: usize,
    pub(crate) num_types: usize,
    num_types_added: usize,
}
impl<'a> Aliases<'a> {
    pub fn new(items: AppendOnlyVec<ComponentAlias<'a>>) -> Self {
        let (mut num_core_funcs, mut num_core_memories, mut num_funcs, mut num_types) =
            (0, 0, 0, 0);
        for i in items.iter() {
            match i {
                ComponentAlias::CoreInstanceExport { kind, .. } => match kind {
                    ExternalKind::Func => num_core_funcs += 1,
                    ExternalKind::Memory => num_core_memories += 1,
                    _ => {}
                },
                ComponentAlias::InstanceExport { kind, .. } => match kind {
                    ComponentExternalKind::Type => num_types += 1,
                    ComponentExternalKind::Func => num_funcs += 1,
                    _ => {}
                },
                _ => {}
            }
        }
        Self {
            items,
            num_core_funcs,
            num_core_memories,
            num_funcs,
            num_types,
            ..Self::default()
        }
    }

    pub(crate) fn add(&mut self, alias: ComponentAlias<'a>) -> (u32, AliasId) {
        let ty_id = self.items.len() as u32;
        let ty_inner_id = match alias {
            ComponentAlias::CoreInstanceExport { kind, .. } => match kind {
                ExternalKind::Func => {
                    self.num_core_funcs += 1;
                    self.num_core_funcs_added += 1;

                    self.num_core_funcs - 1
                }
                ExternalKind::Memory => {
                    self.num_core_memories += 1;
                    self.num_core_memories_added += 1;

                    self.num_core_memories - 1
                }
                _ => todo!(),
            },
            ComponentAlias::InstanceExport { kind, .. } => match kind {
                ComponentExternalKind::Type => {
                    self.num_types += 1;
                    self.num_types_added += 1;

                    self.num_types - 1
                }
                ComponentExternalKind::Func => {
                    self.num_funcs += 1;
                    self.num_funcs_added += 1;

                    self.num_funcs - 1
                }

                _ => todo!("haven't supported this yet: {:#?}", kind),
            },
            _ => todo!(),
        };

        self.items.push(alias);
        (ty_inner_id as u32, AliasId(ty_id))
    }
}
