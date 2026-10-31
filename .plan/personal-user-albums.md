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

Today every album under `IMAGE_HOME` is global: any logged-in user (and any
admin) sees and modifies the same album tree, timeline, and shares. There is
no per-user private area. We want a personal album area per user alongside
the shared public tree, with strict isolation (including admins), while
keeping the filesystem as source of truth.

## Settled decisions (2026-10-31 interview)

- Storage: opt-in external path. `PICASU_USERS_HOME` unset = feature off,
  server behaves exactly as today. No default inside `IMAGE_HOME`.
- Mapping: username to directory via a mapping stanza (config), defaulting
  to a canonicalized form of the user-id; overrides allowed. User-id renames
  while data exists are out of scope.
- Admin boundary: full isolation. Admins keep user/invite/config powers but
  have no cross-user album, photo, or share visibility.
- Share powers: any logged-in user may share public albums and their own
  personal albums.
- Timeline: opt-in toggle (not automatic union). Personal items appear in
  timeline only when the user enables it; default off. Server enforces the
  toggle, never the client alone.
- Scan: admin Scan Now covers global plus all personal trees.

## Boundary namespace (2026-10-31 follow-up)

- Storage stays external (`S0`/`S1` as built); nesting rejected: no volume
  separation, share exposure, name reservation, dot-dir backup risk.
- Minimal marker where paths cross the API boundary: `POST /post/index/album`
  gains an explicit root selector (default preserves probe order
  image-then-users); `dir_path` in `get-albums` gains an owner/root marker
  (`S2`). No URI scheme; mutation APIs stay ID-based.

## Solution strategy

Chosen direction: filesystem-prefix ownership under a separate, configurable
`users_home` root (`PICASU_USERS_HOME` / `gallery.users_home`, host tier,
nullable = feature disabled). Each user owns exactly
`<users_home>/<user-id>/...`; everything else is public. Ownership is derived
from the canonical path prefix on every request, never from a stored owner
column or a client-supplied filter, so a rebuild from disk preserves it.

Rejected alternatives and why:

- `$IMAGE_HOME/.users/$username` single-root layout: simplest indexing
  (one watcher, one `dir_album` tree, existing `image_root.join` logic
  untouched) but mixes private data into the public tree, backup/export, and
  dot-dir walker semantics; still needs the same hide-and-scope logic, so it
  saves little and forecloses placing personal data on a separate volume.
  Decisive drawback: cannot satisfy the "outside `$IMAGE_HOME`" requirement.
- Stored owner column in album metadata / DB: fast lookups but diverges from
  `docs/design.md` filesystem-truth (moves/renames outside the app desync
  it), needs backfill and conflict resolution. Decisive drawback: second
  source of truth for identity.
- Client-side scoping only (new `basicString` for personal page, timeline
  union in the SPA): no security; `basicString`/`search` is caller-controlled
  and every `prefetch`/`get-data` path would still serve cross-user rows.
  Decisive drawback: spoofable, violates AUTH model.

The `users_home` root itself is never listed, addressable, movable,
shareable, or scannable; only its per-user subtrees are. Timeline returns the
joined set (global plus caller-owned personal) enforced server-side. Shares
can be minted from either a public or the caller's own personal album.
Admins keep user/invite/config powers but get no cross-user album, photo, or
share visibility: an admin is a regular user for data access.

## Implementation steps

Each step is independently reviewable, TDD (failing test first), with docs
and OpenAPI artifacts regenerated in the same change.

### S0 — Config and path authority

Scope: `backend/src/model/config.rs` (`CONFIG_FIELDS`, `TomlGallery`,
`apply_env_overrides`), `backend/src/storage/files.rs`, `docs/config.md`,
`scripts/env-var-lint` fixtures.
Add host-tier `gallery.users_home: Option<PathBuf>` + `PICASU_USERS_HOME`
override, nullable (null = personal areas disabled). Central helpers:
`get_resolved_users_home()`, `users_dir_for(user_id)` (mapping stanza,
default canonicalized user-id; validation reuses user-id rules),
`classify_path(canonical) -> Public | Personal { owner } | UsersRoot | Outside`,
`personal_root_visible_to(caller, path)`. Username-to-segment mapping reuses
existing user-id validation (no `/`, `\0`, `.`/`..`, separator homoglyphs);
`users_home` must not equal or contain `image_home` and vice versa (reject at
boot/scan with 400/500, tested).
Done: unit tests for classification, boot rejection cases, `just env-var-check`
green, `docs/config.md` tier/env tables updated.

### S1 — Indexing and watcher over two roots

Scope: `backend/src/tasks/actor/album_index.rs`
(`index_album`, `internal_subtree_roots`, stale sweep),
`backend/src/tasks/batcher/start_watcher.rs`, `backend/src/workflow/mod.rs`
(`index_image`, `ensure_dir_albums`), `backend/src/process/dir_album.rs`.
Index/walk both roots; admin Scan Now covers global plus all personal
trees. Nested/equal roots are rejected at boot (S0), so no double-index
case exists. Watcher subscribes to both
roots. `index_image`/`ensure_dir_albums` accept either root. The `users_home`
root dir itself gets no album record and is pruned if one exists.
Done: backend tests indexing into both roots, no album record for the root,
watcher test for personal path; existing single-root tests green.

