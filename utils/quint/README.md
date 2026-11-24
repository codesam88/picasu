# utils/quint

Checkable models of subsystem protocols in `utils/quint/pipeline/`. Each
model is exercised by `just quint test` (typecheck plus `quint test`);
exploratory simulation is `quint run` and never gates.

## Assumption rules

Assumptions record what the model takes as given: operating
conditions it relies on, non-goals it excludes, and properties met
by components out of scope here.

- Assumptions are expensive and require human review.
- If the assumption can be reasonably expressed as a requirement
  onto the implementation, it is a requirement, not an assumption.
- If the assumption has no formal representation in the model,
  it is likely vacuous or redundant. Consider consolidating.

Current contents of `pipeline/`:

- `merged.qnt` — the pipeline model in one module: shared state and finite
  bounds (five paths, two roots), the snapshot matcher and the
  `NoLostUpload` / `NoPermanentStale` safety predicates, the `Converged`
  goal, the protocol as prescribed event handling (watcher/API boundary
  events, recovery events, intake submits, worker steps, per-root
  scan/sweep), the four-aspect slice (asset file, sidecar,
  thumbnail/derived, DB asset row), and the proof-lane completion machine.
  Invariants: no lost intake work, no permanent stale rows; the
  convergence goal is `Converged` (every processable asset path has
  exactly one correct row; every row's asset exists; nothing queued),
  pinned per diverged shape by the runs. Unprocessable paths are logged,
  never indexed, and are excluded from the goal.

  Sections are marked with `// ---------- section: ... ----------`
  comments; declarations share one namespace, so the file is the unit of
  scoping. Because `step` and `stepCompletion` live side by side, each
  carries a `@step-universe` tag naming the actions it must cover; the
  steps gate checks universe membership, branch presence, and that no
  declared action falls outside every universe.

- `entry-canonical.qnt` — the canonical-bounds entry: a plain
  `import pipeline_merged.* from "./merged"`, executed by
  `just quint test`. The bounds are the module's own `PATHS`, `HASHES`,
  `SMALL`, and `ROOTS` values.

  `stepCompletion` enumerates every base and asset-aspect action over its
  parameter ranges in a three-phase cycle (`phase` 0 free, 1 draining,
  2 scanning; `scannedRoots` watermark). Phase 0 admits every action;
  phase 1 freezes the environment and admits the work-consuming
  branches, crashes, and untracked rescans; phase 2 takes one tracked
  `CompleteScan` per root and drains the work it creates. `Quiescent` is
  the drained-state predicate and `CompletionInvariant` claims the goal
  at phase-2 quiescence. Its sanity runs are simulation-only: they assert
  per-tick properties or walk one deterministic path, so they run with
  the suite without claiming the invariant over all paths.

- `operations.json` — internal mutation operations OP-1..OP-12 with
  events, aspect reads/writes, guards, status and interaction nodes;
  validated by `just operations-check` against backend annotations.
  Out of scope by design: the read-model invalidation advance
  (`mark_tree_dirty_background`) and the dir-album sweep lane
  (`handle_sweep_dir_albums`); tree-epoch modeling is deferred
  (node N5 / the full aspect boundary).
- `events.json` — boundary and recovery events with their settled
  effects and known faults, validated by `just events-check`.
- `goals.json` — goals G0–G4 with decision (`committed`, `queued`,
  `dropped`), anchors and assumptions; only committed goals source
  requirements. Validated by `just catalog-check`.
- `requirements.json` — implementation obligations (REQ-1..n) derived
  from committed goals. Each entry carries the contract (`statement`,
  `derived_from`, `owner`, `gate`, `status`) plus the review text
  (`implementation`, `tests`, optional `limits`). `catalog-check`
  enforces that a requirement derives only from committed goals, that
  every committed goal derives at least one requirement, and that a
  `verified` requirement names at least one gate test. The generated
  `docs/eventual-consistency.md` renders the contract under
  Requirements and the review text under Implementation.
- `assumptions.json` — index of ASS-1..n with owner and status;
  the model's `@assumes` tags state which definitions each one
  grounds, and the generated traceability map carries the reverse
  refs. Validated by `just catalog-check`.
- `scope.json` — modeling bounds and abstractions (SCOPE-1..n), each
  with bound, why_safe, lifting step, and provisional flag where the
  safety argument is weaker than the implementation's guarantee. A
  top-level `modeled_scope` summarizes the current model boundary;
  validated by `just catalog-check`.
- `catalog.schema.json` — required fields and enums shared by the
  catalog validators.
- `traceability.json` — machine-generated mapping written by
  `just spec-check --generate`. The audit fails on drift.
  Load-bearing references appear in `.qnt` action comments and
  `events.json` notes. Code-test refs are hand-maintained in
  `operations.json` `properties` instead.

Each catalog file carries a top-level `description`; the generated
review doc renders it as the section introduction. Requirements derive
from committed goals (never from dropped or queued ones), and
`@requires` tags in the model resolve against `requirements.json`. A requirement stays `open` until its gate test
exists, and is `imposed`/`verified` only with that gate.

Conventions for new subsystems: name the file for the subsystem, keep
bounds tiny (2–3 of each entity), state enabling conditions explicitly,
and record every derived contract where its enforcement lives (see
`docs/quint-modeling.md` for the full flow). Simulator
counterexamples become regression tests or documented transients —
never silent.

## Structure

- `quint-annotate/` — the validating proc-macro crate (package name stays
  `quint`; attribute path `#[quint::operation(...)]` / `#[quint::event(...)]`).
- `coverage/` — the TypeScript validation tool (dispatcher
  `coverage/bin/check.ts`).
- `pipeline/` — `.qnt` specs and JSON catalogs.
- `tests/` — checker self-tests.

## Tooling

- `coverage/bin/check.ts specs` — closed-world tag audit and the generated
  `pipeline/traceability.json` map.
- `coverage/bin/check.ts steps` — branch-completeness audit for step
  actions: every `action` declaration reachable through a `step*` module's
  imports must appear in its `any { ... }` list (`init`/`init<Name>` entry
  actions and the step actions themselves are exempt). When several step
  actions share one module, each names its own universe with
  `// @step-universe A, B, C` (repeatable; lines merge): the tag must list
  only declared actions, each listed name must appear in that step's branch
  list, and every declared action of the module must appear in at least one
  step universe — coverage is per step, never silently dropped. A forgotten
  branch or an uncovered action fails the gate instead of silently narrowing
  the verified reachable set; stutter (`Skip`) presence is not required.
- `coverage/bin/check.ts events`, `coverage/bin/check.ts catalog` — catalog
  validation against `pipeline/catalog.schema.json`.
- `coverage/bin/check.ts ops` — validates `pipeline/operations.json` and the
  `#[quint::operation(...)]` / `#[quint::event(...)]`
  markers in `backend/src/**/*.rs`.
- `coverage/bin/check.ts docs` — writes `docs/eventual-consistency.md`. The
  run sources named after the output file (the files `just quint test`
  executes) supply the status line's run counts, so the generator hardcodes
  no model file name.

## Operations and annotations

`pipeline/operations.json` tracks the implementation of the four-aspect slice
(asset file, sidecar, thumbnail/derived, DB asset row) as operations OP-1..OP-12.
Each operation records its events, aspect reads/writes, guards, optional
`properties` objects `{model, test?}` linking guarded behavior to model
declarations and backend tests, status and interaction nodes. Backend functions
carry attribute markers from the `quint` crate
(`#[quint::operation("OP-N")]`,
`#[quint::event("EVENT_NAME")]`). Compile-time validation (rung 0 of the
annotation crate) rejects unknown or misspelled operation/event ids at
`cargo check -p picasu`, so a typo'd or renamed OP id fails the build
instead of surfacing only in `just check`. `operations-check.mjs` remains the
catalog-side backstop and is the rung 2a/2b gate: it counts a marker only
when it sits on a real attribute line (comment, doc-comment, string and
`#[cfg(test)]`-gated mentions do not count), requires every cataloged OP to
have at least one marker and rejects markers naming an unknown OP, requires
every event to be owned by at least one operation or to be an explicitly
exempt read/recovery event, validates each `properties[].model` against the
declarations (`val`/`def`/`action`/`run` from `utils/quint/pipeline/*.qnt`),
validates each `properties[].test` ref, and fails when a guarded OP has no
`@operation` tag in the model. It prints a per-OP coverage matrix, marks
properties without a test ref as `NO TEST`, and reports
`properties with test: X/Y`. What is still NOT enforced (rung 2+): a marker
naming a wrong-but-valid id still compiles (id correctness against the
function is not checked), and aspect read/write set and order drift between
model and implementation is not checked. The markers carry only ids; aspect
read/write set matching is rung 2+ material.

Each operation may carry an optional `properties` array. Each entry is
`{"model": "<declaration>", "test": "<path>::<test_fn>"}`; `test` is optional
and its absence is a visible coverage gap, not an error. `model` names an
invariant, val or run that pins the guarded behavior; `guards` strings remain
code anchors. Empty `properties` is allowed when an OP has no model evidence
(e.g. OP-11 RESET).

A test ref is a repo-relative `.rs` path plus `::` and the test function name,
for example
`backend/src/tests/scan_abort.rs::scan_result_dropped_when_path_busy`.
`backend/tests/*.rs` and `backend/src/tests/*.rs` are both accepted. The gate
reads the referenced file and requires the text `fn <name>` to appear; a
comment mention of the name therefore also satisfies it (accepted limitation:
this is a link check, not compiled-test discovery). A ref may also point at a
generated backend scenario by ending in `.yaml`, in which case it must name an
existing file under `backend/tests/scenarios/` (file-existence check only;
malformed YAML already fails the build via `build.rs`). Generated scenario
tests live in `OUT_DIR` and cannot be referenced under the `.rs` convention.

Code-test refs stay in `operations.json`, not in the generated
`pipeline/traceability.json` map: `specs-check.mjs` rewrites the traceability
map and fails when it is stale, so it cannot carry hand-maintained links.

The Quint toolchain installer stays in `scripts/deps/quint.sh` with the
shared pins in `scripts/deps/versions.env`.

Modeling scope (not assumptions, no formal counterpart required):
no cross-root identity in v1 — moves across watched-root
boundaries are observed as creation plus ghost and deletion; walk
roots are disjoint from the derived store, so pipeline writes
cannot resubmit themselves.

## Source of truth

The axiom (ASS-3): any external or user filesystem modification — edits,
renames, deletions, restores from backup, crash residue — is
authoritative over DB state and is adopted as part of convergence. This
is a design premise, not a checked property; watcher events, rescan,
snapshot comparison and the sweep operationalize it.

The goal (G5): the system converges based on the filesystem as the
primary truth — asset media bytes, the asset's stored path and its
sidecar (`.xmp`, or `.albuminfo.xmp` for albums) carry user-authored
metadata. The redb tables (`ASSET_BY_ID`, `ASSET_BY_PATH`,
`DUPE_INDEX`, `METADATA_TABLE`) and the thumbnail/derived store are
secondary and exist for speed and caching only: loss, orphaning or
tearing of derived state is not a goal violation because convergence
rebuilds it. The enumerated exception is DB-only non-derived state —
`share_list` (album access state) — which cannot be rebuilt from the
filesystem and must be individually guarded. Model state vars carry
their role (authoritative / derived / DB-only exception); when a survey
note says "loss", the role decides whether that is a violation.

## Model-to-code terminology

Rule: model identifiers must map to a concrete class, function,
operation or table in the implementation. When the mapping is direct and
stable (op names, function names, table names), the identifier takes the
code entity's name — e.g. `SweepRemoveBatchApply` is named after
`WriteOp::SweepRemoveBatch` / `flush_tree.rs::sweep_remove_batch`. When
the mapping is conceptual or likely to move (phases inside one function,
per-worker branches, generic state), the identifier keeps a descriptive
name and carries a doc-comment anchor (`// <term> maps to <entity>`), as
in the table below. Renaming an identifier is done only when its mapping
is stable, and as its own small commit — never bundled with behavior
changes. Unqualified role words like "the writer" are not used in model
names, catalogs or docs; name the entity
(`tasks/queue/writer.rs`, `flush_tree.rs::sweep_remove_batch`,
`tasks/queue/worker.rs`).

| Model identifier                                                                                                                    | Code anchor                                                                                                                                   |
| ----------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `files`                                                                                                                             | filesystem under the data roots (media bytes + sidecar bytes)                                                                                 |
| `generations`                                                                                                                       | `tasks/path_epoch.rs` `GENERATIONS` via `next_generation`/`current_generation`                                                                |
| `intake`                                                                                                                            | `IntakeItem` set, `tasks/queue/intake.rs`                                                                                                     |
| `rows`                                                                                                                              | `AssetRecord` in `storage/asset_store.rs` `ASSET_BY_ID`/`ASSET_BY_PATH`                                                                       |
| `assetById`                                                                                                                         | `ASSET_BY_ID` table in `storage/asset_store.rs` (`AssetRecord`)                                                                               |
| `assetByPath`                                                                                                                       | `ASSET_BY_PATH` table in `storage/asset_store.rs`                                                                                             |
| `dupeIndex`                                                                                                                         | `DUPE_INDEX` table in `storage/asset_store.rs`                                                                                                |
| `metadataPayload`                                                                                                                   | `METADATA_TABLE` in `storage/asset_store.rs` (`MetadataRecord`)                                                                               |
| `thumbPresent`                                                                                                                      | thumbnail store paths keyed by content hash (derived rendition presence)                                                                      |
| `AssetRecord` (model type)                                                                                                          | `model/asset.rs` `AssetRecord`                                                                                                                |
| `MetadataRecord` (model type)                                                                                                       | `model/metadata_record.rs` `MetadataRecord`                                                                                                   |
| `sweepBatch`                                                                                                                        | `sweep_stale_asset_paths` survivors → `WriteOp::SweepRemoveBatch`                                                                             |
| `leasedPaths`                                                                                                                       | `EXACT_LEASES`/`SUBTREE_LEASES` in `tasks/path_epoch.rs`                                                                                      |
| `sweepPending`                                                                                                                      | per-root sweep pending flag set by `ScanRoot`/the scan job in `tasks/actor/album_index.rs`                                                    |
| `nextAssetId`                                                                                                                       | asset-id allocation via `generate_random_hash` in `model/asset.rs` (`AssetRecord::new_media`/`new_album`) and `flush_tree.rs`                 |
| `unprocessable`                                                                                                                     | model-only; ingest failures are logged via `note_file_failed`/`handle_error`, not a stored set                                                |
| `phase` / `scannedRoots`                                                                                                            | model-only proof-lane completion machine (free → draining → scanning); no backend counterpart                                                 |
| `SweepEnumerate`                                                                                                                    | enumeration loop of `tasks/actor/album_index.rs::sweep_stale_asset_paths`                                                                     |
| `SweepRecheck`                                                                                                                      | inline re-check loop of `sweep_stale_asset_paths` (generation, `is_busy`, `path.exists`, pre-submit)                                          |
| `SweepRemoveBatchApply`                                                                                                             | `flush_tree.rs::sweep_remove_batch`, submitted as `WriteOp::SweepRemoveBatch` via `tasks/queue/writer.rs::submit`; exists-skip + resolve only |
| `MoveLeaseAcquire`                                                                                                                  | `assign_album.rs` move window opening `lease_paths(source, dest)`                                                                             |
| `MoveRowRewrite`                                                                                                                    | `assign_album.rs::move_asset_into_album` row rewrite (`put_asset_by_id` + `remove_asset_by_path` + `put_asset_by_path`) and lease release     |
| `UploadStoredOnly`                                                                                                                  | `post_upload.rs::upload` store + `submit_uploaded_path` → `submit_watcher_index`                                                              |
| `UploadIndexed`                                                                                                                     | `tasks/queue/worker.rs` intake application running `workflow::process_file` (index path)                                                      |
| `IntakePickupFileGone`                                                                                                              | `tasks/queue/worker.rs` intake application running `workflow::process_file` (gone-file outcome)                                               |
| `ApiMoveFsOnly` / `DirMoveFsOnly`                                                                                                   | `assign_album.rs` move + `apply_dir_move_plans`                                                                                               |
| `SidecarEditPartial`                                                                                                                | `process/sidecar_edit.rs::commit_metadata_edits`                                                                                              |
| `RotateWriteSidecar` / `RotateFinish`                                                                                               | `router/put/rotate_image.rs` phases                                                                                                           |
| `WorkerApplySuccess` / `WorkerSkipSuperseded` / `WorkerSkipStale` / `WorkerSkipUnchanged` / `WorkerOpenFails` / `WorkerRemoveApply` | `tasks/queue/worker.rs` intake application branches (`IntakeKind` handling in `tasks/queue/intake.rs`)                                        |
| `External*`                                                                                                                         | environment (filesystem mutations outside the process)                                                                                        |
| `ForceModify`                                                                                                                       | environment (filesystem mutation)                                                                                                             |
| `ScanRoot`                                                                                                                          | scan walk entry `tasks/actor/album_index.rs::index_album_with_root`                                                                           |
