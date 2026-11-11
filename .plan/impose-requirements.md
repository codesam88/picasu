---
status: in-progress
type: feature
priority: high
area: backend
---

## Goal

Implement the implementation obligations in `utils/quint/pipeline/requirements.json`
(REQ-2..7, REQ-9..14) so the index pipeline matches the proved
eventual-consistency model. Each obligation gets a gate test first, then
the smallest code change that makes it pass. Requirement statuses move
`open` -> `imposed` when the gate exists, `verified` when it is green.

REQ-1 (sidecar identity for the hash short-circuit) and REQ-8 (per-row
thumbnail checksum) stay deferred with the skip optimization: without a
skip predicate there is no consumer for either, and full rescans already
reprocess. They return with the implementation preserved on
`backup/hash-short-circuit`.

## Decisions

- Scope: REQ-2..7 and REQ-9..14 on this branch. REQ-1 and REQ-8 deferred,
  statuses remain `open` with the conditional wording already in the
  registry.
- Backstop config: `index_backstop_interval_secs` in the `gallery`
  section, default `0` (disabled), env override
  `PICASU_INDEX_BACKSTOP_INTERVAL_SECS`. Per-root scan; a global job is
  accepted because it is stronger than the required per-root non-overlap.
  A tick during a cycle coalesces into one follow-up.
- Scan-vs-mutation coordination: per-path epochs plus short leases. A
  sweep enqueues per-file `Verify` work; a verify prunes only when the path
  epoch still matches, no lease is held, and the file is still absent.
- Scan ownership: one scan scheduler owns every trigger (API, backstop,
  `Flag::Rescan`). It keeps the remaining work queue, deduplicates a new
  request against entries already queued, and trickle-feeds the Rayon
  pool. Shutdown stops the watcher, discards the queue, and lets only
  in-flight jobs finish (see C3).
- Writer side effects: redb writes increment an epoch. Global in-memory
  `TREE` views are not rebuilt on every flush; a client requests a scoped
  view when it needs one (see C4).
- Renames: factor notify event classification into a pure function so
  rename handling is testable. File rename submits old-path removal plus
  new-path index; directory rename removes rows under the old prefix and
  indexes the new tree.
- Atomic writes (REQ-13) use one shared `tempfile::NamedTempFile::persist`
  mechanism with an obvious temp name
  (`.{event}-{filename}.{pid}.{seq}.tmp`). A startup sweep removes temp
  files whose owning pid is dead or whose mtime is old. Concurrent
  read-modify-write and fsync policy remain open follow-ups (C6).
- One branch/stacked PR to `spec/quint-index-pipeline`. Commit after each
  reviewed step; push after each phase so CI verifies the chain.

## Steps

Each step is one worker invocation: RED test observed, implementation,
GREEN, review, commit. Test-only steps state why no RED is possible.

### Phase 1 - local correctness and verification

- **S1 [DONE] REQ-4: per-root sweep isolation** (test-only).
  Scope: `backend/src/tasks/actor/album_index.rs` tests.
  RED: none; sweep is already root-scoped. Add a test with a ghost row
  under an unscanned sub-root and scan a sibling root; assert the outside
  row survives. Run before/after; mark `verified`.
- **S2 [DONE] REQ-13: atomic sidecar/albuminfo writes** (test-only unless the
  gate fails).
  Scope: `process/sidecar_edit.rs`, `process/xmp_write.rs`.
  RED: none; add a failure-injection seam around the temp write and
  assert the target keeps its previous bytes. If the helper leaks a
  partial target, fix it. Mark `verified`.
- **S3 [DONE] REQ-9: orphan `.xmp` detection is log-only**.
  Scope: `album_index.rs`, `init.rs`/startup, logging.
  RED: pure `find_orphan_sidecars(root, known_assets)` returns a sidecar
  with no media; integration scans/restarts, logs it, never deletes it.
  Wire discovery into scan; log only.
- **S4 [DONE] REQ-10: stale upload tmp sweep**.
  Scope: `init.rs` or scan sweep, `router/post/post_upload.rs` naming.
  RED: a `{name}-{uuid}.tmp` under an image root survives startup/scan;
  after the change it is removed and no row is created.
