---
status: in-progress
type: feature
priority: high
area: backend
---

Tracked in `pre01.md` (meta) as 1c.

## Feature status

The `assign_album` conflict feature is **implemented and has passing scenario coverage** (detailed below), but its
conflict model is being **redesigned** (2026-09-16): the `OnConflict` surface narrows to **`rename` | `merge`** on both
assign and upload — `skip` and `replace` are being removed. The decisions are recorded under G2 below (model,
dir-album recursion, required `on_conflict`, outcome reporting); implementation has not started. G1 is resolved (record
in Open decisions); G3 is still open.

`OnConflict` = `rename` / `merge` (redesign target; today the code still has `skip`/`replace`).

- Item move (`move_item_into_album`): today check `dest_path` exists → `skip` returns early (200 no-op), `rename`
  picks `find_unique_path` (`-001`, `-002`, …), `replace` overwrites the existing file with **no hash check** (see
  G2). Sidecar moves to `dest_path.with_extension("xmp")`.
- Sub-album dir move (`move_album_into_album`): whole-dir `fs::rename`; `skip`/`rename` behave like items; **`replace`
  is rejected** (400). Guarded against moving an album into itself or its own subtree. Rewrites every DB record under
  the old dir (nested albums and all file aliases), updates the dir-album cache prefix, and marks old/new parent albums
  for stats refresh.
- Upload route `POST /post/upload?presigned_album_id&on_conflict&auto_rename` has its own inline `save_file` conflict
  handling (strings parsed strictly: `skip|rename|replace`, else 400).

## Routes & interaction vectors

- `PUT /put/assign_album` — accepts image, video, or dir-album hashes. Reached from `ItemAlbum.vue` (metadata panel),
  `ItemEditAlbums.vue` (single menu), `ItemBatchEditAlbums.vue` + `AssignAlbumModal.vue` (batch = sequential loop of
  independent calls).
- `POST /post/upload` — direct upload into an album directory, sharing the conflict strategies (today
  `skip|rename|replace`, redesign `rename|merge`).
- Frontend sequences without a server-side transaction:
  - trash-restore → move (`setTrashed(false)` then `assign_album`),
  - create-album → move (`createDirAlbum` then `assign_album`).

## Implemented & passing scenario tests

### assign\_album conflict semantics (`assign_conflict_*`)

> Redesign: the `skip`/`replace` scenarios below are **obsolete** — `skip` and `replace` are removed from
> `OnConflict`; `rename` scenarios survive. Replace them per G2 (conflict model).

- **`assign_conflict_default_skip`** — (obsolete) no `onConflict` in body → skip: source `import/photo.jpg` and dest
  `album/photo.jpg` both remain on disk (default is safe, no silent overwrite).
- **`assign_conflict_skip_z4`** — (obsolete) explicit `skip` → 200, both files remain.
- **`assign_conflict_rename_z5`** — `rename` → source gone, `album/photo-001.jpg` created, existing
  `album/photo.jpg` untouched.
- **`assign_conflict_rename_double_z7`** — `photo-001` taken → next free suffix `photo-002.jpg` used; blockers
  untouched.
- **`assign_conflict_replace_z6`** — (obsolete) `replace` → 200, source gone, dest overwritten (asserted via file
  presence only — see Gap 2).

### sub-album / directory moves (`assign_album_dir_conflict_*`)

> Redesign: `skip`/`replace` variants below are **obsolete**; dir moves gain the recursive leaf→root content migration
>
> - parent-dir cleanup model (G2).

- **`assign_album_dir_conflict_skip_zz8`** — (obsolete) dir-name collision → both source and target dirs intact,
  nothing moved.
- **`assign_album_dir_conflict_rename_zz9`** — collision → moved dir lands as `child-001/`, existing `target/child/`
  untouched, source gone.
- **`assign_album_dir_conflict_replace_rejected_zza`** — (obsolete) `replace` on a dir → non-200; nothing moved or
  deleted.

