# DWARF Debug-Info Rewriting

Design doc for issue #169. The goal: when wirm instruments a Wasm
module that carries DWARF debug info, the output module's DWARF must
remain coherent — original instructions still map to their original
`(file, line, col)`; injected instructions inherit their anchor's
`(file, line, col)` so a debugger never "stops inside" wirm-injected
code.

## TODO

Work through these in order. Each box is roughly one PR-sized unit.

- [x] **0. Validate the testing approach.** Spike confirmed the design
      is viable; findings recorded under "Spike findings (step 0)"
      below.
- [x] **1. Parse-aside.** Add `gimli` dep. Add opt-in flag (`with_dwarf`
      on `Module::parse`). When set, recognize `.debug_*` custom
      sections and pull them into `ModuleDebugData` (currently a stub
      at `src/ir/dwarf.rs`). All other sections continue to flow
      through `custom_sections` unchanged. No rewriting yet — encode
      should round-trip the parsed-aside DWARF byte-identically.
- [x] **2. Capture new per-op PCs during encode.** Modify
      `encode_function` (`src/ir/module/mod.rs:1202`) to record the
      `wasm_encoder::Function::byte_len()` after each emitted op,
      producing a `Vec<usize>` of new in-function PCs aligned with the
      emitted op order. Gated on the same opt-in flag.
- [x] **3. Track new code-section start.** During `encode_internal`,
      record the byte offset where the code section begins in the
      output (header bytes + sizes of preceding sections). DWARF PCs
      are relative to the code section start.
- [x] **4. Post-resolve anchor-walk.** After
      `resolve_special_instrumentation`, walk every function's
      `InstrumentationFlag.{before, after, alternate}` and produce
      `Vec<(emit_order_idx, anchor_instr_idx)>` per function. Original
      ops map to themselves; injected ops map to their host
      instruction.
- [x] **5. `.debug_line` rewriter.** Walk the input's line program;
      emit a new program where each original PC is translated via the
      maps from steps 2–4. For every injected PC, emit a row with the
      anchor's `(file, line, col, is_stmt, ...)`.
- [x] **6. `.debug_info` and friends.** Use
      `gimli::write::Dwarf::from(&read, &addr_translator)` with an
      address translator that consults the same maps. This handles
      `DW_AT_low_pc`/`high_pc`, `DW_AT_ranges`, location lists,
      aranges, frame info — anything that holds a PC.
- [x] **7. Re-emit DWARF as custom sections.** Encoded DWARF goes into
      the output module's custom sections in the conventional order.
- [x] **8. Unit/regression tests.** Five or six hand-written `.wat`
      inputs run through `wasm-tools parse --generate-dwarf full`,
      with hand-checked expectations. Cover: locals added, nop
      injected before every op, replacement, block_alt, multi-function
      module, `func_exit` cloned to multiple return sites. Inputs must
      be designed to *expose* miscoherence — see "Spike findings" — so
      prefer one row per opcode, distinct (line, col) per opcode, and
      either multi-byte injections or enough cumulative shift that no
      original op stays inside its old line-program range.
- [x] **9. Differential test.** Two offset-discovery paths
      (in-encode capture vs. re-parse-after-encode), assert
      byte-identical maps. Catches off-by-ones in step 2.
- [x] **10. Property test.** `proptest` over (small dwarf-bearing
       corpus) × (random instrumentation plans). Single invariant:
       `lookup(new_pc)` in output equals `lookup(anchor_orig_pc)` in
       input, for every emitted op.
- [x] **12. libfuzzer target (corpus-based).** Reuse the parse-aside
       seeds from steps 8/11; let libfuzzer generate the
       instrumentation plan via `Arbitrary`.
- [ ] **13. libfuzzer target (synthesized DWARF).** Helper that emits
       a stub `.debug_line` on top of `wasm-smith` output so we get
       unbounded variation. Optional, deferred until earlier layers
       are stable. *Deferred alongside step 11: wasm-smith outputs are
       multi-function, which step 6 refuses, so most cases would hit
       the refusal path. Interesting features (loc lists, ranges,
       inlined subroutines) are blocked behind multi-function support.
       Action-space variation is already covered by step 10 proptest +
       step 12 libfuzzer. Revisit alongside step 11 once multi-CU
       routing lands.*
