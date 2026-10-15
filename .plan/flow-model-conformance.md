---
status: open
type: feature
priority: high
area: testing
---

# Model-based flow conformance (single golden model, two drivers)

## Problem

Sequence bugs (R1 stale probe bounce, R4 first-user squat, R25
create-then-login split) are not single-state bugs. Per-page and per-route
tests pass while the composition breaks: state cached in A, session changed
in B, guard read stale state in C. Hand-writing one test per sequence does
not scale and re-injects drift.

## Goal

One machine-readable golden model of allowed flows; two conformance drivers
that check discovered traces against it:

- Layer 1 (backend API, first): state machine over the real backend
  (HTTP / `local::Client`). Independent of any frontend. Catches R3, R4,
  R8, R9-class violations deterministically.
- Layer 2 (full e2e with frontend): same model plus client state
  (probe cache, cookie jar, page, reload). Catches R1, R23–R26-class
  violations that only exist with frontend state in the loop.

The model is test-only and never imported by prod code: it encodes what the
user should observe, not how the implementation does it.

## Single source of truth

- Human doc: `docs/flow-model.md` (states, events, guards table).
- Machine twin: `tests/model/flow-model.json` (JSON Schema-validated).
- Generators read only the JSON twin; the doc is rendered or diffed from
  the same source so prose cannot drift. OpenAPI `links` feed the generic
  fuzzers; the golden model carries the auth semantics fuzzers cannot know.

## Scope

- States: store (empty / non-empty / adminless), flag (bootstrap on/off),
  session (none / user / admin / ephemeral-open-mode); Layer 2 adds
  probe-cache (fresh-[] / fresh-users / fresh-401 / stale-*), cookie
  (absent / valid / expired), page.
- Events: probe, bootstrap, register (3 gates), login, logout,
  reload/clear-cache, change-password (self + admin-to-other), expire-token,
  demote/remove-user, toggle-knobs, concurrent bootstrap x2.
- Each transition: guard + expected observables (page + API status + store
  count + invite/token effects). Invariants checked after every step
  (counts, admin flags, invite burned exactly once, demotion beats expiry).

## Relation to existing work

- Broader than auth but Layer 1 starts on auth (matches
  `auth-flows-enrollment.md` matrices + `auth-flows.md` tracker).
- Complements, not replaces: existing YAML scenario runners stay as
  deterministic sinks; Schemathesis/RESTler-class fuzzing stays as generic
  oracle (5xx, schema, `ignored_auth`); this model is the semantic oracle.
- Fits `docs/test-strategy.md` queued rows (property/state,
  mutation) and `api-assurance.md` (contract-derived tests).

## Steps

1. Freeze the model schema + auth slice (states/events/guards table for
   the 5 auth operations + probe). Gate: schema validates, doc renders.
2. Layer-1 driver spike: Hypothesis `RuleBasedStateMachine` (or
   `proptest-stateful` if Rust-local preferred) over tempdir backend;
   seeded, shrinking to minimal repro. Gate: R3/R4-class seeds fail
   before fix, pass after; concurrent-bootstrap x2 case included.
3. Wire generated paths into existing runners as deterministic cases
   (shortest-path per target + regression seeds). Gate: `just test`.
4. Layer-2 driver: same model + client-state rules over Playwright
   (navigate, reload, UI login/register). Gate: R1 seed fails before,
   passes after; backend-only suite stays green independently.
5. Seeded random-walk + 2-actor interleavings on top (after sequential
   model green; otherwise failures are uninterpretable).

## Decisions / open

- Generator engine (Hypothesis vs proptest-stateful for Layer 1) decided
  at step 2 spike; Layer 2 reuses the winner's model file regardless.
- True parallel mutation is step 5, not step 1.