### reject / error paths

- **`assign_album_rejects_manual_album`** — unknown/bogus album\_id → 400.
- **`assign_album_rejects_stale_file_path_j`** — source file removed from disk → non-200 (stale-alias guard).

### upload into album (`upload_conflict_*` — all upload the same source file twice)

> Redesign: first upload can no longer use `replace` (removed), and `skip` is also removed. The route keeps **`rename` |
> `merge`** only. Both `upload_conflict_skip` and `upload_conflict_replace` are **obsolete**; a same-content second
> upload is now `merge`, which dedups to the existing alias instead of rewriting it.

- **`upload_conflict_skip`** — (obsolete) first upload `replace`, second `skip` → 200; `photo.jpeg` exists
  (**weak**: true even if skip overwrote).
- **`upload_conflict_rename`** — `rename` on second → both `photo.jpeg` and `photo-001.jpeg` exist.
- **`upload_conflict_replace`** — (obsolete) both `replace` → 200; `photo.jpeg` exists (same content both times).

### surrounding assign behavior (not conflict-specific)

`xmp_sidecar_moves_with_file_z2` (sidecar follows move), `complex_tags_survive_assign_y` (tags/metadata preserved),
`assign_album_moves_sub_album_directory_zz7` (nested subtree + rewritten paths),
`assign_album_moves_multiple_independent_albums_zzb`, `assign_album_updates_album_tree_parent_zza`,
`assign_album_move_clears_source_grid_zzc` (grid cache), `album_visible_via_get_data_after_assign_q`,
`image_serving_survives_album_move_v`, `album_membership_singular_i`, `assign_multiple_files_to_album_z8`.

### frontend UI (Playwright)

`assign-photo-to-album.yaml` — single photo moved via the sidebar modal; asserts success toast + sidebar album chip
reflects destination. **No** batch, trash-restore→move, or create-album→move flow is covered.

## Test changes under the conflict redesign

### Deprecated — delete with the old code

- `assign_conflict_default_skip.yaml` (default behavior gone: `onConflict` becomes required).
- `assign_conflict_skip_z4.yaml`, `assign_conflict_replace_z6.yaml`.
- `assign_album_dir_conflict_skip_zz8.yaml`, `assign_album_dir_conflict_replace_rejected_zza.yaml`.
- `upload_conflict_skip.yaml`, `upload_conflict_replace.yaml`.

### To update with the implemented model

- Every `assign_*` scenario call body gains the now-required `alias` field (the selected file's path; missing → 4xx).
- `assign_conflict_rename_z5.yaml`, `assign_conflict_rename_double_z7.yaml` — survive as `rename`-mode coverage
  (unchanged for differing content). Add a same-content rename variant to pin the documented "duplicate aliases of same
  content in one album" outcome.
- `assign_album_dir_conflict_rename_zz9.yaml` — survives.
- `upload_conflict_rename.yaml` — survives (upload keeps `rename`).
- All conflict scenarios assert the per-file outcome field (see response contract below).

### New — critical (implement alongside the redesign)

1. `assign_merge_dedup_different_name` — identical content already in the target album under another name → selected
   source removed, no file lands, response `deduplicated-removed`. **The headline merge behavior.**
2. `assign_merge_same_name_same_hash` — identical file at the same filename → collapses to one entry: no move, no
   rename, dedup.
3. `assign_merge_same_name_different_hash` — filename collision with different content → auto-rename
   (`photo-001.jpg`), dest untouched, never overwrite.
4. `assign_merge_verify_mismatch` — selected alias bytes corrupted vs its recorded hash → error, source **not**
   deleted (gap 3b).
5. `assign_merge_multi_alias_sibling_in_target` — G1 ∩ G2: record whose _other_ alias already lives in the target
   album → only the selected alias is removed; the sibling alias entry + file stay.
6. `assign_rename_same_hash_different_name` — pins the "dumb" rename outcome: duplicate alias created, both copies
   kept.
7. `assign_on_conflict_required` — missing `onConflict` → 4xx; legacy `skip`/`replace` values rejected (gap 7).
8. `upload_merge_same_content` — re-upload identical file → dedups to the existing alias; no second file on disk.
9. `assign_dir_merge_recursive` — sub-dir tree colliding with an existing target path: content migrates leaf→root,
   emptied dirs + their dir-album records removed. Variants: nested subdirs; a blocked/failed file keeps the containing
   dir **and all its ancestors**; self/subtree guard still 400.
10. `assign_self_move_noop` — assign into the album the file already lives in → 200 no-op under both modes (gap 5).
11. Response contract — each branch returns its outcome (`moved` / `renamed-from` / `deduplicated-removed` / error),
    and the frontend maps it to toast/chip state (G3).

### DSL limitation — RESOLVED (2026-09-16)

get-data/prefetch responses surface only the last alias (transitor trims the alias list), so merge's alias-entry
removal — and G1's sibling-preservation — are not observable through the API response. **Decision:** add a **test-only
DB probe** — a gated endpoint returning a record's full `alias[]`, 404/403 unless enabled via env (`PICASU_TEST_PROBE=1`
guarded by debug assertions), driven from scenarios via the existing `when.call` verb. Landed as its own commit (C2)
before the merge/G1 scenarios are written.