- [x] **14. Warn on adjacent debug sections.** When DWARF rewriting is
       opted in, detect `external_debug_info` and `sourceMappingURL`
       custom sections during parse and emit a `log::warn!` (the `log`
       crate is already a dep). The user opted into DWARF rewriting,
       so they care about debug info coherence — they need to know
       these sections exist and that we won't fix them. See "Adjacent
       conventions" below.

## Decisions already made

These are settled — recorded here so we don't relitigate.

- **Anchor inheritance policy.** Every instruction injected as
  `Before`/`After`/`Alternate` on a target op (whether attached
  directly or expanded from a special mode) inherits that target op's
  `(file, line, col)`. A debugger stepping through the rewritten
  module never sees a stop "inside" wirm-injected code — instrumentation
  appears to belong to the adjacent original instruction.
- **Anchor mapping happens *after* `resolve_special_instrumentation`.**
  By that point every injection is pinned to a specific target
  instruction via `InstrumentationFlag.{before, after, alternate}`,
  regardless of whether it started as a plain mode or a special one
  (`SemanticAfter`, `BlockEntry`, `BlockExit`, `BlockAlt`, func-entry,
  func-exit). A single uniform walk over the resolved layout
  produces the anchor map. Doing it inside resolve would either miss
  the plain modes or duplicate the bookkeeping.
- **Opt-in.** DWARF rewriting adds `gimli` parse/encode work and
  per-op offset capture. Both have nontrivial cost. Gated behind a
  `with_dwarf: bool` flag on `Module::parse` (sibling of
  `with_offsets`). Default is off.
- **For func-exit clones**, the natural anchor is the local op the
  clone was pinned to: a `return` for return-site clones, the
  function's last op for the implicit fall-through clone. Same
  principle for any other special mode that fans out.

## Current-state notes

- `src/ir/dwarf.rs` defines `ModuleDebugData`, which holds the
  `.debug_*` custom sections lifted aside at parse time. `gimli` 0.33
  is a dependency; `ir/mod.rs` declares the module.
- DWARF sections pass through opaquely by default. When `Module::parse`
  is called with `with_dwarf = true`, the custom-section dispatch in
  `parse_internal` (`src/ir/module/mod.rs:484`) diverts `.debug_*` into
  `Module::debug` instead of `custom_sections`, and `encode_internal`
  runs the rewriter before re-emitting them at the tail of the
  custom-section run. The rewriter has two halves:
  - `src/ir/dwarf.rs::rewrite_debug_line` walks the input line program
    via gimli and emits a new one with anchor-aware row inheritance —
    injected ops carry their host op's `(file, line, col)` so a
    debugger never stops "inside" wirm-injected code.
  - `src/ir/dwarf.rs::rewrite_other_dwarf_sections` runs `.debug_info`
    and friends through `gimli::write::Dwarf::convert` with a per-DIE
    attribute walk (`convert_die`) that intercepts `DW_AT_low_pc` and
    `DW_AT_high_pc` stored as `DW_FORM_data*`/`DW_FORM_udata`. Gimli's
    default convert passes constant-form attrs through verbatim, which
    would leave length attributes stale after instrumentation; the
    custom walk fixes that.
  Step 7 re-emits the rewritten sections in input order, appending any
  sections gimli generated that the input didn't carry (e.g.
  `.debug_line_str` for v5 path tables) at the tail.
- Adjacent debug-info channels — `external_debug_info` and
  `sourceMappingURL` custom sections — are detected at parse time
  (step 14) and surface as `log::warn!` when `with_dwarf` is on. wirm
  passes them through unchanged because they point at out-of-module
  content (a URL to a side-file, a URL to a source map) that requires
  separate tooling to rewrite.
- The "original instruction → original PC" half is done.
  `Module::parse(_, _, with_offsets, _)` threads through to
  `Instructions::new` (`src/ir/types.rs:1578`), which records
  `offset - locals_start` per op. Empirically `locals_start` lands on
  the first instruction's offset (the entire locals declaration is
  excluded), so op 0 sits at PC 0. Exposed via
  `Instructions::lookup_pc_offset_for`.
- The "emit-order → new PC" half is also done. `encode_internal`
  captures per-local-function `Vec<usize>` of start offsets (gated on
  `Module::debug.is_some()`) and returns it as the third tuple
  element. Capture state lives in a `PcCapture` threaded through
  `encode_function`'s helpers; PCs are rebased onto the first
  instruction so they share the parse-side convention directly.
