---
status: open
type: feature
priority: medium
area: backend
---

## Goal

The implementation-tracking layer — backend markers, the
`pipeline/operations.json` + `events.json` catalogs, and the model
`@operation` tags — is checked against the implementation and the Quint
model, so semantic drift between them fails a gate. Coverage is the
focus; the read-model freshness lane was dropped (quint-asset-aspects
step 13) as not a modeling fit.

## Decisions

- 2026-11-13: the crate lives at `utils/quint`, not in a separate
  `utils/quint-annotations` directory. A `proc-macro = true` crate
  ignores non-Rust files, so the `.qnt` specs, `.mjs` scripts and
  `pipeline/*.json` catalogs stay where they are and the macros read the
  catalogs they validate from their own crate dir
  (`CARGO_MANIFEST_DIR/pipeline`). Attribute path is
  `#[quint::operation("OP-N")]` / `#[quint::event("NAME")]`. Known
  constraint: a proc-macro crate cannot export ordinary Rust items; a
  runtime library facet would need a second crate.
- 2026-11-13 (rung 0): compile-time id validation, not a registry. The
  macro parses the attribute argument, checks it against the catalog
  (serde_json), and emits `compile_error!` on an unknown id; `build.rs`
  sets `rerun-if-changed` on the catalog JSONs so catalog edits re-expand
  backend markers.
- 2026-11-13 (rung 1): the gate is smarter attribute-aware text parsing
  in `operations-check.mjs`, not a macro-emitted registry. A marker is
  counted only on a real attribute line (`^\s*#\[[...]`), so comment,
  doc-comment, string and `#[cfg(test)]`-gated mentions no longer satisfy
  the scan. No new dependencies (workspace carries neither `inventory`
  nor `linkme`); a macro-emitted registry remains available if a later
  rung needs one.
- 2026-11-13 (rung 1): event ownership rule — an event is owned iff it
  appears in at least one operation's `events` array; ownerless events
  must be read-only boundary events (`API_READ_*`) or recovery events
  (`PROCESS_CRASH`, `DERIVED_STORE_LOSS`, `DB_LOSS`), enumerated in
  `EVENT_OWNER_EXEMPT` in `operations-check.mjs`.
- 2026-11-13 (rung 1): CI placement — the gate stays in the `just check`
  aggregate; no separate CI job without user sign-off.
- 2026-11-14 (rung 2 reframe): runtime trace reconciliation is replaced
  by property-anchored test coverage (rungs 2–5 below). Rationale: a
  model↔code trace comparator needs a manual action↔function mapping
  layer and becomes a second drift surface in exactly the dimension it
  exists to detect; the interesting scenarios are the nondeterministic
  ones (per-scenario harness cost, cf. the writer contract test); and
  aspect-access tracing would need a second annotation surface on the
  `asset_store` helpers. The model's named invariants and deterministic
  runs, chained through `traceability.json` (REQ → model evidence → code
  test), give the same detection power for the properties that matter at
  far lower cost. The old rung-4 coverage report is subsumed by the
  rung-3 matrix; dylint stays parked.

## Current state (2026-11-14)

- Rung 0 done: `utils/quint` is a validating proc-macro crate; markers on
  12 cataloged OPs; `-p quint` wired into the justfile utils recipes;
  README states what is and is not enforced; `utils/quint-annotations`
  never landed on main (spec-only carry-over history).
- Rung 1 landed in `operations-check.mjs`: attribute-aware marker
  parsing; bidirectional OP↔marker coverage with per-OP marker counts
  printed; event ownership enforced; guarded OPs without a model
  `@operation` tag are report-only (1 remains after step-11 tagging of
  MemberWrite/AlbumTaskApply).
- Model: `asset-aspects.qnt` carries per-asset aspect state
  (assetById/assetByPath/dupeIndex/metadataPayload/thumbPresent/
  albumPending) and named invariants per guard (e.g.
  `SweepRemoveBatchSkipsPresentFile`, `AlbumStatsConsistentWhenDrained`);
  `Converged` is cross-table alignment.
- Catalogs: `operations.json` guards were re-verified against the merged
  pipeline (quint-asset-aspects step 8) but remain free text — not yet
  linked to named model vals.
- `traceability.json` links REQs → model evidence (vals, runs); no
  code-test refs yet.
