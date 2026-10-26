---
status: done
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

### S1 — Viewer reads: guards (DONE 2026-10-26, reviewed + committed)

Implemented as specified (`try_jwt_cookie_user`, both fallback switches;
`get-tags`/`get-albums` take `GuardUser` directly). Review notes, kept
with the change: `GuardUser`-style denotation precedent (model already
names live-`User` identity outside the Cap grammar, so the viewer term
follows suit — no grammar expansion); `try_jwt_cookie_user` refreshes the
admin flag from the record (strictly fresher than the token flag it
replaces); empty-store refusal preserved (401 via `GuardShare`, same as
before). Gates run: fmt, clippy `-D warnings`, 801 nextest + doc tests,
`openapi-check` clean (regen + no new ignores needed — only response
additions), e2e `non-admin-cmd-user.yaml` + `invite-registration.yaml`
green, full suite 47 passed.

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

### S2 — Content writes to `GuardUser` (DONE 2026-10-26, committed 8e193d3f)

`assign_album`, `delete-data`, `edit_flags`, `create_dir_album`,
`set_album_cover`, `rotate-image`, `regenerate-thumbnail`,
`edit_tag`/`edit_rating`, upload cookie leg. Each with a non-admin matrix
row proving the act and the admin path still working; read-only gating
unchanged (AUTH-12). `edit_share`/`create_share`/`delete_share` stay admin.
Docs in slice: `docs/auth.md` guard manifest and boundary section updated.
`authz-model.md` needed no AUTH-11 edit: the model never enumerated
content routes, so the guard denotations carry the split.
Verify: targeted `cargo test`, `backend-check`, `openapi-check`
(401-matrix updates + dated oasdiff ignores where removal-shaped).

### S3 — Share passwords stop leaving the backend (DONE 2026-10-26, committed e243f736)

Drop `password` from `AlbumInfo.share_list` reads (serve `hasPassword`
for the LinksPage icon); `edit_share` absent/null keeps, string sets
(`""` clears). Frontend: blank-to-keep edit form.
Docs in slice: `docs/auth.md` + `docs/openapi-reference.md` regenerated
as needed; share contract notes keep-semantics.
Tests in slice: read-never-contains-password matrix (admin included);
edit keep/set/clear matrix; frontend edit flow covered by existing share
e2e or extended YAML.
Verify: `share_secrets` matrix 2/2 (read redaction admin+viewer, keep/
set/clear against live share auth), 807 nextest, `openapi-check` clean,
vitest 267 (2 new contract tests), e2e 47 green.

### S4 — Frontend enablement + e2e green (DONE 2026-10-26)

Removed `shouldLoadAppConfig`/`shouldLoadAdminLists` (and their unit
tests) from `currentUser.ts`, `usePrefetch.ts` and `GalleryEmptyCard`;
all three endpoints are open to viewers, so the gates with-held data
instead of protecting anything. Edit UI was already ungated in the
frontend (`canScan` stays admin — repo jobs remain admin-only).
`non-admin-cmd-user.yaml` and the invite story legs green; full
`just frontend-playwright` 47 passed with the rebuilt bundle. `docs/auth.md`
Status row records the final state.

## Out of scope

Viewer upload-target album creation beyond `create_dir_album`; per-user
trash/ACL (trash stays shared-visibility); share-password hashing at rest
(F8); user-store location changes; throttle/lockout changes.
