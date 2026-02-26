use crate::encode::component::assign::assign_indices;
use crate::encode::component::encode::encode_internal;
use crate::ir::component::visitor::events_topological::get_topological_events;
use crate::ir::component::visitor::VisitCtx;
use crate::ir::types;
use crate::Component;

mod assign;
pub(crate) mod encode;
mod fix_indices;

/// Encode this component into its binary WebAssembly representation.
///
/// # Overview
///
/// Encoding proceeds in **three distinct phases**:
///
/// 1. **Collect**
/// 2. **Assign**
/// 3. **Encode**
///
/// These phases exist to decouple *traversal*, *index computation*, and
/// *binary emission*, which is required because instrumentation may insert,
/// reorder, or otherwise affect items that participate in index spaces.
///
/// At a high level:
///
/// - **Collect** walks the IR and records *what* needs to be encoded and
///   *where* indices are referenced.
/// - **Assign** resolves all logical references to their final concrete
///   indices after instrumentation.
/// - **Encode** emits the final binary using the resolved indices.
///
/// ---
///
/// # Phase 1: Collect
///
/// The collect phase performs a structured traversal of the component IR.
/// During this traversal:
///
/// - All items that participate in encoding are recorded into an internal
///   "plan" that determines section order and contents.
/// - Any IR node that *references indices* (e.g. types, instances, modules,
///   exports) is recorded along with enough context to later rewrite those
///   references.
///
/// No indices are rewritten during this phase.
///
/// ## Component ID Stack
///
/// Components may be arbitrarily nested. To correctly associate items with
/// their owning component, the encoder maintains a **stack of component IDs**
/// during traversal.
///
/// - When entering a component, its `ComponentId` is pushed onto the stack.
/// - When exiting, it is popped.
/// - The top of the stack always represents the *current component context*.
///
/// A **component registry** maps `ComponentId` → `&Component`, allowing the
/// encoder to recover the owning component at any point without relying on
/// pointer identity for components themselves.
///
/// ---
///
/// # Phase 2: Assign
///
/// The assign phase resolves all *logical* references recorded during collect
/// into *concrete* indices suitable for encoding.
///
/// This includes:
///
/// - Mapping original indices to their post-instrumentation positions
/// - Resolving cross-item references (e.g. type references inside signatures)
/// - Ensuring index spaces are internally consistent
///
/// ## Scope Stack
///
/// Many IR nodes are scoped (for example, nested types, instances, or
/// component-local definitions). During traversal, the encoder maintains a
/// **stack of scopes** representing the current lexical and component nesting.
///
/// - Entering a scoped node pushes a new scope.
/// - Exiting the node pops the scope.
/// - At any point, the scope stack represents the active lookup context.
///
/// ## Scope Registry
///
/// Because scoped nodes may be deeply nested and arbitrarily structured,
/// scopes are not looked up via traversal position alone.
///
/// Instead, the encoder uses a **scope registry**, which maps the *identity*
/// of an IR node to its associated scope.
///
/// - Scoped IR nodes are stored behind stable pointers (e.g. `Box<T>`).
/// - These pointers are registered exactly once when the IR is built.
/// - During assign, any IR node can query the registry to recover its scope
///   in O(1) time.
///
/// This allows index resolution to be:
///
/// - Independent of traversal order
/// - Robust to instrumentation
/// - Safe against reordering or insertion of unrelated nodes
///
/// ---
///
/// # Phase 3: Encode
///
/// Once all indices have been assigned, the encode phase performs a final
/// traversal and emits the binary representation.
///
/// At this point:
///
/// - No structural mutations occur
/// - All index values are final
/// - Encoding is a pure, deterministic process
///
/// The encoder follows the previously constructed plan to emit sections
/// in the correct order and format.
///
/// ---
///
/// # Design Notes
///
/// This three-phase architecture ensures that:
///
/// - Instrumentation can freely insert or modify IR before encoding
/// - Index correctness is guaranteed before any bytes are emitted
/// - Encoding logic remains simple and local
///
/// The use of component IDs, scope stacks, and a scope registry allows the
/// encoder to handle arbitrarily nested components and scoped definitions
/// without relying on fragile positional assumptions.
///
/// # Panics
///
/// This method may panic if:
///
/// - The component registry is inconsistent
/// - A scoped IR node is missing from the scope registry
/// - Index resolution encounters an unresolved reference
///
/// These conditions indicate an internal bug or invalid IR construction.
pub fn encode(comp: &Component) -> types::Result<Vec<u8>> {
    // Phase 1: Collect
    // NOTE: I'm directly calling get_topological_events to avoid generating
    // the events 2x (one per visitor used during assign and encode)
    let mut events = Vec::new();
    get_topological_events(comp, &mut VisitCtx::new(comp), &mut events);

    // Phase 2: Assign indices
    let ids = assign_indices(&mut VisitCtx::new(comp), &events);

    // Phase 3: Encode (pass in the root-level component's plan, assigned indices, and original->new index map)
    let bytes = encode_internal(&ids, &mut VisitCtx::new(comp), &events)?;

    // Reset the index stores for any future visits!
    Ok(bytes.finish())
}
