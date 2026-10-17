---
status: in-progress
type: chore
priority: high
area: backend
---

## Notes

Problem: two password-change routes share one logic (`set_password_sync`).
`PUT /put/users/password` (`users_password.rs`) is canonical (self-with-old
plus admin-to-other recovery). `PUT /put/config/password` (`edit_config.rs`)
is a self-only alias from the single-password era, kept in `auth-users S4e`.
The SPA still calls the alias (`api/config.ts:updatePassword` from
`ChangePassword.vue`), so the canonical route has no SPA caller and the
admin-recovery path has no UI. Decided 2026-10-17 (interview): hard-remove
the alias now, repoint the form at the canonical route with the session
`userId`; admin panel and recovery-token work are separate idea stubs.

Strategy: frontend-first repoint then backend deletion, so no commit leaves
the UI calling a deleted route. Rejected: soft-deprecate with warn-log
(keeps two surfaces and a second audit path); 410 stub (same maintenance
without function). Hard-remove is breaking for direct API callers; the
migration is one line (`PUT /put/users/password` with `{userId, oldPassword,
newPassword}`).

## Implementation steps

1. Frontend repoint (scope: `frontend/src/api/users.ts`, `frontend/src/api/config.ts`,
   `frontend/src/components/Page/Config/ChangePassword.vue`,
   `frontend/src/script/utils/passwordChange*`): add `setUserPassword(userId,
oldPassword, newPassword)` calling `PUT /put/users/password`; `ChangePassword`
   reads `userId` from session/current-user store and passes it through;
   delete `updatePassword` from `api/config.ts`. Done: `passwordChange` vitest
   plus `ChangePassword` suite green, `tsc/eslint/prettier` clean. Leaves out:
   admin-to-other UI (separate plan).
   Done 2026-10-17: `setUserPassword` added, `ChangePassword` repointed to
   session userId, `updatePassword` deleted (169 vitest green).
2. Backend deletion (scope: `backend/src/router/put/edit_config.rs`,
   `backend/src/router/put/mod.rs`): delete `UpdatePasswordRequest`,
   `update_password_handler`, route registration. Done: `cargo test --lib`
   green, no `config/password` hits in `backend/src`. Leaves out: changes to
   `users_password.rs` behavior.
3. Contract and docs (scope: `backend/openapi.json`,
   `docs/openapi-reference.md`, `docs/auth.md`, `docs/auth-flows.md`,
   `docs/config.md`, `docs/frontend.md`, `docs/scenario-dsl.md`,
   `docs/playwright_generator.md`): regen openapi, rewrite alias mentions to
   the canonical route with body shape. Done: `just docs-openapi` + `openapi-check`
   green, zero `put/config/password` hits outside this plan and history.
   Leaves out: recovery-token and admin-panel docs.
4. Test sweep (scope: `backend/src/tests/enroll_gate.rs`,
   `backend/src/router/put/edit_config.rs` tests, scenario YAMLs,
   Playwright givens): delete alias rows, keep canonical `users/password`
   matrix (self 200/401, cross-user 403, unknown 404, empty-store 404).
   Done: full `cargo test`, frontend suite, `just check` green.
