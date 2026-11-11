---
status: in-progress
type: feature
priority: high
area: backend
---

## Goal

Implement the implementation obligations in
`utils/quint/pipeline/requirements.json` (REQ-2..7, REQ-9..14) so the
index pipeline matches the proved eventual-consistency model. Each
obligation gets a gate test first, then the smallest code change that
makes it pass. Requirement statuses move `open` -> `imposed` when the
gate exists, `verified` when it is green.

REQ-1 (sidecar identity for the hash short-circuit) and REQ-8 (per-row
thumbnail checksum) stay deferred with the skip optimization: without a
skip predicate there is no consumer for either, and full rescans already
reprocess. They return with the implementation preserved on
`backup/hash-short-circuit`.

## Decisions

- Scope: REQ-2..7 and REQ-9..14 on this branch. REQ-1 and REQ-8
  deferred, statuses remain `open` with the conditional wording already
  in the registry.
- Backstop config: `index_backstop_interval_secs` in the `gallery`
  section, default `0` (disabled), env override
  `PICASU_INDEX_BACKSTOP_INTERVAL_SECS`. Per-root scan; a global job is
  accepted because it is stronger than the required per-root
  non-overlap. A tick during a cycle coalesces into one follow-up.
- Scan-vs-mutation coordination (superseded by C9/C10): per-path epochs
  plus short leases replaced the global gate, but the sweep Verify
  fan-out (C2) and per-flush cascade rebuilds (C4) are replaced in turn
  — see C9 (single batched DB sweep) and C10 (per-path try-and-abort
  baseline).
- Scan ownership: one scan scheduler owns every trigger (API, backstop,
  `Flag::Rescan`), dedupes against remaining work, trickle-feeds Rayon.
  Shutdown discards queued work, awaits in-flight jobs only. Abort and
  preemption semantics live in C10.
- Writer side effects (revised by C9/C10): redb writes advance an epoch;
  request-driven views replace per-flush rebuilds. Per-flush cascade
  rebuild claims are withdrawn — see C9/C10.
- Renames: factor notify event classification into a pure function so
  rename handling is testable. File rename submits old-path removal plus
  new-path index; directory rename removes rows under the old prefix and
  indexes the new tree.
- Atomic writes (REQ-13) use one shared
  `tempfile::NamedTempFile::persist` mechanism with an obvious temp name
  (`.{event}-{filename}.{pid}.{seq}.tmp`). A startup sweep removes temp
  files whose owning pid is dead or whose mtime is old. Concurrent
  read-modify-write and fsync policy remain open follow-ups (C6).
- One branch/stacked PR to `spec/quint-index-pipeline`. Commit after
  each reviewed step; push after each phase so CI verifies the chain.

## Steps

Each step is one worker invocation: RED test observed, implementation,
GREEN, review, commit. Test-only steps state why no RED is possible.

### Phase 1 - local correctness and verification

- **S1 [DONE] REQ-4: per-root sweep isolation** (test-only). Scope:
  `backend/src/tasks/actor/album_index.rs` tests. RED: none; sweep is
  already root-scoped. Add a test with a ghost row under an unscanned
  sub-root and scan a sibling root; assert the outside row survives. Run
  before/after; mark `verified`.
- **S2 [DONE] REQ-13: atomic sidecar/albuminfo writes** (test-only
  unless the gate fails). Scope: `process/sidecar_edit.rs`,
  `process/xmp_write.rs`. RED: none; add a failure-injection seam around
  the temp write and assert the target keeps its previous bytes. If the
  helper leaks a partial target, fix it. Mark `verified`.
- **S3 [DONE] REQ-9: orphan `.xmp` detection is log-only**. Scope:
  `album_index.rs`, `init.rs`/startup, logging. RED: pure
  `find_orphan_sidecars(root, known_assets)` returns a sidecar with no
  media; integration scans/restarts, logs it, never deletes it. Wire
  discovery into scan; log only.
- **S4 [DONE] REQ-10: stale upload tmp sweep**. Scope: `init.rs` or scan
  sweep, `router/post/post_upload.rs` naming. RED: a `{name}-{uuid}.tmp`
  under an image root survives startup/scan; after the change it is
  removed and no row is created.
- **S5 [DONE] REQ-11: rotate sidecar before thumbnail and DB**. Scope:
  `router/put/rotate_image.rs`. RED: force the sidecar write to fail;
  today the thumbnail is already rewritten. Reorder to sidecar ->
  thumbnail -> DB and assert thumbnail and DB unchanged on sidecar
  failure.
- **S6 [DONE] REQ-12: assign copies the sidecar before moving the
  asset**. Scope: `router/put/assign_album.rs`. RED: fail the asset move
  after the sidecar copy; assert the destination sidecar exists and
  metadata is not separated. Reorder to copy sidecar, move asset, delete
  old sidecar.

### Phase 2 - coordination before extra scan triggers

