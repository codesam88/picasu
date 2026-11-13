---
status: open
type: feature
priority: medium
area: backend
---

## Goal

Replace the no-op marker attributes with a real annotation crate whose
macros validate operation/event ids at compile time, so a typo'd or
renamed OP id fails `cargo check -p picasu` instead of surfacing only in
`just check`.

## Decisions

- 2026-11-13: the crate lives at `utils/quint`, not in a separate
  `utils/quint-annotations` directory. A `proc-macro = true` crate
  ignores non-Rust files, so the `.qnt` specs, `.mjs` scripts and
  `pipeline/*.json` catalogs stay where they are and the macros read the
  catalogs they validate from their own crate dir
  (`CARGO_MANIFEST_DIR/pipeline`). Attribute path becomes
  `#[quint::operation("OP-N")]`. Known constraint: a proc-macro crate
  cannot export ordinary Rust items; later trace hooks must be
  macro-generated code, and a runtime library facet would need a second
  crate.
- 2026-11-13: rung 0 is compile-time id validation, not a registry. The
  macro parses the attribute argument, checks it against the catalog
  (serde_json), and emits `compile_error!` on an unknown id;
  `build.rs` sets `rerun-if-changed` on the catalog JSONs so catalog
  edits re-expand backend markers. Registry mechanisms
  (`inventory`/`linkme`/build-script registry) stay rung-1 material.
- `operations-check.mjs` remains the catalog-side backstop (unknown OP
  in JSON, model `@operation` tags) until rung 1 decides its fate.

## Current state

- `utils/quint` is now a proc-macro crate (`quint`) that validates
  operation/event ids at expansion time; `utils/quint-annotations` is gone.
- Review findings 2026-11-13 that rung 0 must close:
  - The text scan accepts a comment, doc-comment, string or
    `#[cfg(test)]` mention as a marker, so the README claim
    ("every tracked operation has at least one implementation site",
    README.md:110-113) is not actually enforced. The plan file
    documents the limitation; README oversells it.
  - A marker naming the wrong-but-existing OP id compiles and passes
    (re-mapping the implementation site silently).
  - `quint-annotations` is missing from the `-p` lists in `justfile`
    `utils-format`/`utils-check`/`utils-test`; CI runs `just
utils-check`, so the crate is never fmt/lint/tested.
- `pipeline/operations.json` maps OP-1..OP-12 to events, four-aspect
  reads/writes, guards, status and N nodes; `events.json` carries the
  event names.
- `asset-aspects.qnt` tags new actions with `@operation`; existing
  actions are mostly untagged.

## Steps

0. **[done] Migrate to `utils/quint` with expansion-time validation** —
   crate at `utils/quint`, expansion-time validation, backstop unchanged.
   add `Cargo.toml` (`proc-macro = true`, serde_json dep) and `src/lib.rs`
   to `utils/quint`; implement `operation`/`event` macros that validate
   the id against `pipeline/operations.json` / `events.json`; drop
   `utils/quint-annotations`; rename the backend dep and all markers;
   add `-p quint` to the three justfile utils recipes; fix README's
   oversold guarantee to state what is and is not enforced.
   Exit: a typo'd OP id in a backend marker fails `cargo check`;
   `just check` green; `utils/quint-annotations` gone.

1. **[ ] Static coverage gate** — compare the (now validated) marker
   set, catalog, event catalog and model tags: every OP has a marker on
   a real item and vice versa; every operation's event set and aspect
   read/write set matches the catalog; every event has an owning
   operation or an explicit reason not to. The comment-satisfies-scan
   hole is closed either by AST/token parsing in the checker or by a
   registry the compiler built.
   Explore first: validation cost, event-to-operation many-to-many
   ownership, model `@operation` coverage, generated report format,
   whether this belongs in precommit, CI, or both.
   Exit: missing/extra OP, event or aspect declarations fail the gate;
   a marker deleted with the production code fails even when a doc
   comment mentions it.

2. **[ ] Runtime trace reconciliation** — feature-gate operation
   entry/exit and aspect access traces; drive each boundary event
   scenario and compare the observed operation/aspect sequence with the
   expected sequence from `operations.json`/the model.
   Explore first: instrumentation overhead and cfg surface, async and
   closure coverage, which aspect accessors need instrumentation, trace
   format, and how much of the model can produce expected sequences.
   Exit: covered event scenarios fail when operations or aspect access
   order drift.

3. **[ ] Dylint local rules** — enforce type-resolved local invariants
   such as lease/resolve before mutation, aspect mutators confined to
   annotated operations, and annotated operations reachable from an
   event handler.
   Explore first: HIR/typeck vs MIR dataflow, false-positive rate,
   nightly/dylint maintenance cost, and which rules are worth a lint
   versus runtime trace or typestate.
   Exit: agreed local rules fail at lint time without blocking valid
   interleavings.

4. **[ ] Model guidance and coverage report** — generate an
   operation/aspect/event matrix and a model coverage report that shows
   modeled versus unmodeled operations, model actions without code
   anchors, and deferred nodes.
   Explore first: direction of source of truth (code annotations vs
   model), report schema, generated doc integration, and how the matrix
   guides model edits without letting code silently define the model.
   Exit: the report is reviewable and a drift between model and
   implementation is visible in one place.

5. **Not relevant: taint/flow analysis** — MIRAI/Flowistry/Charon are
   data-flow tools; revisit only if a specific provenance property
   (for example sidecar bytes reaching a row) cannot be checked by the
   layers above.

## Open questions

- Whether the docs layer (`docs/quint-modeling.md`, justfile
  `quint-check` comment) is updated as part of rung 0 or the separate
  spec-alignment pass in `quint-asset-aspects`.
- Registry mechanism for rung 1, if the validated-marker set proves
  insufficient for bidirectional coverage.
- Whether trace instrumentation (rung 2) is test-only or a dedicated
  harness.
- Dylint budget and which local rules justify it.
- CI placement: full `just check`, individual recipes, or a separate
  drift job.
