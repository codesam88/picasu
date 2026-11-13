---
status: in-progress
type: feature
priority: medium
area: backend
---

## Notes

Extend the index-pipeline Quint model from whole-row convergence to
per-asset-aspect mutation semantics: which operations mutate which
aspects of an asset (media bytes, sidecar bytes, derived renditions,
record identity fields, metadata payload, dupe membership, album
stats, tree epoch), and which pairwise interleavings can corrupt,
lose, or resurrect aspect state. Goal: the model names every
mutation window the C9/C10 implementation actually guards (leases,
generation abort, resolve-then-validate, inline sweep re-checks) and
proves the guards sufficient; anything unguarded becomes a scoped
assumption or a new requirement.

Out of scope: changing implementation behavior (modeling only),
REQ-1/REQ-8 skip optimization, liveness/fairness properties.

## Decisions

- New state lives in the existing `pipeline/` modules (extend
  `FileState`/`AssetRecord` in `eventual-consistency-state.qnt` with
  aspect fields, or add a focused module that imports it) — decide
  after the survey; keep `just quint-check` green throughout.
- SCOPE-4/5/6 (atomic worker step, synchronous direct writes,
  completed-write crashes) are the lifting targets; each lifted
  micro-stage gets a scope entry with bound/why-safe/lifting.
- 2026-11-12: survey complete, filed as N1–N13 with provenance +
  status tags; N3 guarded by C5, N11 refuted as an open hazard (the
  fixed sidecar-first order is pinned by the rotate runs), N13 added
  from C11.
- 2026-11-12: first asset-aspects slice shipped in
  `utils/quint/pipeline/asset-aspects.qnt`: predicates
  AspectAligned/AspectsAligned/ThumbPresent over existing state; actions
  DirMoveFsOnly, ApiMoveFsOnly, SweepRootLeased, SidecarEditPartial,
  DroppedSidecarModify; 11 runs. Deferred: N5, N8, N9, N10, N13 and the
  full eight-aspect boundary beyond the four-aspect
  (file/sidecar/thumbnail/row) slice.

- 2026-11-12: implementation tracking starts with Rust attributes
  (`operation`/`event` markers backed by the annotation crate, see
  `quint-annotation-crate`), `pipeline/operations.json` for OP-1..OP-12,
  and `utils/quint/operations-check.mjs`. The catalog records event names,
  four-aspect reads/writes, guards, status and N nodes; source markers
  derive the code mapping without line numbers or model references in the
  JSON.
- 2026-11-13: branch rebased specs-only onto `feat/impose-requirements`
  (settled user decision): the quint branch's backend rework was dropped
  in favour of impose's independently reworked pipeline (single-file
  store-and-return upload with background intake, background flush
  advancing read-model invalidation, exact-path watcher removals, album
  stats rebuild outside the write txn). Specs, catalogs, tooling and
  markers were carried losslessly onto the new backend; the carry-over
  review found the upload-semantics descriptions stale (step 7).
- 2026-11-13: lanes outside the four-aspect catalog scope
  (`mark_tree_dirty_background` read-model invalidation advance,
  `handle_sweep_dir_albums`) stay unanchored by design; record the
  exemption explicitly in `operations.json`/README instead of leaving it
  inferable.
- 2026-11-13: upload reframed as store-and-return. Catalogs and model
  described the removed synchronous upload/barrier; upload now places
  the file and fire-and-forget enqueues via submit_uploaded_path ->
  submit_watcher_index. N8 hazard is the unindexed window, not a
  barrier bug. Read-model freshness bounded by mark_tree_dirty_background
  (~1 s leading-edge throttle). Freshness gate:
  docs-eventual-consistency-check added to check aggregate.
- 2026-11-13: modeling roadmap adopted (steps 8–13). Two findings drive
  it: (a) the 2026-11-12 survey predates the merged pipeline — node
  guard claims must be re-verified against this tree before new
  modeling (the leases/exact-path/dir-move guards survive via impose's
  independent implementations, but shapes and evidence need re-checks);
  (b) `SweepRootLeased` is more atomic than
  `sweep_stale_asset_paths`: the code enumerates, then re-checks
  generation + mutation-lease busy + FS per candidate, then submits an
  exact-path batch the serial writer resolves-then-validates — the
  windows those guards close are invisible in the model as written.
  Intake supersession is already modeled (WorkerSkipSuperseded).

## Steps

1. [done] Survey: operation x aspect matrix with R/W/- markers
   plus pairwise interleaving hazards with file:line evidence.
2. [done] Interaction nodes N1–N13 (aspects, interleaving, guard or
   gap); `[verify]` items resolved where the tree already answers.
3. [partial] State extension: no new state fields; aspect alignment
   expressed as predicates over existing `FileState`/`AssetRecord` in
   `asset-aspects.qnt` (AspectAligned/AspectsAligned/ThumbPresent).
   Full aspect-field split (media bytes vs sidecar vs rendition vs
   identity) deferred.
