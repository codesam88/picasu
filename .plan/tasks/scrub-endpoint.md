---
status: backlog
type: feature
priority: low
area: backend
---

## Scrub / self-check endpoint

Supersedes `fs-consistency-checks.md` (vague placeholder → this is the concrete design).
The `reindex` endpoint was removed in PR #6. This task adds a proper healing path:
detect DB records whose derived fields have drifted from what fresh extraction would
produce (due to partial write, extraction bug, FS corruption, or interrupted indexing),
report mismatches, and — after user review — optionally repair them.

---

## Context: what the code actually does today

> Derived from explore-agent deep-reads of the codebase (2026-06-29).

### Single entry point for all indexing

`crate::workflow::index_image(src: &Path, dst: Option<&Path>)` in
`backend/src/workflow/mod.rs` is called by **everything**: album_index,
the filesystem watcher, `POST /post/index/image`, and post_upload.
There is no separate `index_for_watch` function.

### Task chain for a new file

```
OpenFileTask → HashTask → DeduplicateTask → IndexTask → [VideoTask]
                                  ↓ (None: known hash)
                           FlushTreeTask (detached)
                                  ↓
                           UpdateTreeTask → UpdateExpireTask → ExpireCheckTask
```

**`DeduplicateTask` short-circuit** (`backend/src/tasks/actor/deduplicate.rs:46`):

```rust
if let Some(guard) = data_table.get(&*task.hash)? {
    // Hash already in DATA_TABLE → merge aliases, optionally set album,
    // flush existing record via detached FlushTreeTask, return Ok(None).
}
// None → Ok(Some(abstract_data)) → IndexTask runs
```

The only condition for skipping extraction is the content hash already existing
in `DATA_TABLE`. Returning `Ok(None)` stops the pipeline; `IndexTask`/`VideoTask`
never run.

### Per-hash concurrency guard

`workflow/mod.rs:17` — `static IN_PROGRESS: LazyLock<DashSet<ArrayString<64>>>`.
`ProcessingGuard` (RAII, lines 19–32): `try_acquire(hash)` inserts hash into the
set; returns `None` (bail early) if already present; drops remove it.
Acquired **after** `OpenFileTask + HashTask`, so two pipelines for the same path
but different content (file modified mid-index) are NOT serialized — they race to
hash, then the second unique hash gets its own guard.

### Watcher

`backend/src/tasks/batcher/start_watcher.rs`: `notify` crate, recursive watch.
1-second **per-path** debounce (`DEBOUNCE_POOL: Mutex<HashMap<PathBuf, Instant>>`).
After debounce fires → `index_image(relative, None)`. No per-path processing lock.
Multiple paths run fully concurrently.

### Persistence

`FlushTreeTask` (`batcher/flush_tree.rs`): redb write transaction on `DATA_TABLE`
(`TableDefinition<&str, AbstractData>` keyed by hash). Writes the **full**
`AbstractData` record — **last write wins, no field-level merging**. Marks
albums for update; dispatches `UpdateTreeTask` (detached).

`UpdateTreeTask` (`batcher/update_tree.rs`): reads all of `DATA_TABLE` via rayon,
rebuilds `TREE.in_memory` (the sorted in-memory query/sort cache). Pure
in-memory rebuild, no additional disk writes.

### AbstractData field provenance

| Category                                      | Fields                                                                                                                                           |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Content-derived** (scrub re-extracts these) | `width`, `height`, `size`, `exif_vec`, `thumbhash`, `phash` (image only), on-disk thumbnail at `compressed_path()`                               |
| **User-set** (scrub must NOT overwrite)       | `description`, `is_favorite`, `is_archived`, `is_trashed`, `album`, `update_at`                                                                  |
| **Mixed / ambiguous**                         | `tags` — XMP keyword discovery merges into the same `HashSet<String>` as user-added tags; cannot be cleanly separated without storing provenance |
| **Filesystem-derived**                        | `alias: Vec<FileModify>` (paths + mtimes); pruning dead aliases is a separate concern                                                            |

**Video note**: `process_video_info` hashes the generated thumbnail JPEG, not the
raw video file. So scrubbing a video involves re-running ffprobe + ffmpeg.

### Job-slot locking

`album_index.rs`: `INDEX_STATUS: Mutex<IndexStatusSlot>` + `ACTIVE_INDEX: Mutex<Option<ActiveIndex>>`.
Only one album-index job at a time (returns `Conflict` if another is `Running`).
Independent from any scrub job slot — the two can run concurrently on different
sets of files. Per-hash serialization is handled by the shared `IN_PROGRESS` DashSet.

---

## Design

### 1. Per-file primitives (two separate workflow functions)

Keep `workflow::index_image` untouched — it is the discovery path and already
works correctly.

Add `workflow::scrub_file(hash: &ArrayString<64>) -> ScrubResult` as the scrub
primitive:

