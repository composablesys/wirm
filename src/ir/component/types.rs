use crate::ir::id::ComponentTypeId;
use wasmparser::ComponentType;

#[derive(Debug, Default)]
pub struct ComponentTypes<'a> {
    pub items: Vec<ComponentType<'a>>,

    num_funcs: usize,
    num_funcs_added: usize,
    num_instances: usize,
    num_instances_added: usize,
    num_defined: usize,
    num_defined_added: usize,
    num_components: usize,
    num_components_added: usize,
    num_resources: usize,
    num_resources_added: usize,
}
impl<'a> ComponentTypes<'a> {
    pub fn new(items: Vec<ComponentType<'a>>) -> Self {
        let (
            mut num_funcs,
            mut num_instances,
            mut num_defined,
            mut num_components,
            mut num_resources,
        ) = (0, 0, 0, 0, 0);
        for i in items.iter() {
            match i {
                ComponentType::Func(_) => num_funcs += 1,
                ComponentType::Instance(_) => num_instances += 1,
                ComponentType::Defined(_) => num_defined += 1,
                ComponentType::Component(_) => num_components += 1,
                ComponentType::Resource { .. } => num_resources += 1,
            }
        }

        Self {
            items,
            num_funcs,
            num_instances,
            num_defined,
            num_components,
            num_resources,
            ..Self::default()
        }
    }

    /// Add a new component type to the component.
    pub(crate) fn add<'b>(&'b mut self, ty: ComponentType<'a>) -> (u32, ComponentTypeId) {
        let ty_id = self.items.len();
        let ty_inner_id = match ty {
            ComponentType::Defined(_) => {
                self.num_defined += 1;
                self.num_defined_added += 1;

                self.num_defined - 1
            }
            ComponentType::Func(_) => {
                self.num_funcs += 1;
                self.num_funcs_added += 1;

                self.num_funcs - 1
            }
            ComponentType::Component(_) => {
                self.num_components += 1;
                self.num_components_added += 1;

                self.num_components - 1
            }
            ComponentType::Instance(_) => {
                self.num_instances += 1;
                self.num_instances_added += 1;

                self.num_instances - 1
            }
            ComponentType::Resource { .. } => {
                self.num_resources += 1;
                self.num_resources_added += 1;

                self.num_resources - 1
            }
        };

        self.items.push(ty);
        (ty_inner_id as u32, ComponentTypeId(ty_id as u32))
    }
}