- Special-mode resolution lives in
  `Module::resolve_special_instrumentation`
  (`src/ir/module/mod.rs:758`). It expands
  `SemanticAfter`/`BlockEntry`/`BlockExit`/`BlockAlt` plus func-entry
  and func-exit into concrete `Before`/`After`/`Alternate` injections
  pinned to specific target instructions. The anchor walk runs after
  this returns.

## Maps the rewriter operates on

For each local function:

- `orig_instr_idx → orig_pc_in_func`: from `with_offsets`-tracked
  parse-time offsets, accounting for locals-bytes subtraction
  (already done in `Instructions::new`).
- `emit_order_idx → new_pc_in_func`: from per-op `byte_len()` capture
  in `encode_function`.
- `emit_order_idx → anchor_instr_idx`: from the post-resolve walk.
  Trivially the identity for original ops.

Module-level:

- `orig_code_section_start`: known from parse.
- `new_code_section_start`: tracked during encode.
- `orig_func_start_in_code_section`, `new_func_start_in_code_section`:
  per-function. Computed from cumulative function body sizes plus the
  function-body-size LEB128 length.

Together these give: `orig_pc_in_module → new_pc_in_module` for every
original instruction, and `injected_new_pc → anchor_orig_pc` for every
injected op.

## Sections handled by the rewrite

PC-bearing (must rewrite):

- `.debug_line` — line-number program. Walk and rebuild against the
  new PC map.
- `.debug_info` — DIEs with `DW_AT_low_pc`, `DW_AT_high_pc`,
  `DW_AT_ranges`, location-list references.
- `.debug_loc`, `.debug_loclists` — location lists.
- `.debug_ranges`, `.debug_rnglists` — range lists.
- `.debug_aranges` — address-range lookup.
- `.debug_frame` — call-frame info.

Pass-through (no PCs):

- `.debug_abbrev`, `.debug_str`, `.debug_line_str`, `.debug_str_offsets`,
  `.debug_pubnames`, `.debug_pubtypes`.

`gimli::write::Dwarf::from(&read, &translator)` covers the bulk of the
PC-bearing sections; we hand-roll `.debug_line` because the inheritance
logic for injected ops is custom.

## Phase 2: known gaps

Phase 1 (TODO items above marked `[x]`) implements the rewriter for
inputs that match the wasm-tools `--generate-dwarf full` convention:
single CU, one local function, `DW_FORM_data*` constant `low_pc`/
`high_pc`, addresses confined to one function's body. That covers the
test fixtures in `tests/test_inputs/handwritten/dwarf/`. Phase 2 is
what's needed for the rewriter to be robust on real compiler output
(rustc / clang debug builds). Steps 11 and 13 from the TODO list above
are part of Phase 2 — they're deferred precisely because the items
below block them.

### Lift the multi-function `.debug_info` gate — DONE

Lifted by routing addresses through a module-cumulative `dwarf_addr_base`
carried on per-function metadata. Concretely:

- `OrigFuncDebugData` (`src/ir/dwarf.rs`) and `FuncDwarfMaps` /
  `PerFuncEncodeMaps` all carry `dwarf_addr_base: usize`. Computed in a
  serial pass after per-body parse (parse side) and after the per-function
  encode loop (encode side), in both cases as a running sum of preceding
  `body_total_size`.
- `rewrite_other_dwarf_sections` builds a sorted route table and uses
  `partition_point` to find the last function whose `dwarf_addr_base ≤
  orig_addr`. That covers in-body addresses and routes one-past-end
  addresses (CU.high_pc, sentinels) to the function they extend from.
- `rewrite_debug_line`'s sequence begin/end now use module-cumulative
  addresses (`begin_sequence(Address::Constant(dwarf_addr_base))`,
  `end_sequence(body_total_size)`), and the orig PC lookup strips
  `dwarf_addr_base + first_instr_dwarf_offset`.
- `assert_source_location_invariant` (`src/ir/module/test.rs`) now
  composes `dwarf_addr_base + first_instr_dwarf_offset` for both orig and
  new lookups so the strong invariant holds across functions.
- Tests: `rewriter_refuses_multi_function_input` retired; replaced by
  `rewriter_preserves_row_addresses_uninstrumented_multi_func`,
  `rewriter_handles_instrumentation_in_{first,second}_function` (in
  `tests/dwarf.rs`) and a multi-function proptest variant. The libfuzzer
  target (`fuzz/fuzz_targets/dwarf_invariant.rs`) takes a fixture-selector
  byte so the corpus reaches both seeds.

