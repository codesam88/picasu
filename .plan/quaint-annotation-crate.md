---
status: idea
type: feature
priority: medium
area: backend
---

## Goal

Replace the no-op `utils/quint-annotations` marker crate with an
annotation crate that does something: emit operation/event metadata
(and, if justified, feature-gated trace hooks) so the implementation,
`operations.json`, `events.json`, and the Quint model can be checked
against each other.

This is an idea-level ladder. Steps 0–4 each need an in-depth cost/gain
exploration before implementation commitment; step 5 is recorded as not
relevant here.

## Current state

- `utils/quint-annotations` is a no-op proc-macro crate. The only
  consumer is the regex scanner in `utils/quint/operations-check.mjs`.
- `utils/quint/pipeline/operations.json` maps OP-1..OP-12 to events,
  four-aspect reads/writes, guards, status and N nodes.
- `asset-aspects.qnt` tags new actions with `@operation`; existing
  actions are mostly untagged.
- The marker layer detects unknown/missing OP text, but not operation
  order, aspect access order, call-site coverage, or model drift.

## Preliminary findings

- utoipa works because `#[utoipa::path]` generates a typed `Path`
  implementation and `utoipauto` aggregates it; `openapi-sanity` then
  checks handlers against the generated artifact. The useful pattern is
  annotation -> generated artifact -> parity gate.
- A no-op attribute is equivalent to a comment marker. It does not
  justify a proc-macro crate unless it generates metadata, registration,
  instrumentation, or a compile-time check.
- Real options: registry macro (`inventory`/`linkme`), feature-gated
  runtime trace, dylint local rules, typestate/capability tokens, and
  taint/flow tools (MIRAI, Flowistry, Charon).
- Taint/flow tools analyze data flow, not operation sequencing. They are
  not a substitute for ordered operation/aspect reconciliation; step 5
  is not relevant unless a specific provenance property remains
  uncheckable.

## Steps

0. **[ ] Create the actual annotation crate** — replace the no-op
   attributes with a macro that emits `OperationSpec`/`EventSpec`
   registry entries (or equivalent typed metadata), so the annotation
   has a compile-time consumer.
   Explore first: `inventory` vs `linkme` vs build-script-generated
   registry; compile-time validation of OP/event ids; Rocket/utoipa
   attribute ordering; linker/dead-code caveats; whether event handlers
   and shared helpers need separate marker forms.
   Exit: a registry dump exists and matches `operations.json`; the
   no-op crate is gone or has a real role.

1. **[ ] Static coverage gate** — compare the registry, catalog, event
   catalog and model tags: every OP has a marker and vice versa; every
   operation's event set and aspect read/write set matches the catalog;
   every event has an owning operation or an explicit reason not to.
   Explore first: validation cost, event-to-operation many-to-many
   ownership, model `@operation` coverage, generated report format, and
   whether this belongs in precommit, CI, or both.
   Exit: missing/extra OP, event or aspect declarations fail the gate.

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

- Registry mechanism and macro syntax; compile-time OP-id validation.
- Whether trace instrumentation is test-only or a dedicated harness.
- Dylint budget and which local rules justify it.
- Key convention for operation, event and aspect ids.
- CI placement: full `just check`, individual recipes, or a separate
  drift job.
