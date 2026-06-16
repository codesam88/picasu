# Local backlog (gitignored)

- standalone TLS deployment path: add `tls` Cargo feature gating `rocket/tls`, validate end-to-end with a real cert (certbot/acme.sh), document in CONFIG.md; only ship once tested
- review for license issues, SPDX label and OSSF score card
- review configuration options against docs
- add a proper documentation / generation system
- move the fork to github, compatible CI flows and Android app by Ted(?)
- enable the release/deployment flows in CI
- add android app 
- review code module by module

## CI

- [x] resolved: 'just build'/'just run'/'just test' now use a debug build without embed-frontend
  (the developer default, matching check/test); 'just build-release' is the production
  configuration (release + embed-frontend), matching CI's release workflow and the installer
  scripts. See `docs/test-strategy.md` "Build configurations: developer vs. production".
- [ ] CI should be expanded to test all supported configurations on PR and release:
  - PR/merge CI: developer config (debug, no embed-frontend) — matches local precommit
  - release CI: production config (release + embed-frontend) — already done in
    `.github/workflows/release.yml`, but not yet gated as a required PR check before release

- tags assigned via api should be merged into DB as well as image repository on disk (idea borrowed from Immich)
  1. use xmp sidecar files to store all extracted data without modifying originals
  2. on indexing, if no xmp sidecar exists, create from image metadata
  3. on tag or other metadata changes, first store to db and then to xmp sidecar
  4. on album assignment (causing the original file to be moved), xmp sidecar files are moved together with the originals 
  5. we can provide a separate helper script to merge metadata from sidecar files back into originals. major photo apps probably also support such files.
  - DEFERRED — not part of the storage-architecture fix below; xmp sidecars are a separate
    future step.

## Storage architecture fix: stop duplicating originals into DATA_HOME (2026-06-16, DONE)

Implemented in 4 sequenced commits exactly as planned below; full suite (75 tests) green
throughout, no docker daemon available so no live-deployment verification was done beyond the
in-process e2e suite. One follow-up still open, not part of this fix: `UROCISSA_STATE_HOME`
(splitting `cache_db.redb`/`temp_db.redb`/`expire_db.redb` out of `DATA_HOME` into a true
disposable-state dir) — see the standalone item further down.

### Follow-up (2026-06-16): force-reindex existing files, not just new ones — DONE

Gap found after the fix above: "Scan Image Path" (`start_image_home_scan`) only processes
*newly-discovered* hashes — `deduplicate_task` short-circuits for already-known content, so it
could never fix an inconsistent/stale record or properly index a pre-existing file repo whose
files were indexed under older/incomplete logic. The only other reindex path was per-selection in
the gallery (select items → "Reindex" in the batch/right-click menu → `PUT /put/reindex`), with no
bulk admin-level action.

Added `force` end-to-end: `workflow::index_for_watch_full` (new, `index_for_watch` now a thin
`force: false` wrapper) → `DeduplicateTask.force` (returns the merged record for full
reprocessing instead of stopping) → `FolderImportTask`/`FolderImportFileTask` → `start_image_home_scan(force: bool)`
→ `POST /post/import/image-home?force=true`. Frontend: a checkbox on the existing "Scan Image
Path" action ("Also refresh metadata for files already indexed"). `start_folder_import` (arbitrary
external paths) intentionally does *not* get a force option — it's a "bulk add" action, not a
reindex one. Test: `scenario_z` (non-forced scan leaves a stale record untouched; forced scan
refreshes dimensions/tags/album).

### Problem (confirmed by code audit, not just the symptom)

`object/imported/<hash>.<ext>` is a full-resolution **copy** of every indexed file, content-addressed,
living under `DATA_HOME`. This contradicts the agreed design (`docs/design.md`): `IMAGE_HOME` should
be the single copy of originals, addressed by their real hierarchical path; only *derived* artifacts
(`object/compressed/` thumbnails/previews) should be content-addressed, and only those belong in
`DATA_HOME`.

Concrete consequences found while auditing every `imported_path()`/`imported_path_string()` call site:

- Every original is duplicated on disk (2x storage) for no architectural reason — nothing requires
  the duplicate to exist *except* the code that was written to read from it instead of the real file.
- **Uploads never land in `IMAGE_HOME` at all.** `post_upload.rs` writes to `DATA_HOME/upload/`,
  `CopyTask` duplicates into `object/imported/`, then `DeleteTask` deletes the `upload/` staging
  copy. The uploaded photo's only persistent representation is the `DATA_HOME` CAS blob; there is no
  file under `IMAGE_HOME`, so it has no real directory/album membership — `presigned_album_id` is
  just a DB label, not where the bytes live.
