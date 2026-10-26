---
status: in-progress
type: meta
priority: high
area: meta
---

# pre01: auth + test readiness board

Surveyed 2026-10-26. Identifiers: `CLEAN1`–`CLEAN5` are quick cleanups
(one sitting each), `FEAT1`–`FEAT7` are major features in dependency
order. Ticket slugs unchanged; prior release-checklist content mapped to
existing tickets (nothing live lost).

## Tickets

| ID     | Slug                              | Status      | Priority | Area     |
| ------ | --------------------------------- | ----------- | -------- | -------- |
| FEAT1a | `spec-reconciliation-4xx`         | done        | high     | testing  |
| FEAT1b | `api-assurance` (gate step)       | in-progress | medium   | testing  |
| FEAT2  | `security-schemes-declaration`    | open        | medium   | backend  |
| FEAT3  | `bug-rebuild-dir-album-cache`     | open        | high     | backend  |
| FEAT4  | `auth-hardening`                  | in-progress | high     | backend  |
| FEAT5  | `auth-flows-user-delete`          | open        | medium   | backend  |
| FEAT5  | `bug-readonly-lockout`            | backlog     | high     | backend  |
| FEAT6  | `auth-admin-panel`                | open        | high     | frontend |
| FEAT7  | `authz-check`                     | backlog     | high     | backend  |
| FEAT7  | `authz-props`                     | open        | high     | backend  |
| FEAT7  | `flow-model-conformance`          | open        | high     | testing  |
| FEAT7  | `error-handling-and-activity-log` | open        | high     | frontend |
| CLEAN4 | `e2e-duplicate-upload-flake`      | backlog     | medium   | testing  |
| CLEAN5 | (no single ticket)                | done        | medium   | testing  |

## Quick cleanups

- **CLEAN1 — Spectral exceptions.** The two persistent `openapi-check`
  warnings (`/unauthorized` 401-only operation, `/{dynamic_album_id}` vs
  `/{path}` rank pair) are deliberate shapes: document them as rule
  exceptions in `.spectral.yaml` so the gate runs visually green instead
  of "0 errors, 2 warnings" forever.
- **CLEAN2 — stale oasdiff ignore line.** The `disableImg`
  request-property entry in `.oasdiff-ignore.txt` still surfaces as a
  warning each run; verify its text against current oasdiff output and
  either correct or delete it.
- **CLEAN3 — tracer `ui.route` interpolation — closed as non-defect.**
  Verified against the code: `tracer.compare` is raw string equality and
  `assertionTarget` returns the raw pattern, so both sides of a
  parameterized cover match as written (`route:/album/[a-z0-9]+$` covers
  two scenarios today with zero warnings). No scenario uses `${}` in
  `ui.route`; interpolating would resolve per-run random ids and break
  the matching that works. No code change — the original leftover note
  was a misdiagnosis.
- **CLEAN4 — duplicate-upload flake** (`e2e-duplicate-upload-flake`) —
  **fixed**. Root cause: upload scheduled its flush and tree-update
  detached while the grid reads once after navigation; under parallel
  load the flush lost and nothing re-reads. The upload handler now
  awaits both hops (delete-path discipline), so 200 means visible.
  Evidence: family 6/6 × 6 runs, full suite 50/50 × 2, backend 808/808.
- **CLEAN5 — negative-DSL leftovers — done.** `E1` shipped `json` +
  `json_absent` on `api.response` with three consumers (boot signal,
  invite-listing no-token, viewer profile shape). `E8` added
  `allow_registration` to the given-config vocabulary, feeding the new
  `registration-knob-negative` scenario that pins `#10` (duplicate 409
  under the open knob) and `#12` (open knob never burns the invite).
  `E4` deferred with reason: logout already yields a cookieless jar for
  every planned consumer — build it only when a scenario needs
  anonymous and sessioned requests simultaneously.

## Major features (dependency order)

- **FEAT1 — reconciliation → assurance gate (critical path).**
  FEAT1a done: `spec-reconciliation-4xx` landed (422/404 documented,
  415 normalized, export content-type corrected; exit sweep clean of
  spec noise). FEAT1b remains: the Schemathesis campaign gate
  (`schemathesis-campaign-config`) and the env-var config-consistency
  lint — both now unblocked by FEAT1a.
- **FEAT2 — security schemes in the contract.** Declare cookie/bearer/
  share-header schemes in the generator and reference them per operation;
  `just openapi-check` stays green. Unblocks Schemathesis auth
  auto-inference; pairs with FEAT1's gate run.
- **FEAT3 — rebuild/dir-album cache bug.** Real correctness bug
  (Schemathesis F0): after `POST /post/rebuild`, share snapshots match
  nothing until restart. Reproduce over HTTP, fix cache refresh or
  invalidation in the rebuild path, pin the regression.
- **FEAT4 — auth-hardening closeout.** Fast pair first: `Cache-Control:
private` on `/object` and dropping `?token=` from the four bearer-guard
  call sites (finishes F10). Then the F8 decision: share-password hashing
  at rest (storage format + validation + migration of existing plaintext
  shares) — own sub-decision before any code.
- **FEAT5 — two bounded backend slices with decided designs:**
  `DELETE /delete/users/{id}` with the zero-admin 409 invariant plus its
  admin UI (`auth-flows-user-delete`), and the re-authentication guard
  on the read-only lift (`bug-readonly-lockout` decided fix).
- **FEAT6 — auth-admin-panel closeout.** Registration-toggle decision
  from the ticket's options, wire or delete the caller-less `createUser`,
  remaining elevation/set-password controls; then mark done.
- **FEAT7 — structural assurance.** Largest items, start when the
  critical path is clear: `authz-check` (encode `authz-model.md` as data
  - bidirectional gate), `authz-props` (proptest world + P-1…P-12),
    `flow-model-conformance` (golden model + two drivers),
    `error-handling-and-activity-log` (worker surfacing, error pages,
    activity log).

## Out of scope here

UI-polish tracking lives in `ui-refinement.md` (the old release board's
B2); release promises (file sync, metadata, sidecar writes) are complete
per that board's history.