## Suspected gaps & untested corner cases

1. **Multi-alias record move (data loss).** Dedup produces records with \>1 alias (`tasks/actor/deduplicate.rs`).
   `move_item_into_album` replaces the whole alias list with a single path (`assign_album.rs` move\_item), moving only
   `alias[0]`'s file and **dropping every other alias** — the remaining physical copies stay on disk but the DB stops
   referencing them. No scenario covers it, and the scenario DSL cannot assert `alias[]`/ `album()` contents (only file
   existence and response JSON), so DB-level consequences are invisible to the suite. **Decision recorded under G1**;
   this gap becomes the coverage proving the fix (assert sibling alias file still on disk and the surfaced file moved).
2. **~~`replace` across different hashes (integrity)~~ — RESOLVED.** The byte-clobber `replace` is removed entirely
   (G2); there is no longer any code path that writes different bytes over an existing indexed path. Hash = BLAKE3 of
   content (`process/hash.rs`).
3. **~~Replace clobbers the dest record's `.xmp` sidecar~~ — RESOLVED.** With `replace` gone there is no
   sidecar-overwrite path. Under merge, a deduped source copy is deleted together with its sidecar (the shared record
   keeps metadata, per `docs/design.md` "allow drifted sidecar files"); under rename the sidecar moves with the file.
   Both paths need scenario coverage, not design fixes. 3b. **Merge dedup safety.** Merge claims two paths are identical
   based on the shared hash; the design adds an explicit on-disk sanity check (bytes still match the recorded hash)
   before deleting the redundant source copy. This check and its failure mode (must NOT delete on mismatch) are
   untested. 3c. **Dir-album recursive move + parent cleanup (new).** The leaf→root content migration, empty-dir
   removal, and "never remove a non-empty dir (nor its ancestors)" rule for sub-album merges are entirely new behavior
   with no coverage; extensive scenarios required (also: sidecar/MAC collision when a file lands in a dir whose target
   path already exists).
4. **Self-descendant guard via API.** `target_dir.starts_with(&source_dir)` (move\_album) has no direct test — only
   the client-side ancestor + descendant _selection_ is covered
   (`assign_album_moving_ancestor_then_descendant_extracts`). A direct PUT of an album into its own subtree (expect 400,
   untouched) is unasserted.
5. **Dest == current path (self-move).** Moving a file into the album it already lives in — `base_dest !=
current_path` branch makes it a 200 no-op today; stays a no-op under both redesign modes (new test 10). The UI
   disables it but the API path is untested.