- **S5 [DONE] REQ-11: rotate sidecar before thumbnail and DB**.
  Scope: `router/put/rotate_image.rs`.
  RED: force the sidecar write to fail; today the thumbnail is already
  rewritten. Reorder to sidecar -> thumbnail -> DB and assert thumbnail
  and DB unchanged on sidecar failure.
- **S6 [DONE] REQ-12: assign copies the sidecar before moving the asset**.
  Scope: `router/put/assign_album.rs`.
  RED: fail the asset move after the sidecar copy; assert the destination
  sidecar exists and metadata is not separated. Reorder to copy sidecar,
  move asset, delete old sidecar.

### Phase 2 - coordination before extra scan triggers

- **S7 [DONE] REQ-7: scans coordinate with mutating API work**.
  Scope: `tasks/path_epoch.rs`, `album_index.rs` sweep, `assign_album.rs`,
  `delete.rs`, directory-move path.
  RED: a scan sweep plus an overlapping assign must keep the moved row and
  its sidecar. The final mechanism is per-path generations and short
  leases, with the sweep enqueueing Verify-lane work; the earlier global
  gate was replaced in the course correction (C2).

### Phase 3 - scan triggers and cycle semantics

- **S8 [DONE] REQ-5: honor `Flag::Rescan`**.
  Scope: `tasks/batcher/start_watcher.rs`.
  RED: extract a pure event classifier/root selector; a synthetic
  `EventKind::Other` + `Flag::Rescan` resolves to the affected root and
  submits a rescan. Wire the watcher loop to it.
- **S9 [DONE] REQ-6: configurable per-root backstop scan**.
  Scope: `model/config.rs`, `docs/config.md`, `lib.rs`/looper.
  RED: config parses/validates/defaults the interval; with a 50 ms
  interval and the watcher disabled, a missed change converges within one
  cycle. Default `0` starts no loop.
- **S10 [DONE] REQ-14: non-overlapping, drain-complete backstop cycles**.
  Scope: backstop scheduler.
  RED: a scheduler state-machine test: tick during a running cycle marks
  one pending follow-up, never overlaps, and completes when the scan and
  intake drain return.

### Phase 4 - watcher renames

- **S11 [DONE] REQ-2: file rename submits remove plus create**.
  Scope: `start_watcher.rs` + batching.
  RED: real-FS rename of a watched file; the row disappears at the old
  path and appears at the new one without a sweep.
- **S12 [DONE] REQ-3: directory rename follows descendants**.
  Scope: `start_watcher.rs`, album index/asset store prefix query.
  RED: real-FS rename of a watched directory; every descendant row moves
  to the new path with assetId preserved and no ghosts.

### Deferred

- **REQ-1** sidecar identity for the hash short-circuit: returns with the
  optimization; gate stays recorded in `utils/quint/pipeline/requirements.json`.
- **REQ-8** per-row thumbnail checksum: needs a skip consumer; rescans
  already regenerate derived artifacts. Returns with REQ-1.

REQ-1 and REQ-8 remain `open` in `utils/quint/pipeline/requirements.json`; every other
obligation is `verified` by a gate test.

## Course correction (review 2026-11-11)

The first pass satisfied the clauses individually but introduced
process-wide coordination and unbounded writer side effects. The branch
must not merge until C1 is green. Easy items land first; C1..C5 are
bounded follow-up steps.

### Easy items (land first)

- **C6 [done] REQ-13: shared named-temp atomic write**.
  Problem: `write_via_temp` uses a deterministic `.{filename}.tmp`, so
  two writers to one sidecar collide, and the name carries no owner.
  Fix: move `tempfile` to runtime dependencies; stage through
  `NamedTempFile::persist` using `.{event}-{filename}.{pid}.{seq}.tmp`.
  Add a startup sweep for our temp names whose pid is dead or whose
  mtime is old. Open follow-ups: concurrent read-modify-write on a
  sidecar is still last-writer-wins; no fsync policy is defined.
