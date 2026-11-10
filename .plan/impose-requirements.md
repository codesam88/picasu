---
status: in-progress
type: feature
priority: high
area: backend
---

## Goal

Implement the implementation obligations in `specs/requirements.json`
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
- Scan-vs-mutation coordination: one shared mutation gate. Sweep and the
  path-mutating API (assign/delete/directory move) hold it across their
  filesystem+row transaction; the scan walk does not. This is the
  smallest mechanism that satisfies REQ-7 without blocking reads.
- Renames: factor notify event classification into a pure function so
  rename handling is testable. File rename submits old-path removal plus
  new-path index; directory rename removes rows under the old prefix and
  indexes the new tree.
- Atomic writes (REQ-13) are verification-only: the temp+rename helpers
  already replace atomically; add an injected partial-write failure and
  pin the target bytes. No production behaviour change unless the gate
  exposes a gap.
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

- **S7 [ ] REQ-7: scans coordinate with mutating API work**.
  Scope: new mutation gate module, `album_index.rs` sweep, `assign_album.rs`,
  `delete.rs`, directory-move path.
  RED: a scan paused before sweep plus an overlapping assign must keep the
  moved row and its sidecar; today the sweep can prune it. Add the gate,
  plus a focused primitive test and the integration overlap test.

### Phase 3 - scan triggers and cycle semantics

- **S8 [ ] REQ-5: honor `Flag::Rescan`**.
  Scope: `tasks/batcher/start_watcher.rs`.
  RED: extract a pure event classifier/root selector; a synthetic
  `EventKind::Other` + `Flag::Rescan` resolves to the affected root and
  submits a rescan. Wire the watcher loop to it.
- **S9 [ ] REQ-6: configurable per-root backstop scan**.
  Scope: `model/config.rs`, `docs/config.md`, `lib.rs`/looper.
  RED: config parses/validates/defaults the interval; with a 50 ms
  interval and the watcher disabled, a missed change converges within one
  cycle. Default `0` starts no loop.
- **S10 [ ] REQ-14: non-overlapping, drain-complete backstop cycles**.
  Scope: backstop scheduler.
  RED: a scheduler state-machine test: tick during a running cycle marks
  one pending follow-up, never overlaps, and completes when the scan and
  intake drain return.

### Phase 4 - watcher renames

- **S11 [ ] REQ-2: file rename submits remove plus create**.
  Scope: `start_watcher.rs` + batching.
  RED: real-FS rename of a watched file; the row disappears at the old
  path and appears at the new one without a sweep.
- **S12 [ ] REQ-3: directory rename follows descendants**.
  Scope: `start_watcher.rs`, album index/asset store prefix query.
  RED: real-FS rename of a watched directory; every descendant row moves
  to the new path with assetId preserved and no ghosts.

### Deferred

- **REQ-1** sidecar identity for the hash short-circuit: returns with the
  optimization; gate stays recorded in `specs/requirements.json`.
- **REQ-8** per-row thumbnail checksum: needs a skip consumer; rescans
  already regenerate derived artifacts. Returns with REQ-1.

## Verification per step

- RED transcript before implementation; GREEN plus neighbouring tests
  after.
- `just backend-test` for each backend step (serial, no concurrent runs).
- `just check` package before push; `spec-check`, `catalog-check`,
  `events-check`, `plan-lint`, `gates-check`, prettier on doc changes.
- Update `specs/requirements.json` status and regenerate
  `docs/eventual-consistency.md` when the status changes.
- Push after each phase; open one stacked draft PR to
  `spec/quint-index-pipeline` and read CI before starting the next phase.
