//! Trait that defines general injection behavior

use crate::ir::id::LocalID;
use crate::DataType;

pub trait AddLocal {
    /// Adds a local and in the current function and returns its ID
    /// note: the implementors of this trait will keep track of the location,
    /// i.e. which function, where this local is added
    fn add_local(&mut self, ty: DataType) -> LocalID;
}