- **This breaks `assign_album` for every uploaded photo**: `fs::rename(alias[0].file, …)` 422s
  because `alias[0].file` is the `upload/` staging path, already deleted by `DeleteTask`.
- Thumbnail/video pipelines read the duplicate, not the original: `generate_dynamic_image.rs`,
  `generate_thumbnail.rs` (video frame extraction), `generate_compressed_video.rs`,
  `generate_width_height.rs` (video ffprobe), `process/info.rs`'s `regenerate_metadata_for_*` (file
  size). EXIF extraction and hashing already correctly read the live file — no change needed there.
- Already correct, no change needed: `rotate_image.rs` only ever touches the compressed thumbnail,
  never the original. `delete_data.rs` only removes the DB record, never touches files on disk
  (pre-existing, separate, lower-priority gap: this currently orphans the `object/compressed`
  thumbnail on delete; not folded into this fix, track separately if it matters).

### Decisions (made 2026-06-16)

- **Upload ingress**: a configurable ingress subfolder under `IMAGE_HOME` (default e.g. `uploads/`)
  for uploads with no `presignedAlbumId`; it becomes its own top-level album automatically (album =
  directory). Uploads *with* a `presignedAlbumId` write straight into that album's real directory.
- **Migration**: none, automatic. Stop writing/reading `object/imported/`; pre-existing installs keep
  whatever's already there as harmless, manually-deletable orphaned data. Document in `CONFIG.md`.
- **Multiple aliases / missing files**: keep `source_path()`/serving on `alias[0]` only — no
  fallback-through-the-list logic. But: a missing aliased file is a real file/DB inconsistency
  (external manipulation or data loss outside the app), so make detection of it loud instead of
  silent:
  - `deduplicate_task` already does `exist_alias.retain(|a| Path::new(&a.file).exists())` —
    currently silent. Add a `warn!` naming exactly which alias path(s) got pruned and why.
  - Add a distinct, specific `warn!` when a *new* alias is actually added for already-known content
    (genuine duplicate-content detection), replacing today's generic
    `"File already exists in the database"` warning that fires even on a routine same-path re-index
    (i.e. make the warning fire only when something actually changed, and say what).

### File-by-file change inventory (backend)

**Remove the original-duplication path:**
- `tasks/actor/copy.rs` — delete `CopyTask` entirely.
- `process/io.rs` — `copy_with_retry` becomes unused; remove (confirm no other caller first).
- `workflow/mod.rs::index_for_watch` — drop the `CopyTask::new(...)` step.
- `public/structure/abstract_data.rs` — remove `imported_path()`/`imported_path_string()`.

**Switch readers from `imported_path()` to `source_path()`:**
- `operations/indexation/generate_dynamic_image.rs` (image branch)
- `operations/indexation/generate_thumbnail.rs` (video frame extraction ffmpeg `-i` arg)
- `operations/indexation/generate_compressed_video.rs` (ffprobe duration + ffmpeg input)
- `operations/indexation/generate_width_height.rs` (video ffprobe width/height)
- `process/info.rs::regenerate_metadata_for_image/_video` (file size via `metadata(...)`)

**Rework serving:**
- `router/get/get_img.rs::imported_file` — instead of joining a literal CAS path, look up the
  `AbstractData` by the hash already extracted/validated by `GuardHashOriginal`, then
  `NamedFile::open(abstract_data.source_path())`. 404/410 if the hash isn't found or the file is
  missing (mirrors the existing stale-alias handling pattern in `assign_album.rs`/scenario_j).

**Rework uploads to write into `IMAGE_HOME`:**
- `router/post/post_upload.rs::save_file` — write directly into the target directory (resolved
  album dir via `get_dir_path_for_album` if `presignedAlbumId` given, else the configured ingress
  subfolder under the resolved `imagePath`) using the same tmp-suffix-then-atomic-rename trick,
  relocated. New: ingress-subfolder name/config (decide where this is configured — likely a new
  `PublicConfig` field or a fixed convention name under `imagePath`; create it at startup like the
  default `images/` dir is created today).
- `public/structure/config.rs` — possible new field for the ingress subfolder name, if made
  configurable rather than fixed.

**Remove now-dead upload-staging machinery** (nothing should write to `DATA_HOME/upload/` anymore):
- `tasks/actor/delete_in_update.rs` — whole file (the `path_starts_with_upload` check on the original
  `path` passed to `DeleteTask` in `index_for_watch` becomes permanently false for everything once
  uploads stop staging through `upload/`).