- **S7 [SUPERSEDED by C9/C10] REQ-7: scans coordinate with mutating API
  work**. Old mechanism (per-path generations + Verify-lane sweep)
  replaced. Safety rule carries forward; implementation lives in C9/C10.
  Gate superseded by the C9/C10 RED gates. (S1 gate note: REQ-4
  isolation test uses distinct hashes per root and cannot catch
  cross-root hash-reap — fixed under C9 step 5.)

### Phase 3 - scan triggers and cycle semantics

- **S8 [DONE] REQ-5: honor `Flag::Rescan`**. Scope:
  `tasks/batcher/start_watcher.rs`. RED: extract a pure event
  classifier/root selector; a synthetic `EventKind::Other` +
  `Flag::Rescan` resolves to the affected root and submits a rescan.
  Wire the watcher loop to it.
- **S9 [DONE] REQ-6: configurable per-root backstop scan**. Scope:
  `model/config.rs`, `docs/config.md`, `lib.rs`/looper. RED: config
  parses/validates/defaults the interval; with a 50 ms interval and the
  watcher disabled, a missed change converges within one cycle. Default
  `0` starts no loop.
- **S10 [DONE] REQ-14: non-overlapping, drain-complete backstop
  cycles**. Scope: backstop scheduler. RED: a scheduler state-machine
  test: tick during a running cycle marks one pending follow-up, never
  overlaps, and completes when the scan and intake drain return.

### Phase 4 - watcher renames

- **S11 [DONE] REQ-2: file rename submits remove plus create**. Scope:
  `start_watcher.rs` + batching. RED: real-FS rename of a watched file;
  the row disappears at the old path and appears at the new one without
  a sweep.
- **S12 [DONE] REQ-3: directory rename follows descendants**. Scope:
  `start_watcher.rs`, album index/asset store prefix query. RED: real-FS
  rename of a watched directory; every descendant row moves to the new
  path with assetId preserved and no ghosts.

### Deferred

- **REQ-1** sidecar identity for the hash short-circuit: returns with
  the optimization; gate stays recorded in
  `utils/quint/pipeline/requirements.json`.
- **REQ-8** per-row thumbnail checksum: needs a skip consumer; rescans
  already regenerate derived artifacts. Returns with REQ-1.

REQ-1 and REQ-8 remain `open` in
`utils/quint/pipeline/requirements.json`; every other obligation is
`verified` by a gate test.

## Course correction (review 2026-11-11)

First pass introduced process-wide coordination and unbounded writer
side effects. C9/C10 below are the authoritative direction; C2/C4 are
superseded pointers only.

### Easy items (done, kept)

- **C6 [done] REQ-13: shared named-temp atomic write**. Staged through
  `NamedTempFile::persist` as `.{event}-{filename}.{pid}.{seq}.tmp`;
  startup sweep by owner pid/mtime. Follow-ups: read-modify-write
  last-writer-wins; no fsync policy.
- **C7 [done] REQ-9: orphan sidecar memory**. Collect only `.xmp` paths;
  sibling check on demand. Log-only unchanged.
- **C8 [done] REQ-10: upload temp naming**.
  `.upload-{filename}.{pid}.{seq}.tmp`, pid/mtime sweep. Upload semantic
  rewrite waits for scheduler ownership.

### Structural work (authoritative: C9/C10; rest are pointers)

- **C1 [done, residual drain cost see C9/C10]** Shutdown lifecycle. Owns
  backstop/expire handles; stops watcher, rejects intake, cancels/awaits
  scan, discards queued work, drains writer. Original 70 s hang fixed.
  Residual: drain still covers per-flush cascade rebuilds (see C9/C10);
  gate determinism under TEST-01.
- **C2 [superseded by C9/C10]** Path-scoped coordination via Verify-lane
  sweep. Do not patch; implement C9/C10.
- **C3 [partial, abort/preemption moves to C10]** Scheduler ownership
  done; in-flight abort + lock-map hygiene open in C10.
- **C4 [superseded by C9/C10]** Lazy tree view as claimed. Epoch advance
  kept; per-flush rebuild removal withdrawn (see C9/C10 gates).
- **C5 [open] Watcher rename robustness**. Split `From`/`To` dir
  renames, `new.is_dir()` TOCTOU, epoch (not gate) for removals.

### Earlier review notes (kept, condensed)

- Kept under C10: subtree leases + generation bumps on album move;
  serialized writer seq/watermarks/barriers with post-final rejection;
  thumbnail deletion with confirmed removal; failed rebuild keeps
  previous view with epoch-based invalidation.
- Superseded by C9 (do not extend): sweep Verify-lane generation
  capture/release + leased-path skip — survives only as C9 inline
  re-check / C10 rule.
- Open optimisation: full redb scan+sort on requesting thread;
  query-scoped view follow-up.

### Remaining claims (not absorbed by C9/C10)

