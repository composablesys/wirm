use crate::ir::id::CanonicalFuncId;
use wasmparser::CanonicalFunction;

#[derive(Debug, Default)]
pub struct Canons {
    pub items: Vec<CanonicalFunction>,

    pub(crate) num_lift_lower: usize,
    num_lift_lower_added: usize,
}
impl Canons {
    pub fn new(items: Vec<CanonicalFunction>) -> Self {
        let mut num_lift_lower = 0;
        for i in items.iter() {
            if matches!(
                i,
                CanonicalFunction::Lift { .. } | CanonicalFunction::Lower { .. }
            ) {
                num_lift_lower += 1;
            }
        }

        Self {
            items,
            num_lift_lower,
            ..Self::default()
        }
    }

    /// Add a new canonical function to the component.
    pub(crate) fn add(&mut self, canon: CanonicalFunction) -> (u32, CanonicalFuncId) {
        let fid = self.items.len() as u32;
        let fid_inner = match canon {
            CanonicalFunction::Lift { .. } | CanonicalFunction::Lower { .. } => {
                self.num_lift_lower += 1;
                self.num_lift_lower_added += 1;

                self.num_lift_lower - 1
            }
            _ => todo!(
                "Haven't implemented support to add this canonical function type yet: {:#?}",
                canon
            ),
        };
        self.items.push(canon);

        (fid_inner as u32, CanonicalFuncId(fid))
    }
}
