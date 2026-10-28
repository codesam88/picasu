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

| ID     | Slug                              | Status  | Priority | Area     |
| ------ | --------------------------------- | ------- | -------- | -------- |
| FEAT1a | `spec-reconciliation-4xx`         | done    | high     | testing  |
| FEAT1b | `api-assurance` (gate step)       | done    | medium   | testing  |
| FEAT2  | `security-schemes-declaration`    | done    | medium   | backend  |
| FEAT3  | `bug-rebuild-dir-album-cache`     | done    | high     | backend  |
| FEAT4  | `auth-hardening`                  | done    | high     | backend  |
| FEAT5  | `auth-flows-user-delete`          | done    | medium   | backend  |
| FEAT5  | `bug-readonly-lockout`            | done    | high     | backend  |
| FEAT6  | `auth-admin-panel`                | done    | high     | frontend |
| FEAT7  | `authz-check`                     | backlog | high     | backend  |
| FEAT7  | `authz-props`                     | open    | high     | backend  |
| FEAT7  | `flow-model-conformance`          | open    | high     | testing  |
| FEAT7  | `error-handling-and-activity-log` | open    | high     | frontend |
| CLEAN4 | `e2e-duplicate-upload-flake`      | backlog | medium   | testing  |
| CLEAN5 | (no single ticket)                | done    | medium   | testing  |

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
  spec noise). FEAT1b done: the Schemathesis campaign config shipped
  as the two-tier harness (`just fuzz-check` / `just fuzz-run`, advisory
  nightly SARIF workflow) and the env-var config-consistency lint
  (`just env-var-check`, wired into `check`/precommit/CI).
- **FEAT2 — security schemes in the contract.** Declare cookie/bearer/
  share-header schemes in the generator and reference them per operation;
  `just openapi-check` stays green. Unblocks Schemathesis auth
  auto-inference; pairs with FEAT1's gate run.
- **FEAT3 — rebuild/dir-album cache bug.** Done — superseded by
  `unify-index-rebuild`: the broken `POST /post/rebuild` path was
  retired in favour of `POST /post/index/album` with `clear`, which
  resets, re-registers and sweeps the dir-album cache. Pinned by
  `bug_rebuild_dir_album_cache_reset_scan_returns_rows` (prefetch
  returns N rows after reset+scan, no restart).
- **FEAT4 — auth-hardening closeout.** Done: `Cache-Control: private`
  on `/object` (public-mode revision note) and header-only bearer
  (F10); F8 closed with argon2id for both password stores — shares
  keep legacy plaintext rows until recreated (no migration), users
  dual-path verify and rehash on next login — and
  `PICASU_PBKDF_ITERATIONS` is replaced by the test-only
  `PICASU_WEAK_PWHASH=1` switch.
- **FEAT5 — both slices done.** `DELETE /delete/users/{id}` ships with
  the zero-admin 409 invariant enforced inside the store transaction
  and a delete control beside demote in the user panel
  (`auth-flows-user-delete`); the re-authentication guard covers the
  read-only lift and the password change, throttle moved with it
  (`bug-readonly-lockout`).
- **FEAT6 — auth-admin-panel done.** The registration toggle stays
  writable (option A): the ticket's host-only premise was R12-specific
  and config-v2 made `allow_registration` a runtime API knob again.
  The panel gains admin-create-user (wiring the caller-less
  `createUser`, resolving R22), per-row set-password behind the
  re-auth header prompt, invite status/revoke display, and read-back
  display — with pins at the api/util layers plus the house
  structural pins for .vue glue.
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
