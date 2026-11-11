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
  status tags; N3 guarded by C5, N11 refuted against the tree,
  N13 added from C11.

## Steps

1. [done] Survey: operation x aspect matrix with R/W/- markers
   plus pairwise interleaving hazards with file:line evidence.
2. [done] Interaction nodes N1–N13 (aspects, interleaving, guard or
   gap); `[verify]` items resolved where the tree already answers.
3. State extension: aspect-level fields + invariants; keep bounds small.
4. Actions: one action per mutating op at aspect granularity; lift the
   SCOPE-4/5/6 deferrals it covers.
5. Runs: conflicting-pair runs (scan-vs-edit, move-vs-sweep,
   edit-vs-edit, upload-vs-scan, stats-vs-member-write).
6. Gates: `just quint-check`, traceability entries, regen
   `docs/eventual-consistency.md` if the generator consumes the model.

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