**Fixture address layout** (verified empirically, plan text was slightly
off): `two_funcs.wasm` has foo at module addresses `[0, 8)` and bar at
`[8, 16)`. Bar's `dwarf_addr_base = 8` (= foo's `body_total_size`); its
subprogram `low_pc = 9` (skipping the size-LEB and locals bytes).

**Still untested on real rustc/clang output.** Inlined functions
(`DW_TAG_inlined_subroutine`), modules split across multiple CUs, and
`DW_FORM_ref_addr` cross-CU references aren't exercised by our fixtures.
`gimli::write::Dwarf::convert` should handle cross-CU refs but it isn't
tested. Verify on real binaries — alongside step 11 below.

### `.debug_aranges` dropped; `.debug_str_offsets` still passes through

**`.debug_aranges`** — `gimli::write::Sections` doesn't expose this
field, so the rewriter can't produce coherent replacement bytes. We now
**drop** the section under `with_dwarf` (see the re-emit loop in
`encode_internal`) rather than pass stale bytes through. Stale aranges
are worse than missing aranges — debuggers happily fall back to scanning
`.debug_info` when aranges are absent, but trust them when present.

Follow-up if a real input needs an accelerated aranges table: hand-roll
a builder. The format is small (per-CU header + sorted `(address, length)`
pairs + terminator); the per-function `dwarf_addr_base` /
`body_total_size` carried by `OrigFuncDebugData` is exactly the input.

**`.debug_str_offsets`** — also missing from `gimli::write::Sections`,
but it's not PC-bearing; addresses don't go stale under instrumentation.
Pass-through is correct, just not currently re-canonicalized through
gimli. Leave as-is until a real input demands attention.

### `DW_AT_stmt_list` offset coincidence

`gimli::write::Dwarf::convert` writes its (discarded) per-unit line
program at `.debug_line` offset 0, and our step-5 rewriter ALSO writes
at offset 0. DIE `DW_AT_stmt_list` attributes inherit gimli's choice
and happen to point at the right byte today.

Multi-program inputs (multi-CU) would break this: gimli's per-CU
programs would be at offsets `0, N1, N2, …` while step-5's rewriter
produces one bundle starting at 0. Fix when multi-function lands:
either capture gimli's per-unit offsets and mirror them in the
step-5 output, or rewrite the DIE `DW_AT_stmt_list` values to match
the step-5 layout.

### Adjacent debug-info channels

Today's `log::warn!` behavior (step 14) is the conservative default.
When a real input demands more, pick from:

- **`external_debug_info`:** refuse / fetch+rewrite+republish the side
  file / strip the section.
- **`sourceMappingURL`:** strictly out of DWARF scope (different
  format). Could be a sibling feature — a source-map rewriter using
  the same per-function PC maps the DWARF rewriter computes. Source
  maps are byte-offset-based, so the math is identical to step 5's.

### DWARF v4 vs v5 — DONE for rustc-shape inputs

The `tests/test_inputs/handwritten/dwarf/from-rust/` fixture is rustc
debug output (DWARF v4, multi-function, inlined subroutines, rangelist
CU, `DW_FORM_addr` low/high_pc, `dead code` low_pc tombstones). The
strong invariant holds on it (`rewriter_handles_from_rust_fixture_*`
in `src/ir/module/test.rs`).

Surfacing this fixture caught one real rewriter bug: the input-row
lookup used `HashMap<orig_pc, row>` exact match, which dropped rows
whose orig PC sat *between* instruction starts (rustc emits these
freely; wasm-tools' synthetic DWARF doesn't). Replaced with a
`partition_point` lookup that picks "the row whose PC is the largest
≤ query PC" — same semantics as `lookup_src_at` in the test helpers.

Past-end and mid-instruction *output* rows are still not preserved (the
rewriter emits one row per emit position, not one per input row). This
doesn't affect the strong invariant — emit positions are always at
instruction starts, so `lookup(new_pc) == lookup(anchor_orig_pc)` holds
— but the output's `.debug_line` lookups in mid-instruction or
past-end address ranges may disagree with the input's. Real debuggers
never stop in those ranges (wasm instructions are atomic), so this is
documented scope, not a defect.

