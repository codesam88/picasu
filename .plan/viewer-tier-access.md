---
status: in-progress
type: feature
priority: high
area: backend
---

## Notes

Viewer-tier access: non-admin users can authenticate but cannot use the
gallery — every data read 401s (plan finding F1 in
`.plan/dsl-first-e2e-strategy.md`), and content writes are admin-gated
although the design grants users moves, names, images, and
confirmation-gated deletes. Owner decisions 2026-10-26 (recorded in the
dsl-first plan): extend the share/upload cookie fallback to any live
`User`; content writes are user acts; share passwords stop leaving the
backend (drop from reads, keep-semantics on edit); config fields cleaned
up first (done, pending commit — `imagePath` host-only, `uploadFolder`
open, `hasAuthKey` gone, `disableImg` frontend-only).

## Problem

`GuardShare`/`GuardUpload` fall back to admin-only cookie auth, so viewers
401 on prefetch, serving, tags, albums, and config; `GuardAuth` gates all
content writes although the threatened acts are user-level. No viewer read
capability exists in `docs/authz-model.md` §4.

## Goal

Authenticated non-admin users read the gallery (grid, serving, tags,
albums, config) and write their content (upload, move, trash, delete,
describe, cover, rotate); admins keep identity/config/jobs/share/host
authority. Every behavior change ships with its docs row and its tests in
the same commit — no safeguard is claimed until its check runs.

## Implementation steps (TDD, one slice per commit, tree green each step)

### S0 — Config field cleanup (done, pending commit)

Backend: drop `image_home`/`has_auth_key`/`disable_img` from `GET
/get/config`; unredact `upload_folder`; remove `disable_img` from
`AppConfig`, CONFIG_FIELDS, PUT/export/admin-config surface.
Frontend: `disableImg` to constStore + FrontendConfig toggle; imagePath
via admin entries; drop authKey indicator. Docs: `config.md`,
`docs/auth.md` mechanism. Tests: `api_config.rs` contract rewritten
RED-first (watched fail), `adminConfig.test.ts` fixtures, full backend
suite + `openapi-check` + frontend gates green.
Verify: `just backend-test`, `just openapi-check`, `just frontend-check`,
`just frontend-vitest`, DSL config scenarios still pass.
Status: implemented, uncommitted — commit as slice 1 after e2e re-run.

### S1 — Viewer reads: guards

Extend the `GuardShare`/`GuardUpload` cookie fallback from admin-only to
any live `User` (share-header/query paths byte-identical). Scope: prefetch
(+ renew-timestamp), serving cookie leg, `get-tags`, `get-albums`,
`get/config`. Bearer chain (`get-data`/rows/scroll/metadata, renew-hash)
follows with no change.
Docs in slice: `docs/auth.md` guard table fallback rows; mechanism
unchanged otherwise.
Tests in slice: backend matrices — viewer prefetch/get-data/serve/tag/
album/config 200 with rows; anonymous still 401; non-admin-without-share
companion reworked to viewer-vs-share terms (share-less anon denied,
viewer allowed); `authz-model.md` §4 denotation delta in the same commit.
Verify: targeted `cargo test`, `backend-check`, `openapi-check`.

### S2 — Content writes to `GuardUser`

`assign_album`, `delete-data`, `edit_flags`, `create_dir_album`,
`set_album_cover`, `rotate-image`, `regenerate-thumbnail`,
`edit_tag`/`edit_rating`, upload cookie leg. Each with a non-admin matrix
row proving the act and the admin path still working; read-only gating
unchanged (AUTH-12). `edit_share`/`create_share`/`delete_share` stay admin.
Docs in slice: `docs/auth.md` proposal banner updated per landed row;
`docs/authz-model.md` AUTH-11 boundary redrawn to admin-only acts.
Verify: targeted `cargo test`, `backend-check`, `openapi-check`
(401-matrix updates + dated oasdiff ignores where removal-shaped).

### S3 — Share passwords stop leaving the backend

Drop `password` from `AlbumInfo.share_list` reads (serve `hasPassword`
for the LinksPage icon); `edit_share` absent/null keeps, string sets
(`""` clears). Frontend: blank-to-keep edit form.
Docs in slice: `docs/auth.md` + `docs/openapi-reference.md` regenerated
as needed; share contract notes keep-semantics.
Tests in slice: read-never-contains-password matrix (admin included);
edit keep/set/clear matrix; frontend edit flow covered by existing share
e2e or extended YAML.
Verify: `cargo test`, `openapi-check`, frontend gates.

### S4 — Frontend enablement + e2e green

Remove viewer skips (prefetch config/tags/albums gates); enable edit UI
for non-admin (read-only mode still hides everything when on).
Docs in slice: `docs/frontend.md` if the data flow changed; proposal
banner removed from `docs/auth.md` once enforcement matches the text
(it must describe the shipped behavior on merge, not the intent).
Tests in slice: `non-admin-cmd-user.yaml` green end to end (provision →
login → grid → image view → move/delete); invite story legs re-attached
and green; full `just frontend-playwright` green (no failing pins).
Verify: `just frontend-check`, `just frontend-vitest`, full e2e.

## Out of scope

Viewer upload-target album creation beyond `create_dir_album`; per-user
trash/ACL (trash stays shared-visibility); share-password hashing at rest
(F8); user-store location changes; throttle/lockout changes.