- Runtime: the aspect suite is ~4 min at 10k samples/run; `just check`
  exceeds 10 min. CI placement and sample counts are open (below).

## Steps

0. **[done] Rung 0 — migrate to `utils/quint` with expansion-time
   validation.** Crate + macros + `build.rs` rerun-if-changed; drop of
   the never-on-main no-op crate; justfile `-p quint` wiring; README
   guarantee corrected. Exit met: a typo'd OP id in a backend marker
   fails `cargo check` (demoed with `OP-99`).

1. **[partial] Rung 1 — static coverage gate.** Landed: marker-on-real-
   item (attribute-aware parsing), reverse coverage (cataloged OP without
   marker fails; unknown marker id fails), event ownership (rule above),
   report-only model coverage. Exit met for: missing/extra OP, event
   ownership, comment-satisfies-scan hole (demoed by commenting out a
   real marker and by removing an event from its owner). Not met:
   aspect read/write set matching and wrong-but-valid id correctness —
   markers carry only ids; see rungs 2/5.

2. **[open] Rung 2a — property linkage.** `operations.json` guard
   strings reference named model vals (e.g.
   `SweepRemoveBatchSkipsPresentFile`), and the gate validates that each
   referenced val exists in the `.qnt` (specs-check already catalogs
   declarations, so this is a cheap cross-check). Tag the last guarded
   untagged OP; once the untagged count is 0, flip reverse coverage from
   report-only to failing.
   Exit: a guard naming a nonexistent model val fails the gate; no
   report-only warnings remain.

3. **[open] Rung 2b — coverage matrix.** `traceability.json` gains
   code-test refs per property; a report shows property → model evidence
   → code test in one place, with properties lacking a backend test
   visible as gaps (report-only first). The gate validates that test refs
   point at existing test functions, mirroring the model-symbol check.
   Exit: the matrix is generated, reviewable, and drift between the
   three layers is visible in one report.

4. **[open] Rung 2c — close the gaps.** Add the missing backend tests,
   TDD, one at a time, highest-value first. Backlog seeded by the
   review sketches: upload-gets-indexed (delete the
   `submit_uploaded_path` call while every catalog check stays green —
   a code test must catch it), sweep never prunes a row whose file is
   present, dupe-group-conditioned thumbnail reap, N5 split-window heal,
   album-stats bounded staleness with late member write.
   Exit: every linked property has a code test or an explicit exemption
   with a recorded reason.

5. **[open, optional] Rung 2d — test-only operation ledger.** Only if
   sequencing gaps remain after 2c: the macros emit
   `quint_log(op)`/`quint_log(event)` at fn entry under `cfg(test)`, and
   a few hand-picked interleaving tests assert the expected sequence
   (sweep-vs-move, upload-vs-scan). Explicitly NOT a generic tracing
   layer and no aspect-access instrumentation. Exit: the picked
   scenarios fail on order drift; nothing else changes.

6. **[parked] Dylint local rules.** Lease/resolve-before-mutation,
   mutators confined to annotated operations, annotated operations
   reachable from an event handler. Revisit only when a specific rule
   justifies the HIR/lint maintenance cost against the test-based
   coverage above.

7. **Not relevant: taint/flow analysis.** MIRAI/Flowistry/Charon are
   data-flow tools; revisit only if a specific provenance property
   (for example sidecar bytes reaching a row) cannot be checked by the
   layers above.

## Open questions

- Wrong-but-valid id correctness (a marker naming OP-1 where OP-9 was
  intended): detectable only by rung-2d sequences or human review;
  decide whether any gate is worth it or it stays a review concern.
- Suite runtime: reduce per-run sample counts in the aggregate lane
  (keep 10k for deep runs), split recipes, or a dedicated drift job —
  `just check` exceeds 10 min and grows with every modeled node.
- `EVENT_OWNER_EXEMPT` maintenance: who may add entries and with what
  evidence, so the ownership rule does not erode.
- Guard-string rewrite in rung 2a touches every `operations.json`
  guard: one pass, or batched with the next catalog edit.

## Terminology rule

Model/catalog names map to code entities per
`utils/quint/README.md` ("Model-to-code terminology"): direct stable
mappings take the code name; conceptual names carry doc-comment anchors;
renames are separate small commits; unqualified role words ("the
writer") are not used.