- SHUTDOWN-02 (S3): dead cascade indistinguishable from stall.
- CACHE-01 (S3): same-ms `fetch_max` prefetch staleness.
- SHUTDOWN-03 (S3-low): watcher can start after `stop_watcher`.
- M3 (S3): REQ-3 "in one step" is per-record `warn!`, not one txn.
- TEST-01 (hardening): SIGTERM gate fires before backlog depth.
- Absorbed into C9/C10 RED steps (no separate action): EPOCH-01,
  SWEEP-01, CASCADE-01, M1, M2, M4.

## Revised designs (2026-11-12, open)

Supersede the C2 sweep fan-out while keeping its REQ-7 safety rule
(per-path in-flight detection, no global lock).

- **C9 [open] DB sweep as a single batched job (replaces sweep Verify
  fan-out)**. Requirements: REQ-4 (per-root isolation stays green, gate
  must share one hash across roots), REQ-7 (no prune while the path's
  own mutation is in flight). Disposes SWEEP-01/M1 as structure, not
  patches. Intended solution: one crawl feeds both lanes. `walk(root)`
  builds a transient `fs_set`; prefix-scan `ASSET_BY_PATH(root)` gives
  `db_paths`; `sweep_set = db_paths - fs_set` is handed to a cancelable
  sweep job and both sets are dropped; `fs_set` streams into intake with
  backpressure. For large libraries scope by subdir:
  walk/diff/submit/drop per child prefix (deterministic order, cancel
  point per subdir). The sweep job re-checks each candidate inline
  (`generation` unchanged, `!is_busy`, `!Path::exists()`) just before
  batching; deletes go to the serial writer in small batches (e.g. 200
  paths/txn) by exact `(path, id)`, never by hash-group reap (or reap
  scoped to the batch root). No `snapshot_tree` clone in the sweep path;
  no `VerifyFile` hop for sweep candidates. Steps: (1) RED: same-hash
  ghosts under sibling roots — sweeping A must keep B;
  rename-then-flush-empty-path must not delete the moved row. (2) Add
  `ASSET_BY_PATH` prefix scan helper; add `SweepRemoveBatch` writer op
  (short txn per batch, delete by path/id, thumbnail delete stays with
  confirmed removal). (3) Rewrite `sweep_stale_asset_paths` into the
  batched job with per-chunk cancel/shutdown checks and inline
  re-checks; wire per-subdir scoping. (4) Remove sweep
  `submit_verify_file` calls (keep the lane only if watcher-removal
  needs it); delete path-prune-before-submit on the sweep path. (5) Fix
  the REQ-4 gate to share hashes; GREEN plus `backstop`, `shutdown`,
  rotation/assign gates.
- **C10 [open] Per-path concurrency: interactive preempts scan (baseline model)**.
  Requirements: REQ-7 with the rule "scan never waits, interactive never
  loses". Scan-first-then-modify is safe (modify applies on top through
  the writer); modify-first-then-scan never runs stale (scan aborts).
  Intended solution: key on normalized path for all producers (new files
  have no ID until flush). Existing assets resolve `assetID ->
ASSET_BY_ID.path -> normalize` at lock acquisition, then validate.
  Single lock per job; directory moves and batch deletes take one subtree
  lock for the prefix instead of N per-file locks. Scan side is
  try-and-abort (`try_lock` fails -> drop result, count barrier complete;
  generation bump mid-decode -> drop at next stage boundary, never flush
  stale). Interactive side bumps the generation (cancel signal), aborts
  the in-flight decode for that path if tracked, then locks, mutates
  (FS + sidecar + row), unlocks; no re-queue needed since the mutation
  already wrote the row. Sidecar-mutating ops (metadata edit, rotate)
  are never relaxed: a concurrent scan reading torn image+sidecar state
  would flush over the edit, so cancel-and-retry is required, not
  best-effort overwrite. Pure read-only album stat recomputation may run
  without the path lock once M2 moves the rebuild out of the write txn.
  Steps: (1) RED: `lease_paths([p,p]); drop; assert!(!is_busy(p))`
  (EPOCH-01, done C10-1); scan-decoding path + concurrent metadata edit
  keeps the edit (scan result dropped, edit wins). (2) Fix `lease_paths`
  dedup 1:1 (done C10-1) plus lock-map cleanup (`remove_if` on zero,
  covering M4). (3) Add per-path abort: generation check at decode stage
  boundaries and at flush submit; interactive paths bump-then-lock
  (delete/assign/rotate/edit/watcher-remove). (4) Resolve ID->path inside
  the lock section for existing assets; moves lock sorted old+new or
  subtree. (5) GREEN: path_epoch units, scan/edit race gate, existing
  assign/rotate/delete gates.

## Verification per step

- RED transcript before implementation; GREEN plus neighbouring tests
  after.
- `just backend-test` for each backend step (serial, no concurrent
  runs).
- `just check` package before push; `spec-check`, `catalog-check`,
  `events-check`, `plan-lint`, `gates-check`, prettier on doc changes.
- Update `utils/quint/pipeline/requirements.json` status and regenerate
  `docs/eventual-consistency.md` when the status changes.
- Push after each phase; open one stacked draft PR to
  `spec/quint-index-pipeline` and read CI before starting the next
  phase.