```
1. Load existing AbstractData from DATA_TABLE (if missing: ScrubResult::RecordGone)
2. Find a live alias path (first alias where File::exists(); if none: ScrubResult::NoAccessibleAlias)
3. try_acquire(hash) — if IN_PROGRESS: return ScrubResult::Deferred (watcher or
   album_index is processing this hash; caller retries or skips)
4. Re-run process_image_info / process_video_info on a clone of the existing record
   (so user fields ride along without needing a later merge)
5. After extraction: re-read the file's mtime via fs::metadata. If mtime changed
   since step 2, mark invalidated = true (file was written during scrub)
6. Compare derived fields between fresh-extracted clone and original stored record
7. Return ScrubResult::Ok { mismatches, invalidated } or ScrubResult::ExtractionError(e)
```

The `IN_PROGRESS` guard acquired in step 3 causes any concurrent watcher `index_image`
call for the same hash to bail ("Processing already in progress"). The watcher will
fire again on the next FS event for that path, so no data is lost — the watcher
event is simply deferred until the scrub guard releases.

If a file is modified to different content during scrub (new hash), the watcher
creates a new DB record for the new hash. The scrub continues against the old hash
record, may find mismatches or error, and marks the record `invalidated` if mtime
changed. This is correct behaviour: the old record may still be valid if the old
file exists under another alias.

### 2. Mismatch fields

```rust
enum FieldDiff {
    WidthDiffers   { stored: u32, extracted: u32 },
    HeightDiffers  { stored: u32, extracted: u32 },
    SizeDiffers    { stored: u64, actual: u64 },
    ThumbhashMissing,
    ThumbhashDiffers,
    PhashMissing,                      // image only
    PhashDiffers,                      // image only
    ExifKeyAdded   { key: String, extracted: String },   // in extracted, missing from stored
    ExifKeyRemoved { key: String, stored: String },      // in stored, missing from extracted
    ExifValueDiffers { key: String, stored: String, extracted: String },
    ThumbnailFileMissing,              // compressed_path() not on disk
}

struct ScrubMismatch {
    hash: ArrayString<64>,
    alias: PathBuf,             // which alias was used for extraction
    fields: Vec<FieldDiff>,
    invalidated: bool,          // mtime changed during scrub
}
```

**Tags**: excluded from comparison in v1. The mixed user/XMP provenance makes
automated comparison misleading. Noted as future work (requires tag provenance
tracking per `xmp-sidecar-metadata` task).

### 3. Repair mode

A second pass: `POST /post/scrub` with `mode: Repair` and `scope: Hashes([...])`.
For each hash, `scrub_file_repair`:

1. Same steps 1–4 as `scrub_file`.
2. Read the current stored record **again** under a DATA_TABLE read (to capture any
   user-field changes since the scrub report was generated).