4. [done] Actions: DirMoveFsOnly (OP-5), SidecarEditPartial (OP-7),
   DroppedSidecarModify (environment) in `asset-aspects.qnt`; all update
   every state var.
5. [partial] Runs: 9 conflicting-pair runs modeled (scan-apply,
   scan-supersedes-edit, move-vs-sweep, dir-move-crash-heal,
   sidecar-partial-edit, dropped-sidecar, thumb-loss, watcher-remove,
   rotate). Deferred: N5/N8/N9/N10/N13 and stats-vs-member-write (N13),
   upload-vs-scan (N8), edit-vs-edit beyond N4, full aspect boundary
   (media bytes vs sidecar vs rendition vs identity vs metadata vs dupe
   vs album stats vs tree epoch).
6. [done] Gates: `just quint-check` (33 convergence + 44 asset-aspect
   executions = 33 imported + 11 new), traceability regenerated for the
   new definitions, `operations-check` and `specs-check` green; the
   generated doc now counts both model entries in its Proof line.
7. [done] Spec-alignment pass onto the impose pipeline (found by the
   2026-11-13 carry-over review; all catalog checks stay green while
   these describe the dropped pipeline, so nothing else catches them):
   - `events.json` API_UPLOAD: drop "row written synchronously; the
     intake queue is not involved" / upload-barrier effects; the
     endpoint stores the file and fire-and-forget enqueues via
     `submit_uploaded_path` → `submit_watcher_index`
     (`post_upload.rs:286-294, 534-538`).
   - `goals.json` G3: the flush-barrier note cites
     `post_upload.rs:330`, which is now content-validation code; rewrite
     G3 around the read-model freshness contract
     (`mark_tree_dirty_background`, ~1 s leading-edge throttle,
     `writer.rs`, contract test in 265ae9f0).
   - `scope.json` SCOPE-5 and `eventual-consistency.qnt` `ApiUpload`:
     model upload as store + intake enqueue, not a synchronous row
     write.
   - `operations.json` OP-2: `writes` shrinks to `asset_file`
     (db_row/thumbnail belong to OP-1/`process_file`); replace the
     `FlushTreeTask::insert barrier` guard — the barrier survives only
     on edit/regenerate/rotate routes, not upload.
   - Rewrite node N8 (below).
   - Regenerate `docs/eventual-consistency.md`; decide whether
     `docs-eventual-consistency` joins the `check` aggregate as a
     freshness gate (currently nothing detects catalog→doc drift).

   Outcomes:
   - events.json API_UPLOAD rewritten: store-and-return, intake enqueue,
     eventual visibility window, crash between placement and intake
     pickup (covered).
   - goals.json G3 rewritten around mark_tree_dirty_background freshness
     contract; upload 200 implies stored, not visible.
   - scope.json SCOPE-5 and model ApiUpload now store + intake enqueue;
     ApiUpload no longer a synchronous projection write.
   - operations.json OP-2 writes=["asset_file"], guard "none —
     fire-and-forget intake enqueue"; N8 reframed as unindexed window.
   - N8 updated in plan (below) as by-design eventual window.
   - docs/eventual-consistency.md regenerated; freshness gate added as
     docs-eventual-consistency-check in check aggregate (green).

## Known gaps and next phase

- Model tracking is one-directional: `operations-check` validates that
  every OP has a backend marker and that model `@operation` tags are
  known, but it does not require every guarded OP to have a model action,
  nor every event to have an operation claimant.
- Guards/statuses are free text. The next phase should link guard strings
  to REQ/C ids (or at least notes with code paths) so OP-4/OP-5/N11-style
  contradictions are machine-detectable.
- Existing model actions (OP-1/3/4/6/9/12) are not yet tagged with
  `@operation`; only the new aspect actions are. Add those tags before
  requiring reverse coverage.
- Event markers cover the main handlers; watcher multi-event paths and
  metadata-edit handlers remain unannotated.
- `quint-check` and `operations-check` are precommit/check-aggregate
  gates, not direct CI job steps; decide whether CI should run `just
check` or the individual recipes.
- Doc freshness: `docs-eventual-consistency` is not in the `check`
  aggregate, so catalog→generated-doc drift is invisible (found by the
  2026-11-13 review; fold into step 7).
- Docs pointers: the justfile `quint-check` comment says
  "eventual-consistency spec" while the recipe checks two specs, and
  `docs/quint-modeling.md` does not mention the operations/annotation
  layer (tracked in `quint-annotation-crate`).

## Next phase (roadmap 2026-11-13)

8. [open] Re-verify the interaction survey (N1–N13) and the OP guard
   strings in `operations.json` against the merged tree; refresh node
   statuses, guard strings and evidence. Blocks all modeling below.