Clang-shape inputs remain untested.

## Components

DWARF in Wasm is a core-module-level convention. The
[tool-conventions Debugging.md](https://github.com/WebAssembly/tool-conventions/blob/main/Debugging.md)
defines DWARF embedding only for modules — `.debug_*` custom sections
inside a Wasm module, with addresses interpreted as offsets into that
module's code section. The component-model spec does not define a
component-level DWARF convention, and there's no in-flight proposal
for one (verified May 2026).

For components: each embedded core module carries its own `.debug_*`
custom sections independently. wirm's component path
(`src/ir/component`) should, when DWARF rewriting is enabled, recurse
into each contained core module and apply the module-level rewriter.
Cross-module glue (lifts/lowers/canonical-ABI shims) is component
machinery, not user code — debuggers don't care about source lines
for it, so there's nothing to translate at the component level
itself.

Component support is additive: do the module path first, then call
into it from the component path.

## Adjacent conventions (out of scope, but related)

Both are core-module-level, both currently flow through wirm's
`custom_sections` opaquely, and both have the same
"addresses-go-stale-after-instrumentation" problem. Both are
detectable by custom-section name during parse — when DWARF rewriting
is opted in we should `log::warn!` so the user knows their debug info
is partially incoherent (see TODO #14). The user opted in *because*
they care about debug coherence; silently leaving these stale would
be a footgun.

- `external_debug_info` custom section: holds a URL pointing to a
  side-file containing the DWARF. If the input uses this, the main
  module has no `.debug_*` sections to rewrite — but the side-file is
  now incoherent with the rewritten module. Options when detected:
  (a) refuse to rewrite when this section is present, (b) fetch +
  rewrite + re-emit the side-file, (c) strip the section. For now:
  warn loudly and pass through. Decide between (a)/(b)/(c) when a
  real input forces the question.
- `sourceMappingURL` custom section: source-map-based debugging.
  Source maps are byte-offset-based, so they go stale identically to
  DWARF. Different format (JSON), separate rewriter. Out of scope for
  issue #169 — but warn so the user knows their source map is
  pointing at the wrong bytes after instrumentation.

## Spike findings (step 0)

Spike lived in `/tmp/dwarf-spike{,-rt}` (throwaway). Pipeline verified:
`wasm-tools parse --generate-dwarf full` on a hand-written `.wat`
produces a module with `.debug_*` custom sections; `wasmparser`
collects them and `gimli::DwarfSections::load` parses them. `gimli`
0.33 + `wasmparser` 0.247 cooperate without friction on the read side.

Two findings shape the rest of the work:

1. **Row equality is the wrong invariant.** Injecting a single nop
   leaves `.debug_line` byte-identical (we don't rewrite it yet), but
   every original opcode has moved by one byte — the rows now name
   different ops. A naive "input rows == output rows" assertion would
   pass. The right invariant is semantic:
   `output_lookup(new_pc) == input_lookup(anchor_orig_pc)` for every
   emitted op. The test harness in step 8 is built around this, not
   row equality.
2. **The semantic check can still pass by accident on tiny inputs.**
   Line-program rows define implicit ranges (a row's `(file, line,
   col)` is in effect until the next row's address). A small shift can
   land each op inside the *next* row's range, which often has the
   same `(file, line, col)` by coincidence. Regression inputs must be
   designed to break this — distinct `(line, col)` per opcode, line
   programs with one row per opcode, and either multi-byte injections
   or enough cumulative shift that ops escape their original buckets.

Implications for downstream steps:

- The harness can't run automatically on instrumented modules until
  the per-op new-PC map (step 2) and the anchor walk (step 4) exist.
  Step 0's harness only checks the noop baseline automatically; the
  injection case was hand-mocked.
- Step 8 input authoring needs explicit thought, not boilerplate.
- DWARF address convention in wasm-tools-generated output appears to
  be "offset from the function-size LEB", not "offset from the code
  section payload start". Worth pinning down before step 5 starts
  emitting addresses.

## Non-goals

- Generating DWARF for code that didn't have it. If the input has no
  `.debug_*` sections, the output has none.
- Generating DWARF for wirm-injected code. The whole point of the
  inheritance policy is that injected code is invisible to the
  debugger.
- Rewriting `sourceMappingURL` source maps. Tracked above as adjacent.
- Component-level DWARF. No such thing today.
