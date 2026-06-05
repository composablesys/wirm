//! The Intermediate Representation for components and modules.

use std::ops::{Deref, Index, IndexMut};
use std::slice::SliceIndex;

pub mod component;
pub mod dwarf;
pub mod function;
mod helpers;
pub mod id;
#[cfg(test)]
pub mod instr_tests;
pub mod module;
pub mod types;

/// An append-only vector with stable element addresses.
///
/// `AppendOnlyVec<T>` is a wrapper around `Vec<T>` that enforces a single
/// structural invariant: **elements may be appended, but never removed,
/// reordered, or replaced**.
///
/// This type exists to support IR instrumentation and encoding workflows
/// that rely on *stable identity* of nodes over time, typically via raw
/// pointers or addresses registered during parsing.
///
/// # Motivation
///
/// Many parts of the encoding pipeline associate metadata (such as scope
/// IDs or index spaces) with IR nodes using their memory address. If nodes
/// were allowed to be removed, swapped, or compacted, these addresses would
/// become invalid and lead to subtle, hard-to-debug encoding failures.
///
/// `AppendOnlyVec` provides a constrained mutation model that preserves:
///
/// - **Pointer stability** for all elements
/// - **Index stability** for previously appended elements
/// - **Monotonic growth**, which matches how Wasm sections are built
///
/// This allows instrumentation passes to safely mutate nodes *in place*
/// without invalidating previously registered scope or index information.
///
/// # Allowed Operations
///
/// - Appending new elements to the end of the vector
/// - Iterating over elements (immutably or mutably)
/// - Indexing into the vector
///
/// # Disallowed Operations
///
/// `AppendOnlyVec` intentionally does **not** expose APIs for:
///
/// - Removing elements
/// - Inserting elements at arbitrary positions
/// - Reordering or swapping elements
/// - Clearing the vector
///
/// These operations would invalidate assumptions made by the encoder.
///
/// # Relationship to Scopes
///
/// Nodes stored in an `AppendOnlyVec` may be registered in the scope registry
/// using their address. Because elements are never moved or removed, these
/// registrations remain valid for the entire lifetime of the component.
///
/// For nodes that *may* own scopes, this wrapper is commonly used together
/// with `Box<T>` (e.g. `AppendOnlyVec<Box<Node>>`) to ensure heap allocation
/// and stable pointers.
///
/// # Examples
///
/// ```rust
/// use wirm::ir::AppendOnlyVec;
///
/// let mut vec = AppendOnlyVec::from(vec![42, 100]);
/// for v in vec.iter_mut() {
///     *v += 1;
/// }
///
/// assert_eq!(vec[0], 43);
/// assert_eq!(vec[1], 101);
/// ```
///
/// # Design Notes
///
/// `AppendOnlyVec` is a *semantic* restriction, not a performance abstraction.
/// Internally it may still use a `Vec<T>`, but its API enforces invariants
/// required by the encoder.
///
/// If you need more flexibility (e.g. temporary collections during parsing),
/// use a plain `Vec<T>` instead and transfer elements into an `AppendOnlyVec`
/// once they become part of the component’s stable IR.
///
/// # Panics
///
/// This type does not panic on its own, but misuse of raw pointers or
/// assumptions about append-only behavior elsewhere in the system may
/// result in panics during encoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppendOnlyVec<T> {
    vec: Vec<T>,
}
impl<T> Default for AppendOnlyVec<T> {
    fn default() -> Self {
        Self { vec: Vec::new() }
    }
}
impl<T> AppendOnlyVec<T> {
    // INSERTs (only accessible in the crate)
    /// To push an item into the vector. Note that this is not exposed beyond the crate.
    /// This is to protect users from appending IR nodes without them going through
    /// correct preprocessing (scoping, registration, ID assignment).
    /// If there are specific IR nodes that a user needs to be able to add and the library
    /// does not support doing so yet, they should put up a PR on the library's GH repo.
    pub(crate) fn push(&mut self, value: T) {
        self.vec.push(value);
    }
    /// To append a list of items into the vector. Note that this is not exposed beyond the crate.
    /// This is to protect users from appending IR nodes without them going through
    /// correct preprocessing (scoping, registration, ID assignment).
    /// If there are specific IR nodes that a user needs to be able to add and the library
    /// does not support doing so yet, they should put up a PR on the library's GH repo.
    pub(crate) fn append(&mut self, other: &mut Vec<T>) {
        self.vec.append(other);
    }

    /// Must provide our own implementation for `iter_mut` in order to
    /// avoid implementing DerefMut trait (we don't want to expose general
    /// mutation control to users).
    pub fn iter_mut(&'_ mut self) -> core::slice::IterMut<'_, T> {
        self.vec.iter_mut()
    }

    /// We will only ever expose a non-mutable vec here!
    /// Any mutation can only be appending or edit-in-place.
    /// Exposing a mutable vec would allow illegal operations.
    pub fn as_vec(&self) -> &Vec<T> {
        &self.vec
    }

    // no remove, no replace
}
impl<T> Deref for AppendOnlyVec<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        &self.vec
    }
}
impl<T, I> Index<I> for AppendOnlyVec<T>
where
    I: SliceIndex<[T]>,
{
    type Output = I::Output;
    fn index(&self, index: I) -> &Self::Output {
        &self.vec[index]
    }
}
impl<T, I> IndexMut<I> for AppendOnlyVec<T>
where
    I: SliceIndex<[T]>,
{
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        &mut self.vec[index]
    }
}
/// General iterator support.
impl<T> IntoIterator for AppendOnlyVec<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.vec.into_iter()
    }
}
/// Iterator support for references to the vector.
impl<'a, T> IntoIterator for &'a AppendOnlyVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.vec.iter()
    }
}
impl<T> From<Vec<T>> for AppendOnlyVec<T> {
    fn from(vec: Vec<T>) -> Self {
        Self { vec }
    }
}
impl<T> FromIterator<T> for AppendOnlyVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            vec: Vec::from_iter(iter),
        }
    }
}