- `workflow/mod.rs::index_for_watch` — drop the `DeleteTask::new(...)` call.
- `operations/initialization/folder.rs` — stop creating `upload/`.
- `tasks/actor/folder_import.rs::internal_subtree_roots` — drop `"upload"` from the excluded-name
  list (keep `"db"`/`"object"` — still relevant for the legacy single-folder layout where
  `IMAGE_HOME`/`DATA_HOME` coincide).

**Alias-pruning visibility (the third decision above):**
- `tasks/actor/deduplicate.rs::deduplicate_task` — add the two targeted `warn!`s described above.

**Docs/comments to fix** (currently describe the wrong architecture, written earlier this session
before this was caught):
- `public/constant/storage.rs` — `get_data_path`'s doc comment lists `object/imported/` as
  irreplaceable data; remove that mention once it no longer exists.
- `TODO.md`'s `UROCISSA_STATE_HOME` note (same file, search `object/imported/`) — update once this
  lands.
- `docs/CONFIG.md` — note the no-migration decision (existing `object/imported/` is now dead,
  harmless, manually-deletable); document the upload-ingress-subfolder setting.

### Test plan — done

Added to `gallery-backend/src/tests/e2e.rs`:
- `scenario_v` — `GET /object/imported/<hash>.<ext>` serves the live file via the real
  prefetch → get-data → object/imported flow, including after `assign_album` moves it.
