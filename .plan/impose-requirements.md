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
- **C5 [done] Watcher rename robustness**. Pending-`From` table pairs
  tracker-cookie split events for prefix rewrite; unpaired halves
  handled independently from DB state (subtree removals for departed
  dirs, dir walk for arrivals); expired entries drain to removals.
  Pairing is best-effort by design. Gates: split/expired/unpaired
  synthetic events, real-FS rename gates green.

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
- CASCADE-01/M2 (S2, open as C11): per-flush cascade rebuilds and the
  held-write-txn in `album_task` are untouched by C9/C10; see C11.

- **C11 [done] Writer cascade: rebuild outside the held write txn**.
  `cascade_after_write` snapshots once per entry and shares it (`Arc`)
  with pending album updates; `album_task` reads (short read txn),
  computes (rayon, no txn), writes (short write txn: durable reads +
  insert/member-clear + commit + epoch bump). No `snapshot_tree`/rayon
  inside any write txn on any path; disagreeing read/write states
  re-mark pending for the next cascade. No eager refresh added; cache
  stays lazy. Gates: `self_update_with` stats unit,
  `cascade_two_pending_albums_rebuilds_at_most_once` (observer
  `TreeRefreshBuild` count).
- Absorbed into C9/C10 RED steps (no separate action): EPOCH-01,
  SWEEP-01, M1, M4. CASCADE-01/M2 were listed here but the C9/C10
  implementation did not touch the cascade — see C11.

## Revised designs (2026-11-12, open)

Supersede the C2 sweep fan-out while keeping its REQ-7 safety rule
(per-path in-flight detection, no global lock).

- **C9 [done] DB sweep as a single batched job (replaces sweep Verify fan-out)**.
  File sweep sources candidates from `ASSET_BY_PATH` (no tree-view
  clone), chunks ~200/root with per-chunk cancel/shutdown checks, and
  re-checks generation/lease/filesystem inline before submitting exact
  `(path, id)` `SweepRemoveBatch` writer ops; dir-album phase does inline
  re-checks (incl. album-id re-resolution) into the pre-existing
  `SweepDirAlbums` op. Verify-lane intake variants, worker handlers,
  submit helpers and `prune_stale_asset_path` removed; requirements text
  and generated doc describe inline re-checks. Gates:
  `index_album_sweep_same_hash_keeps_sibling_root_ghost` (shared hash
  across roots), `sweep_remove_batch_*`, `list_asset_paths_under_*`.
  Residual follow-ups: per-subdir scoping (insertion point noted in
  code), dir-album candidates still read `snapshot_tree`.
  Original steps, all GREEN: (1) RED same-hash cross-root + op-semantics
  gates; (2) prefix-scan helper + exact-only batch op; (3) batched job
  with inline re-checks; (4) sweep Verify traffic and prune-before-submit
  removed; (5) shared-hash REQ-4 gate green, full suite green.
- **C10 [done] Per-path concurrency: interactive preempts scan (baseline model)**.
  Key on normalized path (new files have no ID until flush); existing
  assets re-resolve ID->path under the lease and abort on mismatch
  (delete, file/dir assign). Single lock per job; dir moves take subtree
  locks. Scans try-and-abort at stage boundaries and before every flush
  (`superseded` predicate, unit-pinned); interactive paths lease (which
  auto-bumps) then mutate. Sidecar writes hold the lease for the commit
  window. Leases own their generations (bump on acquire, release on
  drop); standalone `bump()` removed. Commits: lease dedup, scan abort,
  lease-owned generations. Gates: `lease_releases_generation_on_drop`,
  `scan_abort`, `superseded_truth_table`, `commit_with` release test.
  Detail below is historical; the summary above is authoritative.
  Requirements: REQ-7 ("scan never waits, interactive never loses").
  Steps, all GREEN: (1) RED lease-dup + scan/edit race gates; (2) dedup
  fix + lease-owned generations (`bump()` removed); (3) stage-boundary
  and pre-flush abort, lease-then-mutate on all mutating paths; (4)
  ID->path re-resolve under lease, subtree locks for moves (advisory
  refcounts: no deadlock possible); (5) full suite green.

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
