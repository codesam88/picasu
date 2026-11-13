# utils/quint

Checkable models of subsystem protocols, one file per subsystem:
`utils/quint/pipeline/<subsystem>.qnt`. Each spec is exercised by `just quint-check`
(typecheck plus `quint test`); exploratory simulation is `quint run`
and never gates.

## Assumption rules

Assumptions record what the model takes as given: operating
conditions it relies on, non-goals it excludes, and properties met
by components out of scope here.

- Assumptions are expensive and require human review.
- If the assumption can be reasonably expressed as a requirement
  onto the implementation, it is a requirement, not an assumption.
- If the assumption has no formalized representation in the model,
  it is likely vacuous or redundant. Consider consolidating.

Current contents of `pipeline/`:

- `eventual-consistency.qnt` — the pipeline protocol as prescribed
  event handling: watcher/API boundary events, recovery events
  (watcher gap, crash, derived-store loss, DB loss), intake submits,
  worker steps, and per-root scan/sweep. Invariants: no lost intake
  work, no permanent stale rows; the convergence goal is `Converged`
  (every processable asset path has exactly one correct row; every
  row's asset exists; nothing queued), pinned per diverged shape by
  the runs. Unprocessable paths are logged, never indexed, and are
  excluded from the goal.

  The spec is split across modules that import each other (native
  Quint modules, no preprocessor): `eventual-consistency-state.qnt` holds shared
  types, variables, finite bounds (five paths, two roots), and the
  initial state; `eventual-consistency-assumptions.qnt` holds the snapshot matcher
  plus the `NoLostUpload` / `NoPermanentStale` safety predicates,
  each beside its prose and ASS tag;
  `eventual-consistency-goal.qnt` holds the `Converged` goal
  predicate — the subsystem-specific part;
  `eventual-consistency.qnt` holds the actions, the `step`
  interleaving, and the runs.

- `asset-aspects.qnt` — the four-aspect slice (asset file, sidecar,
  thumbnail/derived, DB asset row): alignment predicates and
  conflicting-pair runs over the same state; imports
  `eventual-consistency.qnt`.
- `operations.json` — internal mutation operations OP-1..OP-12 with
  events, aspect reads/writes, guards, status and interaction nodes;
  validated by `just operations-check` against backend annotations.
- `events.json` — boundary and recovery events with their settled
  effects and known faults, validated by `just events-check`.
- `goals.json` — goals G0–G4 with decision (`committed`, `queued`,
  `dropped`), anchors and assumptions; only committed goals source
  requirements. Validated by `just catalog-check`.
- `requirements.json` — implementation obligations (REQ-1..n) derived
  from committed goals. Each entry carries the contract (`statement`,
  `derived_from`, `owner`, `gate`, `status`) plus the review text
  (`implementation`, `tests`, optional `limits`). `catalog-check`
  enforces that a requirement derives only from committed goals, that
  every committed goal derives at least one requirement, and that a
  `verified` requirement names at least one gate test. The generated
  `docs/eventual-consistency.md` renders the contract under
  Requirements and the review text under Implementation.
- `assumptions.json` — index of ASS-1..n with owner and status;
  the model's `@assumes` tags state which definitions each one
  grounds, and the generated traceability map carries the reverse
  refs. Validated by `just catalog-check`.
- `scope.json` — modeling bounds and abstractions (SCOPE-1..n), each
  with bound, why_safe, lifting step, and provisional flag where the
  safety argument is weaker than the implementation's guarantee. A
  top-level `modeled_scope` summarizes the current model boundary;
  validated by `just catalog-check`.
- `catalog.schema.json` — required fields and enums shared by the
  catalog validators.
- `traceability.json` — machine-generated mapping written by
  `just spec-check --generate`. The audit fails on drift.
  Load-bearing references appear in `.qnt` action comments and
  `events.json` notes.

Each catalog file carries a top-level `description`; the generated
review doc renders it as the section introduction. Requirements derive
from committed goals (never from dropped or queued ones), and
`@requires` tags in the model resolve against `requirements.json`. A requirement stays `open` until its gate test
exists, and is `imposed`/`verified` only with that gate.

Conventions for new subsystems: name the file for the subsystem, keep
bounds tiny (2–3 of each entity), state enabling conditions explicitly,
and record every derived contract where its enforcement lives (see
`docs/quint-modeling.md` for the full flow). Simulator
counterexamples become regression tests or documented transients —
never silent.

## Tooling

- `specs-check.mjs` — closed-world tag audit and the generated
  `pipeline/traceability.json` map.
- `events-check.mjs`, `catalog-check.mjs` — catalog validation against
  `pipeline/catalog.schema.json`.
- `operations-check.mjs` — validates `pipeline/operations.json` and the
  `#[quint_annotations::operation(...)]` / `#[quint_annotations::event(...)]`
  markers in `backend/src/**/*.rs`.
- `gen-eventual-consistency-doc.mjs` — writes `docs/eventual-consistency.md`.

## Operations and annotations

`pipeline/operations.json` tracks the implementation of the four-aspect slice
(asset file, sidecar, thumbnail/derived, DB asset row) as operations OP-1..OP-12.
Each operation records its events, aspect reads/writes, guards, status and
interaction nodes. Backend functions carry no-op attribute markers from the
`utils/quint-annotations` crate (`#[quint_annotations::operation("OP-N")]`,
`#[quint_annotations::event("EVENT_NAME")]`) so the validator can confirm every
tracked operation has at least one implementation site. The macros return the
item unchanged.

The Quint toolchain installer stays in `scripts/deps/quint.sh` with the
shared pins in `scripts/deps/versions.env`.

Modeling scope (not assumptions, no formal counterpart required):
no cross-root identity in v1 — moves across watched-root
boundaries are observed as creation plus ghost and deletion; walk
roots are disjoint from the derived store, so pipeline writes
cannot resubmit themselves.
