# Quint for Concurrent Actions Modeling

The following sections describe the use of Quint for formal modeling
and imposing the resulting invariants and properties back on the
implementation. The index pipeline is the first subsystem modeled
this way. See also: `docs/test-strategy.md`.

## Strategy

Human defines the assumptions (world-given) and the goals, commits or
drops each goal, and derives requirements from the committed ones. AI
generates the model from the implementation and user feedback, which is
cheap, but the proof runs on the model first: invariants on finite
bounds, scripted runs, simulator traces with classified findings. The
model may lean only on approved assumptions plus registered scope
delimiters; anything else is a hidden assumption and is classified
before merge. Once the model is proven, the requirements are imposed on
the implementation through gates, and the feedback loop keeps both
honest: every trace becomes a spec fix or a regression test, never a
shrugged-off log line.

Enforcing the structure is a closed-world audit of the model.
Every constraining element — actions, invariants, predicates —
carries at least one registered tag:

- `QUINT-ASS-N`: the element constrains, or relies on, the world
  (filesystem behavior, hash properties, deployment topology).
  The list is closed; new IDs land through human review.
- `REQ-N`: something the team must build (atomicity, barriers,
  exclusivity, scan scheduling). Requirements derive from committed
  goals, carry an owner and the gate test that would impose them, and
  are not reverse-covered by model tags; `verified` requires a live
  gate.
- `SCOPE`: neither of the above — proof-scope delimiters (finite
  bounds, initial state, collapsed lanes, atomic steps). Each
  states its bound and why the property survives it, or names
  the modeling step that lifts it.

Unmapped element means hidden assumption, and the gate fails.
Completeness is mechanical; soundness stays human — but the human
reviews a complete, explicit list instead of hunting invisible
premises. Classifying a new element:

1. Can any line of our code, config, or deployment change whether
   this holds? No → assumption; yes → 2.
2. Is it already imposed by an existing gate? Yes → requirement,
   cite the gate; no → requirement, status open, budgeted.
3. Is it a proof-scope delimiter? Yes → scope, with bound and
   reason. No → hidden assumption: classify before merge.

Implemented as `just spec-check` over committed files (the
`events-check.mjs` pattern): machine-readable `@assumes` /
`@requires` / `@scope` tags on every spec definition, registries in
`specs/assumptions.json`, `specs/requirements.json` and
`specs/scope.json`, and a generated traceability map
(`specs/traceability.json`) that fails on drift. Requirements are not
reverse-covered: they derive from committed goals and are gated by
implementation tests. No new tooling beyond node; no CI wiring — the
just rule is the gate for now. Human judgment keeps exactly three jobs:
strategy choice, assumption and goal correction, and the soundness
review of each classification.

## Components

- `specs/<subsystem>.qnt`: the model — actors, shared state, actions
  with enabling conditions, invariants (`NoLostUpload`-style safety
  properties on finite bounds), and `quint test` runs asserting
  settled states. Run `quint typecheck` and `quint test`; explore
  with `quint run`.
- `scripts/deps/quint.sh` + `QUINT_VERSION`: pinned toolchain,
  installed via `just install quint`, no JDK required.
- `just quint-check`: typecheck plus `quint test` over the model entry
  point (`specs/eventual-consistency.qnt`), wired into the `check`
  aggregate so every precommit runs it.
- Requirement registry: `specs/requirements.json` carries each derived
  obligation with owner, gate and status; the generated
  `specs/traceability.json` maps model definitions to their assumption
  and scope parents.

Currently used checks:

- `quint test` — Quint's runner for scripted scenarios written in the spec file. In this flow each run encodes one interleaving of the subsystem's actions and asserts its invariants on the settled state; in the index instance that is scan, watcher, upload, sweep and API reads against `NoLostUpload` / `NoPermanentStale`.
- Quint simulator (`quint run`) — Quint's explorer for random interleavings within small bounds. In this flow it samples orderings the scripted runs do not cover, and each violating trace becomes a regression test or a classified transient.
- Typestate / constructor-checked bundles — Rust structs that only build when their preconditions hold, so callers hold either a complete bundle or nothing. In this flow they carry a guard's inputs, so the handler runs only with a complete check.
- Dylint — a dynamic linter that plugs project-specific rules into `cargo clippy`. In this flow it is reserved for guard patterns outside type reach, used only when a second call site or a non-local pattern needs it.
- `proptest` — a Rust library that generates inputs for pure functions and shrinks failures. In this flow it covers the leaf decisions the spec and the code share, such as identity, gating and ordering rules.
- Stateful backend tests — integration tests that replay operation histories against the real backend. In this flow they replay histories from the spec's action set and compare settled state to the in-test model at sync points; in the index instance that is upload, modify, delete and rescan compared at each rescan.

## Interactions

A found simulator trace becomes a test or a classified transient
with a reason. Real violations fail the build until fixed;
expected transients are documented once.

New subsystems copy the same shape: small bounds, enabling
conditions as the contract source, in-test model plus sync points
for the stateful side.

## Adding a subsystem

Write the spec, derive its contracts, wire the checks, and record
each in the same places as above. A subsystem earns a spec when its
handlers share mutable state across concurrent triggers, its
correctness conditions fit in enabling conditions plus two or three
invariants, and at least one real interleaving bug (or a serious
flake) justifies the maintenance.