- **C7 [done] REQ-9: orphan sidecar detection memory**.
  Problem: the scan collects every media stem in a second `Vec` for the
  whole walk, O(files) memory on a millions-image library.
  Fix: collect only `.xmp` paths; check sibling media on demand per
  sidecar. Log-only semantics unchanged.
- **C8 [done] REQ-10: upload temp naming and sweep**.
  Problem: `{filename}-{uuid}.tmp` is valid-looking and invisible to a
  simple lifecycle rule.
  Fix: stage uploads as `.upload-{filename}.{pid}.{seq}.tmp` and detect
  residue by owner pid/mtime. Upload semantic rewrite (type/name checks,
  then save; schedule a scan only when no watcher is active; progress
  notification) waits for C3 because it needs scheduler-owned intake.

### Structural work

- **C1 [done] Shutdown lifecycle (REQ-6/REQ-14)**.
  Reproduced: SIGINT makes Rocket unreachable but the process stays
  alive. With `index_backstop_interval_secs=2` and a 500-image scan,
  exit took 70 s; 100 images exited promptly. GDB shows `shutdown_writer`
  waiting on the cascade barrier while the cascade task runs a full
  `refresh_tree_cache`; the backstop handle is dropped and the active
  index job is neither cancelled nor awaited.
  Fix: `tasks/lifecycle.rs` owns the backstop and expire handles; shutdown
  stops the watcher, rejects new intake, cancels and awaits the scan job,
  discards queued work, and only then drains the writer.
  Gate: process-level SIGTERM test with a backstop and a library large
  enough to produce several flush batches; assert exit within seconds.
- **C2 [done] REQ-7: path-scoped coordination**.
  Serializing unrelated albums and holding API requests behind a whole
  sweep is over-strong. Implement path epochs/leases and a `Verify` lane;
  directory moves lease a subtree epoch.
- **C3 [done] Scan scheduler ownership**.
  Move scan triggers into one scheduler that owns the queue, dedupes
  against remaining work, and trickle-feeds Rayon. The backstop awaits
  its cycle instead of polling global status. Shutdown discards queued
  work and awaits in-flight jobs only.
- **C4 [done] Writer epoch / lazy tree view**.
  Writers advance an epoch on redb writes; `snapshot_tree` rebuilds lazily
  when the epoch moved. A scan no longer triggers a full rebuild per
  flush, which removes the O(batches × assets) cost and the shutdown
  backlog. A query-scoped view remains an open optimisation.
- **C5 [open] Watcher rename robustness**.
  Handle split `From`/`To` directory renames and replace the
  `new.is_dir()` TOCTOU with explicit pairing. Watcher removals use the
  path epoch, not the gate.

### Review dispositions (2026-11-11)

Two independent review passes (coverage + adversarial) found:

- **Fixed**: API album move now leases source/destination subtrees and
  bumps their generations; dir-album verifies re-resolve the album id.
- **Fixed**: writer sequence allocation and send are serialized,
  watermarks use `fetch_max`, barriers wait for their own marker, and
  submissions after the final barrier are rejected.
- **Fixed**: thumbnail deletion moved into the confirmed writer removal;
  the sweep no longer deletes renditions before the verify checks.
- **Fixed**: a failed tree-view rebuild keeps the previous view; cache
  invalidation advances the epoch instead of clearing the built marker.
- **Fixed**: verify items capture the current generation instead of
  bumping it; skipped items release their generation; scan/watcher
  indexing skips a leased path; delete holds leases across the row
  flush.
- **Open optimisation**: a lazy rebuild still runs a full redb scan and
  sort on the requesting thread; a query-scoped view is the follow-up.

## Verification per step

- RED transcript before implementation; GREEN plus neighbouring tests
  after.
- `just backend-test` for each backend step (serial, no concurrent runs).
- `just check` package before push; `spec-check`, `catalog-check`,
  `events-check`, `plan-lint`, `gates-check`, prettier on doc changes.
- Update `utils/quint/pipeline/requirements.json` status and regenerate
  `docs/eventual-consistency.md` when the status changes.
- Push after each phase; open one stacked draft PR to
  `spec/quint-index-pipeline` and read CI before starting the next phase.