6. **Stale album-directory cache.** The `album_dir.is_dir()` → 400 guard is untested; the stale test only covers a
   missing _source file_, not a missing _album directory_.
7. **`on_conflict` validation.** Under the redesign the field is required and restricted to `rename|merge`: missing
   → 4xx; legacy `skip`/`replace` rejected (assign: serde 4xx; upload: strict string-parse 400). Untested (new
   test 7).
8. **Dir-vs-file and rename-onto-nonempty-dir collisions.** POSIX errors surface as 500 (GenericFile upgrade). Untested.
9. **Cross-device / permissions (EXDEV, EACCES, read-only FS).** Upload side has `upload_readonly`; the assign side has
   none. A mid-operation failure leaves unrecovered temp/partial state.
10. **Concurrency.** Suite is serialized by `TEST_SERIAL_GUARD`; no test covers assign-vs-assign on the same item,
    assign-vs-delete, or assign-while-upload into the same dir.
11. **Frontend/UI.** Batch assign, trash-restore→move (incl. the half-failed state where untrash succeeded but move
    failed), create-album→move, batch partial-failure recovery, and the silent-skip misreport (G3) are all uncovered
    in Playwright.

## Open decisions

- **G1 — multi-alias move — RESOLVED (2026-09-16).** The assign operation is implemented on a concrete image in a
  concrete directory: the **selected alias**. A move must affect only that alias; sibling aliases sharing the same hash
  stay where they are (their files remain on disk, and their DB entries are untouched). This mirrors the delete
  precedent: `delete_data` takes `aliasList` of per-item alias paths, validates each against the record, and prunes only
  that alias (`router/delete.rs`; frontend sends `item.alias[0].file` in `ItemPermanentlyDelete.vue:29-35`).

  Implied change, in `move_item_into_album` (`router/put/assign_album.rs`):

  - `AssignAlbumData` gains a **required** `alias` — the surfaced/concrete path, sent by the frontend from the same
    `item.alias[0].file` source as delete. Required, not optional: pre-v0.1, no backward-compat concerns; a missing
    `alias` is simply a 400. The server never picks an alias on the caller's behalf.
  - Backend validates the given alias is one of the record's aliases → else 400 (mirror `delete.rs:157-168`). The
    stale-file check applies to the _selected_ alias, not `alias[0]` (`assign_album.rs:323-331`).
  - Rewrite **only the matched alias entry** to the new dest path (kept entry's `modified`/`scan_time`/`is_trashed`),
    removing the whole-list replacement at `assign_album.rs:363-370`. Other alias entries and their files are left
    alone. No thumbnail/DB-removal path is needed — a move never empties the record.
  - Upload (`POST /post/upload`) is unaffected: new files land in place; there is no pre-existing record to select an
    alias from (dedup-merge may later _append_ an alias).

  Frontend pass-through is G3's concern — `assignAlbum()` (`frontend/src/api/assignAlbum.ts`) must start sending both
  `alias` and an `onConflict` strategy.

- **G2 — conflict model — RESOLVED (2026-09-16).** `OnConflict` is reduced to **`rename` | `merge`** on both assign
  and upload. The backend already knows every file's hash and record (aliases = paths sharing a record), so the old
  "infer conflict by filename" model is dropped: `skip` and `replace` are removed, and a move always completes (no
  leftover partially-moved sub-albums). `docs/design.md` "Moving / Deleting" updated to match.

  - `rename` — dumb and safe: move unconditionally; filename collision → `find_unique_path` (`photo-001.jpg`); may
    create duplicate aliases of the same content in one album. Never overwrites.
  - `merge` — smart:
    1. Dedup the selected alias against aliases of the same record (identical content) already inside the target album.
       Found → **verify** the source bytes still match the recorded hash, then delete the redundant source copy + its
       alias entry instead of moving it (content survives via the album-resident alias; sidecar dies with the copy). On
       verify mismatch → error, never delete.
    2. Remaining selected files move into the album, auto-rename on filename collision (a collision there implies
       different content, since same-hash was already deduped in step 1). Result: identical content is never duplicated
       inside an album; the old `replace`-same-hash case collapses into step 1; different-content collisions are only
       ever renamed.
  - Upload: keeps `rename` | `merge` only; "re-upload same file" = `merge` (dedups to the existing alias).
  - Sub-album (dir) moves: a sub-album is a path with display metadata; its identity is the path, it has no aliases of
    its own. When the target path already exists, move the content recursively **leaf → root**, then remove the
    emptied source directories together with their dir-album records. Any source directory that still contains files
    (failed/blocked move) is kept, and so are all of its ancestors. Requires extensive new coverage.
  - `on_conflict` is now **required** in the request body (no default), pre-v0.1. Missing value → 400.
  - Response must report per-file outcome (`moved` / `renamed-from` / `deduplicated-removed` / `not-moved-with-reason`)
    so the UI is never silent — see G3.

  Open sub-points — RESOLVED (2026-09-16):

  - Upload `on_conflict` stays optional; **absent → `rename`** (matches today's
    unique-suffix behavior); `merge` is opt-in via the query param.
  - Dedup lookup scoping: **the exact target album dir** (a record alias whose
    normalised path lives directly under `album_dir`), not the album's
    subtree.

- **G3 — frontend on\_conflict pass-through — decisions recorded.** `assignAlbum()`
  (`frontend/src/api/assignAlbum.ts`) sends neither `alias` (G1) nor an `onConflict` strategy; with `skip`/`replace`
  gone the "silent 200 no-op" is no longer a default, but the UI must still reflect merge outcomes
  (`deduplicated-removed` vs `renamed-from`) and send `alias` + the required `onConflict`. **Decision:** `AssignAlbumModal`
  gains a Merge (smart) / Rename (keep both) radio, **Merge default**; outcome maps to toast/chip (moved /
  renamed-from / deduplicated-removed); the batch flow (`ItemBatchEditAlbums.vue` + `AssignAlbumModal.vue`) surfaces
  per-file results. Implementation is commit C9.

## Race conditions to cover

- Same item assigned twice concurrently (redb write txn serializes; the second call likely errors "not found at recorded
  path" — make it a test).
- Assign vs delete of the same file; assign vs in-flight upload into the same target directory.
- TOCTOU between `get_dir_path_for_album` cache resolution and `fs::rename` when the album directory is renamed/deleted
  in between (partly covered by stale-path rejection).
- Sub-album path rewrite racing a concurrent assign of an item inside the moving subtree.

## Security notes

- Request inputs are opaque (`hash`, `album_id`) and resolved server-side — no path traversal from request bodies.
- Unknown `album_id` / missing `hash` → 400 (covered).
- Integrity risk of the old `replace` (hash advertising bytes that no longer match) is **removed with replace itself**;
  `merge`'s dedup adds a verify-before-delete step so it never wrongly deletes a source copy.

## Refactor candidate: share one file-landing helper

The two handlers cannot merge — upload is a multipart batch with sanitize/ preflight/index, assign moves a single
already-indexed record. But the conflict resolution block is duplicated, including a second copy of the unique-name
finder:

- `save_file` (`post_upload.rs`) — tmp path → conflict resolution → rename, own `find_unique_upload_path`.
- `move_item_into_album` (`assign_album.rs`) — alias path → conflict resolution → rename, own `find_unique_path`.
- `OnConflict` enum already shared.

Extract one helper both call, e.g. `place_file(src, dest_dir, filename, conflict) -> Option<PathBuf>` returning `None`
when no file lands (rename: n/a; merge step 1 dedup: source removed, content stays in album); apply `rename`/`merge`
once; single `find_unique_path` (delete `find_unique_upload_path`). Also share the album target-dir resolution
(`get_dir_path_for_album` + the is-a-directory check). Low blast radius; best done together with the conflict-model
implementation. Spin off as its own ticket when scheduled.

## Implementation plan (commits)

Sequence agreed 2026-09-16. Phases 1–3 are the plan; C10 is optional cleanup.
Each commit is small and self-contained. Verification: `just check` after every
commit; `just test` full-green at C1, C2, C5, C8, C9, C10 — the suite is red
between C3 and C5 by the chosen strict tests-first ordering.

### Phase 1 — Tests

- **C1 — disable deprecated scenarios.** Delete 7 YAMLs (build.rs scans the
  dir; removal disables them, git preserves history):
  `assign_conflict_default_skip`, `assign_conflict_skip_z4`,
  `assign_conflict_replace_z6`, `assign_album_dir_conflict_skip_zz8`,
  `assign_album_dir_conflict_replace_rejected_zza`, `upload_conflict_skip`,
  `upload_conflict_replace`. Green.
- **C2 — test-only DB probe.** Gated endpoint returning a record's full
  `alias[]` (404/403 unless `PICASU_TEST_PROBE=1` + debug assertions).
  Reachable from scenarios via `when.call`. Self-test included. Green.
- **C3 — new + updated scenarios.** Add the scenario groups from "Test changes
  under the conflict redesign" (1–11), asserting alias lists via the probe (1,
  2, 5). Update ~17 surviving `assign_*` bodies: add `alias` +
  `onConflict: rename` (extra body fields are ignored by the backend today).
  Red window starts.

### Phase 2 — Backend

- **C4 — API contract + G1.** `OnConflict` → `{Rename, Merge}` (drop Skip /
  Replace and `#[default]`); `AssignAlbumData` gains required `alias` +
  required `on_conflict` (missing/unknown alias → 400, alias validated against
  the record, mirror `delete.rs:157-168`); move rewrites only the selected
  alias entry, stale-check on the selected alias; rename path unchanged;
  utoipa request schema updated. Greens: on_conflict-required, self-move, G1
  sibling.
- **C5 — merge file semantics.** In `move_item_into_album`: record alias under
  `album_dir` (exact dir) → verify `blake3_hasher(selected) == hash` (else
  error, never delete) → `prune_alias_paths(selected)` (file + sidecar, record
  kept) → `set_album` → persist; else rename-move with auto-rename. Greens:
  merge dedup ×3, verify-mismatch, G1∩G2 sibling. Suite green again.
- **C6 — outcome reporting.** New `AssignResult` JSON body
  `{ outcome: moved | renamedFrom | deduplicatedRemoved }` from both assign
  paths + utoipa. Greens outcome-contract.
- **C7 — dir-album recursive merge.** For `Merge`, migrate dir content
  leaf→root with per-file merge semantics, then remove emptied dirs + their
  dir-album records; never remove a non-empty dir nor its ancestors. `Rename`
  dir path stays whole-dir `fs::rename`. Self/subtree guard retained. Greens
  dir-merge scenarios.
- **C8 — upload rename|merge.** `post_upload.rs` strict parse → `rename|merge`;
  `schema.json:181-188` enum + DSL upload verb aligned; upload merge = after
  hash computed in the write+index loop, same-record alias under the album dir
  → delete the just-written file, skip insert (dedup); absent param = `rename`.
  Greens upload-merge, keeps upload_conflict_rename.

### Phase 3 — Frontend (G3)

- **C9 — assignAlbum + modal.** `assignAlbum()` sends `alias`
  (`data.get(index).alias[0].file`) + strategy; AssignAlbumModal gains the
  Merge/Rename radio (Merge default); outcome → toast/chip (moved /
  renamed-from / deduplicated-removed); batch reports per-file. Update
  `assign-photo-to-album.yaml`, add batch scenario. Greens Playwright.

### Phase 4 — Cleanup (optional)

- **C10 — consolidation.** Optional `place_file` helper (spec'd in the
  refactor-candidate section), utoipa/OpenAPI sweep, cross-ref notes in
  `upload-conflict.md` / `delete-from-disk.md`, dead-code sweep.

## Progress

- 2026-09-17: C1 done — deleted the 7 obsolete `skip`/`replace` scenario YAMLs (suite green). C2 done — test-only DB probe
  `GET /get/test/record/<hash>` returning full `alias[]`; gated by a `cfg(test)` static opt-in flag (the `PICASU_TEST_PROBE`
  env idea was impractical under `#![deny(unsafe_code)]` + edition 2024, so opt-in uses a static instead), selftest included.
  C3 done (red window) — updated 20 surviving `assign_*` callers with `onConflict: "rename"` (+ `alias` for item moves, none
  for dir moves) and added 10 new scenarios (merge dedup ×3, verify-mismatch emulated, G1∩G2 sibling, rename-same-hash, ugly
  on_conflict-required, upload-merge, dir-merge-recursive, self-move no-op). Suite: 194 pass / 10 fail, failures exactly the
  new scenarios. Response-outcome scenario deferred to C6 (keeps C5 full-green achievable). Verify-mismatch is emulated (no
  post-index file-write verb exists); may strengthen at C5. Dir-merge-recursive pins leaf→root flattening `dest/{root,leaf}.jpg`.
- 2026-09-16: consistency pass — fixed dangling "Conflict model (redesign)" references (model lives under G2); gap 5
  self-move and gap 7 on\_conflict validation updated for the new model; added "Test changes under the conflict
  redesign" (deprecated/update/new scenario inventory incl. merge dedup, verify-mismatch, G1∩G2 sibling, dir
  recursion, self-move, response contract) and the DSL/alias-observability limitation; two open sub-points recorded
  under G2 (upload on\_conflict optional-default; dedup lookup scoping).