3. Update only content-derived fields on the freshly re-read record.
4. Write via `FlushTreeTask` (waiting, not detached, so the caller knows it's durable).
5. Trigger `UpdateTreeTask` (waiting).

The two-step read-before-write (step 2) guards against clobbering user edits made
between the scrub report and the repair action.

### 4. Scrub job infrastructure

Mirror album_index pattern in `backend/src/tasks/actor/scrub.rs`:

```rust
static SCRUB_STATUS: LazyLock<Mutex<ScrubStatusSlot>>  // own slot, independent of INDEX_STATUS
static ACTIVE_SCRUB: LazyLock<Mutex<Option<ActiveScrub>>>
```

`ScrubState`: `Idle`, `Running { progress: ScrubProgress }`, `Completed { report: ScrubReport }`,
`Canceled`, `Failed`.

`ScrubProgress`: `{ checked: u64, errors: u64, mismatches: u64, deferred: u64 }`.

`ScrubReport`: `{ checked, mismatches: Vec<ScrubMismatch>, errors: Vec<ScrubError>, deferred: Vec<ArrayString<64>> }`.

The scrub job walks its scope, dispatching `tokio::spawn(scrub_file(hash))` per record
with collected handles, mirrors album_index. No additional global lock — per-hash
serialization via the shared `IN_PROGRESS` DashSet is sufficient. Scrub and
album_index can run concurrently on disjoint hash sets.

### 5. Scope

```rust
enum ScrubScope {
    Path(String),          // all records with any alias under IMAGE_HOME/path
    Hashes(Vec<String>),   // specific hashes — supports single-image right-click
    All,                   // every record in DATA_TABLE
}
```

`Path` and `All` resolve to a hash list by scanning `DATA_TABLE` at job start (a
snapshot; new records added during scrub are simply not in scope).

### 6. HTTP endpoints

```
POST /post/scrub          body: { scope: ScrubScope, mode: "check" | "repair" }
                          → 202 Accepted; starts scrub job or returns Conflict
POST /post/scrub/cancel   → 200
GET  /get/scrub/status    → ScrubStatusResponse { state, progress, report? }
```

`album_index` keeps its own endpoints unchanged. Both job slots are fully independent.

Access: both guarded by `GuardAuth`. In future, a finer-grained `GuardAdmin` can
be split off here without changing album_index.

### 7. Frontend

- Settings panel: "Scrub" button → `scope: All, mode: check` → progress bar.
  On `Completed`: show mismatch count; if > 0, open review panel with per-record
  details and "Repair selected / Repair all" actions → re-POST with `mode: repair`.
- Image right-click menu: "Check this file" → `scope: Hashes([hash]), mode: check`.
  On complete: show inline result. "Repair" → `mode: repair`.

### 8. What needs changing in existing code

- `workflow/mod.rs`: make `try_acquire`/`ProcessingGuard`/`IN_PROGRESS` `pub(crate)`
  so `scrub_file` (in the same crate) can reuse them. Add `scrub_file` function.
- `tasks/actor/deduplicate.rs`: no changes needed — scrub bypasses it entirely.
- `process/index.rs`: no changes — scrub calls `process_image_info`/`process_video_info`
  directly.
- No changes to album_index, watcher, or FlushTreeTask.

---

## Filesystem concurrency: full risk matrix

| Event during scrub                                | Detection                                                                   | Outcome                                                                                   |
| ------------------------------------------------- | --------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| File deleted                                      | `process_image_info` errors at decode/open                                  | `ScrubError`; not a mismatch; old DB record may be stale alias — separate cleanup concern |
| File modified (same bytes)                        | Hash same → `IN_PROGRESS` serializes → if scrub holds guard, watcher defers | Clean: scrub completes, watcher re-fires after guard drops                                |
| File modified (different bytes)                   | New hash → watcher creates new record; scrub reads old bytes or errors      | `invalidated = true` if mtime changed; mismatches (if any) discarded                      |
| File modified mid-read (partial write)            | Decode fails                                                                | `ScrubError`; `invalidated = true`                                                        |
| Two scrub jobs                                    | Blocked by `SCRUB_STATE` slot                                               | Second call returns `Conflict`                                                            |
| Scrub + album_index on same hash                  | `IN_PROGRESS` serializes; second caller defers                              | Safe: one pipeline runs to completion                                                     |
| Scrub + album_index on different hashes           | No interaction                                                              | Both run concurrently, no issue                                                           |
| Scrub scope walk + new files added to DB mid-walk | Scope snapshot taken at job start                                           | New records simply not scrubbed; consistent                                               |

---

## Open questions / deferred

- **Tag provenance**: to include tags in scrub comparisons, add a `tag_source: ContentDiscovered | UserSet` per-tag field. See `xmp-sidecar-metadata` task.
- **Alias scrub**: verify each alias path still exists; prune dead aliases. Could be a separate `ScrubScope::Aliases` mode or a pre-step in the main scrub.
- **Concurrency limit**: album_index currently spawns unlimited `tokio::spawn` per file. Scrub should do the same for consistency, but a `Semaphore` to cap parallelism (e.g. 8 concurrent extractions) would improve responsiveness under load. Noted for both.
- **Durability of scrub report**: report lives in `SCRUB_STATE` in-memory; lost on server restart. For v1 this is acceptable. If durability is needed, serialize to a redb table or JSON file.

---

## Implementation checklist

- [ ] Make `ProcessingGuard`/`try_acquire`/`IN_PROGRESS` `pub(crate)` in `workflow/mod.rs`
- [ ] Add `workflow::scrub_file(hash) -> ScrubResult` and `workflow::scrub_file_repair(hash) -> Result<()>`
- [ ] Define `ScrubMismatch`, `ScrubError`, `ScrubReport`, `ScrubProgress`, `ScrubScope` in `backend/src/model/`
- [ ] Implement `backend/src/tasks/actor/scrub.rs` with `ScrubState`, status slot, `start_scrub`, `cancel_scrub`, `scrub_status`
- [ ] Add HTTP handlers `backend/src/router/post/scrub.rs` and `backend/src/router/get/get_scrub.rs`
- [ ] Register routes in `router/mod.rs`
- [ ] Add `openapi.rs` annotations
- [ ] Frontend: scrub menu item + progress poll + mismatch review panel + right-click action
- [ ] Tests: scenario covering scrub detecting a manually corrupted/missing thumbnail; scenario where a watcher event fires for the same hash mid-scrub (marked deferred); repair scenario
- [ ] Update `fs-consistency-checks.md` to `done`

---

## Notes

2026-06-29: Initial draft after explore-agent deep-read of workflow/mod.rs,
tasks/actor/{album_index,deduplicate,index}.rs, tasks/batcher/{flush_tree,update_tree}.rs,
process/index.rs, model/abstract_data.rs, start_watcher.rs. Key findings:

- `index_for_watch` does not exist; everything calls `workflow::index_image`
- `IN_PROGRESS` DashSet in workflow/mod.rs is the only per-hash lock; scrub reuses it
- `FlushTreeTask` is last-write-wins on the full record; repair must read-before-write
- Tags are mixed user/XMP with no provenance field; excluded from v1 comparison
- Video scrub hashes the generated thumbnail, not the raw video
- `DeduplicateTask` can be bypassed entirely; scrub doesn't need it at all

2026-06-29: Confirmed reindex fully removed in PR #6. No remaining reindex
infrastructure to clean up.
