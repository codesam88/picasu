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

- 2026-11-12: implementation tracking starts with no-op Rust attributes
  (`quint_annotations::operation`/`event`, crate
  `utils/quint-annotations`), `pipeline/operations.json` for OP-1..OP-12,
  and `utils/quint/operations-check.mjs`. The catalog records event names,
  four-aspect reads/writes, guards, status and N nodes; source markers
  derive the code mapping without line numbers or model references in the
  JSON.

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
- N8 upload 200-before-visibility. [open, still current 2026-11-12:
  `post_upload.rs` waits on `FlushTreeTask::insert([])` + `tree_changed()`.
  Model must decide whether the empty insert actually barriers the
  serial writer/cascade; if not, it is a real premature-ack hazard.]
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