- 2026-09-16: G2 redesign settled — `OnConflict` becomes `rename` | `merge` everywhere; `skip` and `replace` removed
  (a move always completes, never overwrites different content). Merge = dedup selected alias against same-record
  aliases in the target album (verify-before-delete, then remove redundant copy) + move the rest with auto-rename.
  Upload keeps only `rename` | `merge`. Sub-album moves become recursive leaf→root content migration +
  emptied-dir/-record removal, keeping any non-empty dir and its ancestors. `on_conflict` required (no default).
  `docs/design.md` "Moving / Deleting" rewritten to match; obsolete scenarios marked (assign/upload `skip`/`replace`);
  gaps 2–3 resolved, merge-dedup + dir recursion added as gaps 3b/3c. G1 unchanged (selected-alias-only, required
  `alias` field).
- 2026-09-16: G1 resolved — assign operates on the selected alias; move affects only that alias, sibling aliases
  untouched, mirroring the `aliasList` pattern already in `delete_data`. Records the implied required
  `AssignAlbumData.alias` field + selective alias rewrite in `move_item_into_album` (missing/unknown alias → 400; no
  server-side alias picking; pre-v0.1, no backward-compat carve-out). Gap 1 re-framed as the proving coverage.
- 2026-09-16: implementation plan (commits C1–C10) appended, folding in four settled decisions: test-only DB probe as
  its own commit (C2) for alias-list observability (DSL limitation resolved); frontend modal gains a Merge-default
  radio (G3); upload `on_conflict` absent → `rename`; dedup lookup scoped to the exact target album dir. Strict
  tests-first: suite green at C1/C2, red from C3 until the C5 merge semantics land.
- 2026-09-16: full investigation appended — vectors, per-scenario test inventory, Gap 1–11 corner cases, race and
  security notes, G1–G3 open decisions, `place_file` unification candidate. Corrected from a first draft: upload
  conflict scenarios do exist but are same-content-only and skip is weakly asserted. Plan rewritten plain after an
  initial over-compression.
