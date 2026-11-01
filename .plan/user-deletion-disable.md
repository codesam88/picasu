---
status: in-progress
type: feature
priority: high
area: backend
---

## Notes

The working spec for the task: problem, goal, implementation steps. Refine
these sections in place as understanding changes; append only short dated lines
for settled decisions. Keep it concise — progress chatter and back-and-forth
belong in commit messages, not here.

## Problem / purpose

Deleting a user today removes the identity record and nothing else: no
check for leftover personal data or live share bearer tokens, no proof of
intent beyond the session cookie, and no lightweight alternative to
deletion for the common "lock this account right now" case. Orphaned shares
keep working after their creator is gone, with nobody left who can revoke
them. This task makes deletion safe (refuse while the user has photos,
password-confirmed, owned shares cascade-deleted in the same step) and
adds account disable as the fast, side-effect-free lever.

## Settled decisions

- Deletion stays admin-only (already true); refusal when the user still
  has any photos answers 409 (state conflict, same as the zero-admin
  refusal on this route); deleting the sole admin stays 409.
- Confirmation is password re-entry via the existing `GuardReauth` guard
  (`X-Picasu-Password` header; 401 bad session, 403 missing/wrong
  password) — same contract as the password change and read-only lift,
  so a captured admin token alone cannot delete.
- Owned shares are cascade-deleted with the user (metadata rows only, no
  filesystem effect): deletion order is shares first, then the identity
  record, so a crash between the two leaves no orphaned bearer behind
  and the operation is retryable. Refusing while shares exist was
  rejected: it adds a second unblock step to the urgent path for no
  safety gain once deletion itself is confirmed.
- Disable is a `disabled` flag on the user record (default false,
  tolerated as absent by the lenient record deserializer): disabled
  users fail login exactly like a wrong password (same status, same
  dummy work — no oracle change), outstanding sessions die (token
  version bumped on disable, guards keep enforcing it), personal data
  and shares are untouched, re-enabling clears the flag. Disabling the
  last enabled admin is refused with 409, mirroring the zero-admin
  invariant. A disabled user with photos still cannot be deleted
  (disable is not a purge shortcut).
- Sharing is an independent per-user flag (`shares_enabled`, default
  true, same tolerant pattern): while disabled, the user cannot create
  new shares and share-based access resolving to their shares is denied
  exactly like a dead share (401, indistinguishable from removed), so no
  new oracle opens. Existing share rows are kept, so re-enabling
  restores access instantly. Account disable does NOT touch shares
  (lock stays side-effect-free); share disable does not touch the
  account. Both flags surface in the admin user listing next to each
  other, each with its own toggle endpoint mirroring the admin-role
  pattern.
- The delete-failure response for remaining photos (409) names the
  remedy in its message: lock the account and remove/revoke the shares;
  to delete manually, remove the files from the host filesystem under
  `USERS_HOME/<username>`, then retry the delete. The manual path is
  documented operator procedure, not API behavior.
- "Has photos" means any media file under the user's personal home
  (trashed included — trashed originals still occupy storage); global
  uploads are not attributable to a user and never block. On successful
  delete with no photos, the now-empty personal album record is pruned
  if present (empty dirs are otherwise reindexed on the next scan).

## Implementation steps

Each step is one worker invocation, TDD (RED observed, then GREEN), one
commit per step after parent review. No step starts until the previous
one is committed.

### D5-1 — Backend: deletion gates, confirmation, share cascade

`DELETE /delete/users/<user_id>` gains: GuardReauth (after the existing
guards), a personal-home media scan refusing with 409 while photos
remain, and owned-share cascade deletion (all albums' `share_list`
entries with `created_by` equal to the target, then the identity
record; empty personal album record pruned). Tests (RED first): delete
with photos → 409 and everything unchanged; delete without photos but
with shares → 200, shares gone (anonymous prefetch of each ex-share
→ 404), album record pruned; missing/wrong confirmation password →
401/403 with nothing changed; non-admin → 401 (existing pins stay
green). Done: `cargo test --lib` for every touched module (state
commands), fmt, clippy, no openapi regen unless annotations change
(they should not — same paths, new guard + 403/409 declarations mean
regen IS expected; confirm `openapi-json-match` + `auth-matrix-check`).

### D5-2 — Backend: disable/enable accounts

`UserRecord.disabled: bool` (default false; extend the tolerant
deserializer the same way `admin`/`home`/`ver` default); login rejects
disabled exactly like a wrong password; session guards reject disabled
records wherever the token version is enforced (bump `ver` on disable
so outstanding tokens die at once; re-enable clears the flag without
touching `ver`); new `PUT /put/users/disabled` (`{userId, disabled}`,
GuardAdmin, 404 unknown, 409 disabling the last enabled admin) with
the same audit logging shape as the admin-role endpoint. Frontend
`UserManagement.vue`: disabled state in the user list, enable/disable
toggle, delete flow collects the password for the re-auth header.
Tests (RED first): disabled login rejected; live session dies on
disable; listing shows the flag; last-enabled-admin disable → 409;
re-enable restores access; disabled user with photos still not
deletable. Done: targeted suites + `just frontend-check` +
`just frontend-vitest` (state counts), fmt, clippy, openapi regen +
`openapi-json-match` + `auth-matrix-check` (new route), env-var/doc
gates unaffected.

### D5-3 — Backend + UI: per-user share enable/disable

`UserRecord.shares_enabled: bool` (default true, same tolerant
pattern). Create path (`POST /post/create_share`) refuses with 403
when the creator's sharing is disabled. Resolve paths
(`try_resolve_share_from_headers/query`, share re-validation in
renew flows) deny shares whose creator disabled sharing, with the
same status as a dead share (indistinguishable, no new oracle).
New `PUT /put/users/sharing` (`{userId, enabled}`, GuardAdmin, 404
unknown) mirroring the disable endpoint (no last-admin rule: sharing
is not a liveness property). Admin user listing exposes both flags;
`UserManagement.vue` gains the share toggle next to the account lock.
Delete flow unchanged (cascade already removes owned shares).
Tests (RED first): disabled creator cannot create (403); anonymous
prefetch/serve/renew against their shares denied like dead shares;
re-enable restores all three; flags visible in admin listing;
non-admin cannot flip either flag. Done: targeted suites +
frontend checks, fmt, clippy, openapi regen + contract checks.

### D5-4 — Docs: deletion runbook and account-state model

Document the operator procedure (refuse-while-photos 409 message
wording pinned by D5-1 tests; lock account; revoke/remove shares;
manual deletion by removing files under `USERS_HOME/<username>` then
retrying delete) and the account-state model (active / disabled /
sharing-disabled as independent flags with a state table) wherever
admin user management is documented. Verify with `just docs-check`
and the plan's doc conventions (current state only, no
progress-chatter).

## Explicitly out of scope

- Attributing global uploads to uploaders (records carry no owner);
  trash semantics stay as-is.
- F9/F10 (explicitly deferred by user choice alongside this plan).
- `docs/openapi-reference.md` regen churn (tracked leftover: available
  widdershins produces template churn; regen with the pinned toolchain
  or leave stale with a note).