### S2 — Read scoping: album list, prefetch, data, timeline

Scope: `backend/src/router/get/get_list.rs` (`get_albums`),
`backend/src/model/expression.rs` (`generate_filter`,
`generate_filter_hide_metadata`), `backend/src/router/get/get_prefetch.rs`,
`backend/src/router/get/get_data.rs`, `get_fs_completion.rs`, tags/counts.
`get-albums` drops the `users_home` root entry and any personal album not
owned by the caller; `dir_path` for personal albums is reported relative to
`users_home` with an owner marker (frontend never sees absolute paths).
Every read ANDs the caller constraint: personal paths require
`owner == caller`, public paths pass. Timeline joins public plus own
personal only when the caller's opt-in toggle is on (server-enforced;
default off), regardless of client `basicString`.
`get_fs_completion` and tag counts obey the same visibility.
Done: authz-style tests (other user's album absent from list/prefetch/data,
timeline contains global+own and not other's, root invisible); share-scoped
reads unchanged.

### S3 — Write scoping: move/assign, create, delete, upload, flags

Scope: `backend/src/router/put/assign_album.rs`,
`backend/src/router/post/create_dir_album.rs`, `backend/src/router/delete.rs`,
`backend/src/router/post/post_upload.rs`, `backend/src/router/put/edit_flags.rs`.
Every mutation resolves source and destination through `classify_path` and
refuses cross-owner moves (public <-> personal-of-other, personal-A <->
personal-B) and any operation addressing the `users_home` root, with no
partial moves (refuse before first rename; existing skip/rename semantics
kept within the allowed tree). Upload default target stays
`<image_home>/<upload_folder>`; personal upload requires an explicit
caller-owned target album. Covers are included (cover must be visible to
caller).
Done: per-op tests for cross-user move/create/delete/upload/trash refusal and
target-unchanged assertions; admin-on-other-user refusal.

### S4 — Shares from personal albums, owner-scoped management

Scope: `backend/src/router/post/create_share.rs`,
`backend/src/router/put/edit_share.rs`, `delete_share`,
`backend/src/storage/db.rs` (`share_visible_to`),
`backend/src/router/auth.rs` (`resolve_share_internal`).
Relax share management from `GuardAdmin` to `GuardUser` + ownership check:
any user may create/edit/delete shares on albums they can see (public or
own personal); nobody (including admins) may manage shares on another user's
personal album. Listing stays caller-created-only. Presenter/read path
unchanged (already album-scoped). Share IDs remain unguessable; resolving a
personal-album share works for anonymous holders as today.
Done: tests for user share CRUD on own personal + public, 403/404 on
other's, admin-cannot-share-other's; `docs/authz-model.md` + auth-matrix
regenerated (`just auth-matrix-check`, `just openapi-gen`).

### S5 — Frontend: personal root page, nav, dialogs

Scope: `frontend/src/route/` (new `myAlbums`/`personal` `baseName`,
`createRoute`), `NavPanel.vue`, `AlbumsPage.vue`/`AlbumContentsPage.vue`
(personal root filter `root_album:true` + owner enforced server-side),
`albumStore.ts`, `AssignAlbumModal.ts` tree, `GalleryBar.vue` breadcrumb,
`LinksPage.vue`, `usePrefetch.ts`.
New nav entry for the personal root; public Albums page never shows the
`users_home` root or other users' albums (server already filters; frontend
drops any such row defensively). Assign/create/upload dialogs list only
visible targets. Timeline gets an opt-in toggle (default off) included in
the server-enforced filter; no client-only gating.
Done: vitest for route/filter wiring, playwright scenario (two users, each
sees own + public, not each other's; assign dialog hides other's).

### S6 — Assurance, docs, rollout

Scope: `docs/authz-model.md`, `docs/authz-coverage.md` (if present),
`docs/config.md`, `docs/openapi-reference.md`, `backend/openapi.json`,
`backend/auth-matrix.json`, `docs/auth-matrix.md`, test-strategy notes.
Add per-user isolation invariants (read/write/share/list parity), extend the
route x invariant matrix, pin findings as named tests. Migration: enabling
`users_home` on an existing install indexes new files only; pre-existing
files under the new root are picked up by Scan Now; no public data moves.
When `users_home` is null the server behaves exactly as today (all S2–S4
gates pass-through; tests cover disabled mode).
Done: `just check`, `just backend-test`, `just frontend-check`,
`just frontend-vitest`, `just openapi-check`, `just auth-matrix-check`,
`just plan-lint` green; docs rebuilt.

## Open decisions (for interview)

1. Storage default: `users_home` default inside vs outside `image_home`,
   null-disabled vs auto-created?
2. Username mapping: raw user-id as directory name (validation strictness,
   rename on user-id change?) vs separate opaque dir with mapping file?
3. Admin boundary: admins lose all cross-user data access (as stated) vs
   read-only break-glass?
4. Share powers: any authenticated user may share public albums too, or only
   own personal ones?
5. Timeline: automatic union (as stated) vs opt-in toggle?
6. Scan semantics: who may trigger personal scans; does admin Scan Now cover
   all personal trees or global only?