9. [open] Model the unindexed upload window (N8) in the four-aspect
   slice: `UploadStoredOnly` (file present, row/thumb/sidecar absent —
   unaligned by design), upload-vs-sweep on a reused path (stale prior
   row → prune → re-index vs prune-after-upload keeps a stale row until
   intake applies), watcher-remove before `WorkerApply` (no resurrect),
   upload-vs-sidecar/rotate on a not-yet-indexed file, tmp residue
   (REQ-10). Safety targets: no partial-aspect row persists from an
   incomplete upload; a file deleted before first indexing cannot
   resurrect an asset; a stale row on an overwritten path converges
   after intake applies.
10. [open] Refine sweep to the code's granularity (SCOPE-4 lifting):
    enumerate → per-candidate re-check (generation unchanged, mutation
    lease not busy, FS) → exact-path batch → writer resolve-then-validate;
    model the superseded-batch case. Makes N2's move-vs-sweep window
    visible; each lifted micro-stage gets a scope entry
    (bound/why-safe/lifting).
11. [open] Widen the aspect boundary from 4 to 8 aspects (media bytes,
    sidecar bytes, rendition, identity, metadata payload, dupe
    membership, album stats, tree epoch). Rewrites the predicates the
    slice sits on; prerequisite for N13 (stats vs member writes),
    dupe/hash-group delete semantics and metadata torn writes.
12. [open] Guard linkage + reverse coverage: `@operation` tags on the
    `eventual-consistency` actions, guard strings linked to REQ/C ids,
    then annotation rung 1 (registry + static coverage gate) for
    bidirectional OP↔action↔event coverage; rung 2 (runtime trace
    reconciliation) as the accuracy validator against observed behavior.
13. [open] Decide the read-model freshness lane: model bounded staleness
    for `mark_tree_dirty_background` (~1 s leading-edge throttle) or
    document it as a standing assumption; closes G3.

## Interaction nodes (survey 2026-11-12, claims need tree-verification)

Source: read-only worker survey (Beam Free) of `backend/src` against
`utils/quint/pipeline/*.qnt`. Aspect legend: (a) media bytes,
(b) sidecar bytes, (c) derived renditions, (d) `AssetRecord` identity,
(e) `ASSET_BY_PATH`, (f) `METADATA_TABLE` payload, (g) `DUPE_INDEX`,
(h) dir-album cache + pending set, (i) album stats, (j) tree epoch,
(k) expire table. Full matrix in conversation record; nodes below are
what the model must cover. Status tags: [guarded] code already
orders it, [open] needs modeling + maybe a requirement, [verify]
survey claim contradicts a done item — re-check the tree first.

- N1 scan-vs-sidecar-mutating edit/rotate (b,f): torn composition
  flushed over the edit. [guarded] C10 abort. Model the abort rule.
- N2 move rename→rewrite window vs sweep/remove (d,e): missing-path
  rows mid-move pruned by another op. [guarded] C9 inline re-check +
  C10 leases. Model the re-check + lease rule (replaces old
  Verify-lane actions).
- N3 dir-move vs in-flight intake + crash between FS move and row
  rewrite; split `From`/`To` dir events. [guarded 2026-11-12 by C5:
  cookie-paired prefix rewrite, DB-state fallback for unpaired halves,
  expired entries drain to removals]. Residual for the model: crash
  between FS move and row rewrite still heals only via sweep/backstop.
- N4 concurrent sidecar edits: read-modify-write LWW, best-effort
  rollback leaves sidecar ahead of cache. [open].
- N5 assign cross-table writes (`put`/`remove` path maps, per-record
  dir-move loop) are separate txns; crash splits them. [open]
  (touches M3 wording).
- N6 derived corruption undetected; regen frame can orphan on unknown
  id. [open] (returns with REQ-8).
- N7 sidecar-only watcher events dropped (`.xmp` filtered) — delayed
  metadata until media rescan. [open].
- N8 upload 200-before-visibility. [open, reframed 2026-11-13: with
  store-and-return + background intake the 200-before-visibility window
  is by-design eventual, not a barrier bug. The stale note about
  `FlushTreeTask::insert([])` + `tree_changed()` described the dropped
  pipeline and no longer applies. Model the unindexed window instead:
  file stored but not yet picked up by intake, watcher supersession
  semantics, and whether scan-vs-upload can resurrect or drop an
  uploaded file before first indexing.]
- N9 reset drops tables but leaves derived bytes orphaned. [open,
  minor].
- N10 share/cover edits are DB-only with no lease; can race a scan
  of the same album. [open, minor].
- N11 rotate thumbnail-before-sidecar window. [refuted 2026-11-12:
  `rotate_image.rs` persists the sidecar before generating the
  thumbnail and flushing the DB. Do not model; survey claim was stale.]
- N12 sweep hash-group cross-root reap. [guarded] closed by C9
  exact-path batches. Model the exact-path rule.
- N13 cascade album stats vs concurrent member writes (from C11).
  [guarded] stats compute from a per-entry snapshot; the write txn
  re-checks liveness per member/row; read/write races re-mark pending
  for the next cascade. Bounded staleness, no lost update.