- `scenario_w` — identical content at two simultaneously-existing paths is tracked as two live
  aliases (the one case scenario_m/r didn't already cover).
- `scenario_x` — upload with no `presignedAlbumId` lands in the ingress folder under `imagePath`
  and is then reachable via `assign_album` (regression-locks the bug found above), via the real
  multipart `/upload` endpoint.
- `scenario_y` — upload *with* `presignedAlbumId` writes directly into that album's real
  directory, via the real multipart `/upload` endpoint.

Skipped: asserting on the new warning log lines directly (not practical with this project's
current test setup, no log-capture infra) — covered behaviorally instead (scenario_m/r/w already
exercise both the pruning and new-alias code paths; the warnings are an observability layer on
top of unchanged, already-tested behavior).

Not done: live deployment verification (no docker daemon in this dev environment) — build,
real `imagePath` with subdirectories, upload with/without a target album, confirm
`object/imported/` is never created, confirm download/view-original works in a browser.

### Sequencing — done, 4 commits

1. Switch all `imported_path()` readers to `source_path()` (no behavior change yet).
2. Remove `CopyTask` + the `imported_path()` methods + rework `get_img.rs` serving.
3. Add the alias-pruning/duplicate-content `warn!`s to `deduplicate_task`.
4. Rework `post_upload.rs` to write into `IMAGE_HOME`; remove the now-dead upload-staging
   machinery (`delete_in_update.rs`, `folder.rs`'s `upload/` creation, the
   `internal_subtree_roots` entry, `MAX_DELETE_ATTEMPTS`).
5. This docs pass (`storage.rs`, `CONFIG.md`, this entry).

- wrap image repo file operations and DB updates into a single transaction
  - the idea is to extend the DB consistency assurance to the photo repo
  - similar to journaling filesystems, a marker (mutex? file log) should record the planned transaction, perform the DB update, then file update, then release the transaction lock
  - if we crash before modifying files, we can detect the unfinished transaction and either rollback or complete, thus ensuring consistency
  - maybe there are better patterns and tooling to do this efficiently. but it should work on any (local) filesystem

## Github fork

- [ ] work on github fork for more compatible CI + contributing back
- [x] fork main
- [ ] push all changes to a dev branch
- [ ] refactor changes to first apply devops, tests, audit fixes, then merge / compress the album stuff?

## Docker / Deployment (assessed 2026-06-15, reworked 2026-06-16)

### Done (2026-06-16) — NOT YET TESTED, no docker daemon available in the dev environment

- `Dockerfile` moved to repo root (was `docker/Dockerfile`); `.dockerignore` moved to repo root
  too and fixed to match the actual `gallery-backend`/`gallery-frontend` directory names (the old
  `docker/.dockerignore` referenced nonexistent `frontend/`/`backend/` dirs and, being outside the
  build context root, was never actually applied by `docker/build-push-action`'s `context: .`).
- Image now builds with `--features embed-frontend` (single self-contained binary), eliminating
  the reason the old entrypoint `mv` pattern existed (it recreated a `gallery-backend`/
  `gallery-frontend` sibling-directory layout under `UROCISSA_PATH` so the non-embedded build's
  hardcoded `../gallery-frontend/dist` relative path would resolve). No more entrypoint script —
  `ENTRYPOINT ["/app/urocissa"]` directly.
- Runtime image sets fixed `UROCISSA_CONFIG_HOME=/config`, `UROCISSA_DATA_HOME=/data`,
  `UROCISSA_IMAGE_HOME=/images`; `compose.yaml` (repo root) bind-mounts host dirs onto
  these instead of the app autodetecting or files being moved at startup.
- `compose.yaml` added at repo root, replacing `run_urocissa_docker.sh` (~230 lines of
  `sed`/`grep` JSON scraping of `syncPaths`/port — also now stale since `syncPaths` doesn't exist
  anymore). `run_urocissa_docker.sh` deleted; README's Docker quick-setup section now uses
  `docker compose up -d`.
- `.github/workflows/dev-docker-build.yml`/`docker-build.yml` updated to build `./Dockerfile`
  (were `./docker/Dockerfile`). These push to `hsa00000/urocissa` (upstream's Docker Hub repo);
  this fork has no push access there, so these workflows are
  presumably inert for this fork's branches as-is.

### Remaining / open

- [ ] **Verify the rework actually works** — no docker daemon in the dev environment this was
  written in, so none of the above has been build- or run-tested. Before relying on it: `docker
  compose build`, confirm the image builds and `embed-frontend` actually picks up the frontend
  dist copied into the builder stage; `docker compose up`, confirm config/data/images land in the
  bind-mounted host dirs and the app is reachable; confirm `imagePath` set to `.` (or any relative
  value) resolves against `/images` inside the container as expected.
- [ ] Publish to `ghcr.io/codesam/urocissa` (GitHub Container Registry) — free for public images,
  co-located with source; complement or replace Docker Hub `hsa00000/urocissa`, which this fork
  can't push to.
- [ ] Add a systemd `.service` file in `deploy/` or `contrib/` for users running the binary
  directly (non-Docker), now that storage locations are env-var-driven
  (`UROCISSA_CONFIG_HOME`/`UROCISSA_DATA_HOME`/`UROCISSA_IMAGE_HOME` set in the unit file rather
  than relying on autodetection).
- [ ] `UROCISSA_STATE_HOME`: `UROCISSA_DATA_HOME` holds `db/index_v5.redb` (irreplaceable —
  the only store of record for tags/album-assignments/flags) alongside genuinely disposable
  cache files (`db/cache_db.redb`, `db/temp_db.redb`, `db/expire_db.redb`). Splitting the latter
  into a proper state directory would make "what's safe to delete on reset" explicit instead of
  implicit in file names. Bigger change than the env var rename — touches each redb file's path
  individually (`tree_snapshot/new.rs`, `query_snapshot/new.rs`, `expire/new.rs`). Not done yet.
  (`object/imported/` and `upload/`, previously listed here, no longer exist — see "Storage
  architecture fix" above.)

---

## Code conventions and tooling notes

### Pre-commit hook order
`just precommit` runs in this order: `cargo fmt --check` → `cargo clippy` → `cargo nextest` → `prettier --check` → `vue-tsc + eslint` → `vitest`. Run `cargo fmt` and `npx prettier --write` before staging to avoid the most common late failures. ESLint and vue-tsc run after prettier, so formatting errors mask type/lint errors until fixed.

### Error code policy (backend)
`ErrorKind::NotFound` maps to HTTP 404, which Rocket also returns for unregistered routes. Using `NotFound` for domain-level "entity not found" makes those errors indistinguishable from routing failures in tests and clients. Use `ErrorKind::InvalidInput` (→ 400) when the route succeeded but a referenced entity does not exist. Only use `NotFound` when a routing-level 404 is genuinely the right signal.

### ESLint strict mode (frontend)
`@typescript-eslint/strict-boolean-expressions` and `@typescript-eslint/no-non-null-assertion` are enforced. These only surface at commit time (pre-commit hook). When writing TypeScript: use `=== null` / `=== undefined` instead of truthiness checks on nullable strings; avoid `!` non-null assertions — use an explicit null/undefined guard branch instead.

---

## Album feature — open items

- [ ] **E2E test for `create_dir_album`** — endpoint has no scenario coverage. Should verify: subdir is created on disk, returns new album ID, parent album updates.
- [ ] **Stale `DIR_ALBUM_CACHE` scenario** — if a directory is deleted externally while the cache still holds its entry, `assign_album` will attempt to move a file into a non-existent path. Decide: detect and evict stale entries at startup, or return a clear 400 with a meaningful message at request time.
- [ ] **`assign_album` file-move verification in tests** — Scenario H checks album membership after assignment but does not verify the file moved to the correct directory on disk. Add a filesystem assertion to close this gap.

---

## Robustness: FS/DB consistency reporting (planning — not yet implemented)

Status: **plan only**. No code changes yet. Once agreed, implementation should add a
"Robustness strategy" section to `docs/design.md` documenting the pattern below (not
the survey/rationale — that stays here), and extend `docs/test-strategy.md`'s
regression matrix with rows for the hook points listed.

### Problem

`gallery-backend` treats the filesystem and redb as two systems that are supposed to
stay in sync (file paths recorded in `alias`/`dir_path` vs. what's actually on disk),
but nothing currently watches for them drifting apart except the narrow fixes already
made for `alias` pruning (see "Known bugs", now fixed). Drift happens for ordinary
reasons — files edited/moved/deleted outside the app, races between the watcher's
debounce window and a concurrent edit, directories removed while their dir-album
record still exists — and today the system's response to each is inconsistent: some
cases self-heal silently, some panic, some become an opaque 500 with no indication
that the *cause* was a consistency problem rather than a generic bug, and none of them
are visible to the user in a way that distinguishes "this still worked, but you should
know X" from "this failed because of X."

### Survey of concrete situations

| # | Location | Scenario | Today | Should be |
|---|---|---|---|---|
| 1 | `deduplicate_task` (`tasks/actor/deduplicate.rs`) | Rediscovering a hash at a path that doesn't match any *other* existing record, but that record's `alias` already had dead entries pruned this run (just-fixed behavior) | Silent (now correct, but no log line distinguishes "pruned a stale entry" from "nothing to prune") | `info!` log noting how many dead aliases were dropped — expected/self-healing, not Discord-noised |
| 2 | `deduplicate_task` | A file's content changes *in place* at an already-tracked path (new hash discovered at a path some *other* existing record's `alias` still references) — that other record now has a dangling alias nobody prunes, since pruning only happens on the *same* hash's own re-discovery | Not handled at all — silent DB drift, two records can claim the same physical path | New: cross-record stale-alias detection when indexing a brand-new hash; `warn!` + prune the stale reference in the old record |
| 3 | `assign_album.rs` | `get_dir_path_for_album` returns `None` because the target album's directory was deleted externally after the album record was created (stale `DIR_ALBUM_CACHE` entry — already flagged above) | Generic `ErrorKind::InvalidInput` 400, "Album not found in dir cache" | Distinguish "never was a dir album" (genuine bad request) from "was one, directory now missing" (consistency problem) — different message, `warn!` logged server-side either way |
| 4 | `assign_album.rs` | `fs::rename` fails for a reason other than the already-checked "source missing" (e.g. `EXDEV` cross-device, permission denied, destination collision) | Generic `ErrorKind::Internal` 500 | Classify by `io::ErrorKind`; cross-device/permission issues are operator-fixable, not "internal bug" — should be 4xx-ish with an actionable message |
| 5 | `workflow::index_for_watch` → `OpenFileTask` | File is deleted between the watcher's `Create` event firing and the (debounced + queued) indexing attempt running — a normal race for *any* filesystem watcher, not a bug | After 3 retries, hard error → `handle_error` → Discord webhook noise | Recognize `ErrorKind::NotFound`-shaped IO errors here specifically and downgrade to `info!`/`debug!`, no Discord notification — this is expected, not exceptional |
| 6 | `tasks/actor/album.rs::album_task` | A dir-album's `dir_path` directory is deleted externally; the album record has no live members but is never cleaned up, and `DIR_ALBUM_CACHE` keeps pointing at a dead path forever (same root cause as #3) | Silent — album just sits at `item_count: 0` indefinitely | `warn!` once detected (e.g. during `album_task` or a periodic sweep) that a dir-album's directory no longer exists; surface in `/get/get-albums` as a flag the frontend can badge, rather than only failing later when something tries to use it (#3) |
| 7 | `router/put/reindex.rs:50` | `reduced_data_vec.get_hash(*index).unwrap()` — panics if `index` is out of range for the snapshot (e.g. the snapshot is stale relative to a concurrent delete) | `.unwrap()` panic — Rocket likely converts it to a generic 500 with no structured logging at all | Convert to `or_raise(ErrorKind::Conflict, ...)` ("snapshot is stale, re-fetch and retry") — ties into the broader effort to retire `.unwrap()` call sites (see "Backend tooling" below) |
| 8 | Any two concurrent read-modify-write handlers on the same hash (e.g. `edit_tag` + `rotate_image` + the watcher's own re-index, all racing) | Each handler reads the full `AbstractData`, mutates one part, writes the whole thing back — classic lost-update: B's write can silently discard A's concurrent change | Not detected at all; redb's transaction isolation prevents *corruption* but not *lost updates* | Out of scope for the immediate pattern (bigger change — would need optimistic concurrency, e.g. an `updated_at`/version check on write, or per-hash locking extended from indexing's existing `IN_PROGRESS` guard to API handlers). Flag here so it isn't forgotten; not a "warning to surface," a correctness gap. |
| 9 | `get_prefetch.rs` / `expire_check.rs` (the snapshot-expiry bug, "Known bugs" above) | A `prefetch` snapshot is deleted within milliseconds instead of living ~1 hour, so the next `/get/get-data` 500s with "Failed to open tree snapshot table" | Hard 500, `ErrorKind::Database`, no indication this is a known/transient consistency issue vs an unrelated DB problem | Good first real consumer of this pattern once it exists: reclassify as a distinct, recoverable kind the frontend can react to by silently retrying (it already knows how — `prefetch` is idempotent), rather than just displaying a raw error |
| 10 | General | ~140 existing `clippy::unwrap_used` call sites across the backend (tracked separately under "Backend tooling") | Each is a potential ungraceful panic on exactly this kind of unexpected state | Treat consistency-pattern adoption and `unwrap_used` cleanup as overlapping efforts — most of the call sites worth fixing first are the ones touching redb lookups / filesystem state, i.e. exactly this list |

### Classification

Every situation above falls into one of three buckets; the pattern needs to make the
bucket explicit in code, not just in a comment:

1. **Expected, self-healing** (#1, and the alias-pruning/dead-watcher-race fixes already
   shipped) — the operation completes normally; log at `info!`/`debug!` for
   observability, never surfaced to the user, never Discord-noised.
2. **Unexpected but recoverable** (#2, #3 "was a dir album", #6, #9) — the operation
   either completes with a caveat or fails in a way the user can act on (re-index,
   re-pick a target, retry). Needs both a structured server-side log (`warn!`,
   Discord-noised — this is the kind of thing that indicates real drift worth a human
   noticing) *and* a way for the relevant API response to say "warning: X" rather than
   either silent success or an opaque error.
3. **Unexpected and blocking** (#4, #5 non-NotFound branch, #7) — the operation cannot
   complete. Already representable via `AppError`/`ErrorKind`; the gap is precision
   (today these mostly collapse into `Internal`) and consistent logging discipline
   (some go through `handle_error`, some don't, with no obvious rule for which).

### Proposed backend pattern

- Add `ErrorKind::Inconsistency` for bucket 3 cases that are specifically *FS/DB
  drift*, not generic bugs — lets the frontend branch on `kind` to show a different
  message/icon than for, say, `ErrorKind::InvalidInput`. Maps to a 409 or 500
  depending on the case; decide per call site.
- For bucket 2, the open design question (flag for review, don't decide unilaterally
  here): either (a) extend success responses with an optional sibling field —
  e.g. wrap relevant endpoints' `AppResult<Json<T>>` in a small
  `Json<WithWarnings<T>> { data: T, warnings: Vec<String> }` envelope only where
  needed, keeping today's plain `T` for everything else — or (b) keep responses as
  today and rely purely on server-side logging + a *separate* lightweight polling/log
  surface for admins, accepting that end users won't see bucket-2 warnings inline.
  (a) is more work but is what "the user dialog should also report this" in practice
  requires for the album/tag-assignment flows specifically called out below.
- A single logging helper (next to `handle_error`/`handle_app_error` in
  `public/error_data.rs`) — e.g. `log_consistency(severity, context: &str, detail:
  impl Display)` — so every hook point uses the same call shape and the
  Discord-webhook-or-not decision lives in one place (mirroring the existing
  `handle_app_error` filter-by-`ErrorKind` logic), instead of each call site deciding
  ad hoc.

### Proposed frontend pattern

- Add `'warning'` to `MessageColor`/`messageStore.ts` (currently `error` / `success` /
  `info` only) and a `messageStore.warning(text)` action — Vuetify already has a
  `warning` theme color, so this is additive, not a new design.
- For the two flows the request calls out by name — `AssignAlbumModal.vue` (album
  assignment) and the tag editor (`PUT /put/edit_tag` call sites) — branch on the
  response: hard failure → `messageStore.error` (already happens); success with a
  `warnings` array (once the backend envelope from the bullet above exists) →
  `messageStore.warning` for each entry, *in addition to* showing the operation as
  completed (don't suppress the success state, since bucket 2 means it *did* succeed).
- Longer term, the same envelope/branching should apply to any other mutating call
  site that can hit a bucket-2 situation (reindex, delete, rotate) — start with
  album/tag assignment since that's what surfaced this need, generalize once the
  pattern is proven there.

### Sequencing (for whenever this gets implemented — not now)

1. Backend: add `ErrorKind::Inconsistency`, the `log_consistency` helper, and the
   `WithWarnings<T>` envelope (or the chosen alternative from the open question above).
2. Wire hooks #1–#3, #5, #6, #9 from the survey table (the ones that are pure
   classification/logging changes, no new detection logic needed).
3. Add the cross-record stale-alias detection for #2 (the one genuinely new piece of
   logic).
4. Frontend: add the `warning` message color/action; wire `AssignAlbumModal.vue` and
   the tag editor to surface `warnings` from responses that carry them.
5. Document the resulting pattern in `docs/design.md` (a new "Robustness" subsection,
   alongside "Keep it robust, low footprint, modular") and add the corresponding rows
   to `docs/test-strategy.md`'s regression matrix, with one `scenario_*` test per hook
   point in the survey table (most can reuse the existing fixture techniques — delete
   a file mid-flow, point a dir-album at a missing directory, etc.).
6. Re-fix #9 (the snapshot-expiry bug) using the new pattern as the worked example,
   instead of the current hard 500.
7. #8 (lost updates under concurrent writes) is explicitly *not* in this sequence —
   it needs its own design discussion (optimistic concurrency vs. locking) and
   shouldn't block the consistency-reporting pattern above.

---

## Testing — best value items

### Backend unit tests (low effort, no DB needed)
- [x] `prettify_dir_name` — pure string transform, dir_album.rs
- [x] schema version round-trips — encode v1/v2, decode, check fields; ser_de.rs
- [x] `Expression` filter predicates — construct filter, run against fixture AbstractData; generate_filter.rs
- [x] `compute_timestamp` priority logic — abstract_data.rs
- [x] `belongs_to_album` path-prefix logic — combined.rs (dir vs manual branching)

### Backend integration tests (need tempdir redb — not as hard as it sounds)
- [ ] index → dedup → flush → album self-update round-trip
- [ ] dir album path-prefix membership (create album, check file in/out of subtree)
- [x] schema migration: write v1-encoded record, read back as v2 (through redb)

### Backend tooling (trivial)
- [x] deny(unsafe_code) — enforced at compile time via main.rs
- [x] cargo nextest — installed, replacing cargo test in justfile
- [x] cargo audit — in justfile (`just backend-audit`); known-unfixable advisories suppressed in `.cargo/audit.toml` with enforcement tests
- [x] cargo deny — deny.toml in place; wired into just audit
- [x] tighten clippy: unwrap_used = warn in Cargo.toml; excluded from -D warnings until call sites are cleaned up
- [ ] cargo geiger — include in release/audit reports (unsafe surface area of dep tree)
- [ ] semi-regular security audit CI job — run `just audit` on a schedule (monthly or on dep changes); options: Codeberg CI (Woodpecker), GitHub Actions on the upstream fork, or a local cron on the dev machine; the `/security-audit` skill covers the manual review workflow when issues are found

### Frontend unit tests
- [x] Vitest — installed; 34 lexer tests covering all atom types, compound operators, escaping, and lex errors
- [ ] Pinia store reducers — pure logic in stores, no DOM needed

### Frontend tooling
- [ ] knip — periodic dead-export sweep, not in precommit
- [x] npm audit — in justfile (`just frontend-audit`); all vulnerabilities resolved by bumping to latest deps
- [ ] dependency updates — establish a regular cadence (npm-check-updates, Dependabot, or Renovate); small frequent updates are cheaper than batched drift
- [ ] Zod (or valibot) — investigate for runtime validation of API responses; TypeScript types don't catch backend schema drift at runtime

### Known bugs
- [ ] **Grey placeholders on initial load** — `useElementSize` reports width 0 on first render; `usePrefetch` skips the fetch (guard: `windowWidth > 0`); the Home.vue watch then fires a row fetch before `prefetch()` sets the correct timestamp, so rows arrive stale and are discarded. A zoom or resize re-triggers the cycle cleanly and images appear. Root: `useElementSize` ResizeObserver fires after the first render tick, too late for the initial fetch path. Fix: ensure `prefetchStore.windowWidth` is set before the first `fetchRowInWorker` call, or delay the Home.vue watch fetch until after `processPrefetchChain` completes.
- [x] **Info sidepane always shows "No album" until manually (re-)assigned** — fixed. `workflow::index_for_watch` now resolves the file's parent directory's dir-album (`get_album_id_for_dir`, falling back to `ensure_dir_albums`'s sync-root walk for brand-new directories) and passes it through as the indexed item's explicit `album` field, the same way `presigned_album_id` already did for direct uploads. Test: `scenario_l_dir_album_membership_not_set_at_index_time` (now green).
- [x] **Alias entry duplicates on every `assign_album` reassignment** — fixed. `deduplicate_task` now prunes alias entries whose file no longer exists on disk and skips pushing a path that's already present. Test: `scenario_m_watcher_reindex_after_assign_duplicates_alias` (now green).
- [x] **Stale alias entries accumulate when a tracked file is moved outside the app** — fixed by the same change as above. Test: `scenario_r_externally_moved_file_keeps_dead_alias_entry` (now green).
- [x] **Tags are never discovered at index time** — fixed for the common case. `extract_keywords_from_xmp` (`operations/indexation/extract_keywords.rs`) now scans for an embedded `<dc:subject>` XMP element and adds its `<rdf:li>` entries as tags; wired into both `process_image_info` and `process_video_info`. Tests: `extract_keywords::tests::*` and `scenario_n_tags_not_discovered_from_xmp_keywords_at_index_time` (now green). Still only handles uncompressed/contiguous XMP — see the format-coverage item below for what's not handled yet (compressed PNG `zTXt`, MP4/MOV `uuid` boxes, binary IPTC IIM, video-pipeline test coverage).
- [ ] **`prefetch` snapshots can be deleted almost immediately instead of living for 1 hour** — found while stabilising the new e2e tests (was causing real, intermittent 500s on `/get/get-data` right after `/get/prefetch`, not just test flakiness). `Expire::expired_check` (`public/db/expire/expired_check.rs:77`, `None => true`) cannot distinguish "this timestamp was never recorded" from "this is the *current* `VERSION_COUNT_TIMESTAMP`, whose expiry hasn't been scheduled yet" (`update_expire_task` inserts `expire_table[current_timestamp] = None` deliberately, as a placeholder). Both collapse to the same `None` arm. Since `flush_query_snapshot_task` names each on-disk `QUERY_SNAPSHOT` table after the *current* `VERSION_COUNT_TIMESTAMP` at flush time, the very next version bump anywhere in the process (any concurrent indexing/edit/reindex) makes `expire_check_task` see `VERSION_COUNT_TIMESTAMP > that_table's_timestamp` and call `expired_check` on it — which returns `true` immediately via the `None` arm instead of waiting ~1 hour, deleting the query snapshot table and cascading (via the `RemoveTask` it schedules for every `Prefetch.timestamp` row inside) to delete the just-created `TREE_SNAPSHOT` table too. Net effect: under any concurrent write activity, a freshly-`prefetch`'d snapshot can vanish within milliseconds, and the next `/get/get-data`/`/get/get-rows` call against it 500s with "Failed to open tree snapshot table". Worked around in tests via `read_current_abstract_data` (re-prefetch-and-retry instead of reusing a timestamp); not worked around in the frontend. Fix: distinguish "unrecorded" from "active, not yet scheduled" — e.g. store expiry as a tri-state, or only ever insert the *current* version's row once its *own* successor exists, or check `entry.is_none()` separately from "row absent" before defaulting to "expired".
- [ ] **Enable metadata-extraction tests for all supported file formats** — `extract_keywords_from_xmp`'s current (stub) substring-scan approach is format-agnostic by construction, but only `scenario_n` (JPEG, hand-spliced APP1/XMP segment) exercises it end-to-end. Supported extensions today (`public/constant/mod.rs`): images `jpg, jpeg, jfif, jpe, png, tif, tiff, webp, bmp`; videos `gif, mp4, webm, mkv, mov, avi, flv, wmv, mpeg`. XMP/IPTC packet location and encoding differ by container — PNG can store text in compressed `zTXt` chunks (the naive scan would miss it), MP4/MOV embed XMP in a `uuid` box, TIFF has no `APPn` segment concept, and IPTC IIM (the older, non-XMP keyword mechanism some tools still write) is a separate binary format entirely. Once real extraction is implemented, extend coverage with one `scenario_n`-style test per representative container (at least: PNG with an uncompressed `iTXt` XMP packet, PNG with a compressed `zTXt` one, MP4 with a `uuid` XMP box, and a plain IPTC-IIM-only JPEG) rather than assuming the JPEG case generalises. Video-pipeline coverage additionally needs `ffmpeg`/`ffprobe` available in the test environment (process_video_info shells out to both), which `scenario_n`'s image-only path avoids.

### Deferred
- [ ] Playwright E2E — needs running backend; save for CI
- [ ] cargo bench / criterion — indexing pipeline regression benchmarks (file hashing, EXIF extraction, thumbnail generation, redb write throughput, serving latency under load)
- [ ] Admin status API — expose queue depth, active indexing jobs, recent errors, per-job progress as a JSON endpoint (foundation for a future admin panel)
- [ ] Frontend progress UI — poll the existing `/get/import/folder/status` pattern (or a future SSE stream) to show active indexing progress to authenticated users
- [ ] Structured logging — investigate `tracing` + `tracing-subscriber` as a replacement for `env_logger` to support spans, JSON output for log aggregators, and `tracing-journald` for native systemd/journald integration
