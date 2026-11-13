# utils/quint

Checkable models of subsystem protocols, one file per subsystem:
`utils/quint/pipeline/<subsystem>.qnt`. Each spec is exercised by `just quint-check`
(typecheck plus `quint test`); exploratory simulation is `quint run`
and never gates.

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

- `eventual-consistency.qnt` — the pipeline protocol as prescribed
  event handling: watcher/API boundary events, recovery events
  (watcher gap, crash, derived-store loss, DB loss), intake submits,
  worker steps, and per-root scan/sweep. Invariants: no lost intake
  work, no permanent stale rows; the convergence goal is `Converged`
  (every processable asset path has exactly one correct row; every
  row's asset exists; nothing queued), pinned per diverged shape by
  the runs. Unprocessable paths are logged, never indexed, and are
  excluded from the goal.

  The spec is split across modules that import each other (native
  Quint modules, no preprocessor): `eventual-consistency-state.qnt` holds shared
  types, variables, finite bounds (five paths, two roots), and the
  initial state; `eventual-consistency-assumptions.qnt` holds the snapshot matcher
  plus the `NoLostUpload` / `NoPermanentStale` safety predicates,
  each beside its prose and ASS tag;
  `eventual-consistency-goal.qnt` holds the `Converged` goal
  predicate — the subsystem-specific part;
  `eventual-consistency.qnt` holds the actions, the `step`
  interleaving, and the runs.

- `asset-aspects.qnt` — the four-aspect slice (asset file, sidecar,
  thumbnail/derived, DB asset row): alignment predicates and
  conflicting-pair runs over the same state; imports
  `eventual-consistency.qnt`.
- `operations.json` — internal mutation operations OP-1..OP-12 with
  events, aspect reads/writes, guards, status and interaction nodes;
  validated by `just operations-check` against backend annotations.
  Out of scope by design: the read-model invalidation advance
  (`mark_tree_dirty_background`) and the dir-album sweep lane
  (`handle_sweep_dir_albums`); tree-epoch modeling is deferred
  (node N5 / the full aspect boundary).- `events.json` — boundary and recovery events with their settled
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
  `events.json` notes.

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

## Tooling

- `specs-check.mjs` — closed-world tag audit and the generated
  `pipeline/traceability.json` map.
- `events-check.mjs`, `catalog-check.mjs` — catalog validation against
  `pipeline/catalog.schema.json`.
- `operations-check.mjs` — validates `pipeline/operations.json` and the
  `#[quint::operation(...)]` / `#[quint::event(...)]`
  markers in `backend/src/**/*.rs`.
- `gen-eventual-consistency-doc.mjs` — writes `docs/eventual-consistency.md`.

## Operations and annotations

`pipeline/operations.json` tracks the implementation of the four-aspect slice
(asset file, sidecar, thumbnail/derived, DB asset row) as operations OP-1..OP-12.
Each operation records its events, aspect reads/writes, guards, status and
interaction nodes. Backend functions carry attribute markers from the `quint`
crate (`#[quint::operation("OP-N")]`,
`#[quint::event("EVENT_NAME")]`). Compile-time validation (rung 0 of the
annotation crate) rejects unknown or misspelled operation/event ids at
`cargo check -p picasu`, so a typo'd or renamed OP id fails the build
instead of surfacing only in `just check`. `operations-check.mjs` remains the
catalog-side backstop. What is still NOT enforced until rung 1: a marker must
sit on a real item (the text scan accepts comment, doc-comment, string or
`#[cfg(test)]` mentions), a marker naming a wrong-but-valid id still
compiles, and aspect/order drift between model and implementation is not
checked.

The Quint toolchain installer stays in `scripts/deps/quint.sh` with the
shared pins in `scripts/deps/versions.env`.

Modeling scope (not assumptions, no formal counterpart required):
no cross-root identity in v1 — moves across watched-root
boundaries are observed as creation plus ghost and deletion; walk
roots are disjoint from the derived store, so pipeline writes
cannot resubmit themselves.

## Model-to-code terminology

Model terms map to named code entities; identifiers are stable — anchors are
documented rather than renamed to avoid drift.

| Model identifier                                                                                                                    | Code anchor                                                                                                                                   |
| ----------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `files`                                                                                                                             | filesystem under the data roots (media bytes + sidecar bytes)                                                                                 |
| `generations`                                                                                                                       | `tasks/path_epoch.rs` `GENERATIONS` via `next_generation`/`current_generation`                                                                |
| `intake`                                                                                                                            | `IntakeItem` set, `tasks/queue/intake.rs`                                                                                                     |
| `rows`                                                                                                                              | `AssetRecord` in `storage/asset_store.rs` `ASSET_BY_ID`/`ASSET_BY_PATH`                                                                       |
| `sweepBatch`                                                                                                                        | `sweep_stale_asset_paths` survivors → `WriteOp::SweepRemoveBatch`                                                                             |
| `leasedPaths`                                                                                                                       | `EXACT_LEASES`/`SUBTREE_LEASES` in `tasks/path_epoch.rs`                                                                                      |
| `sweepPending`                                                                                                                      | per-root sweep pending flag set by `ScanRoot`/the scan job in `tasks/actor/album_index.rs`                                                    |
| `nextAssetId`                                                                                                                       | asset-id allocation via `generate_random_hash` in `model/asset.rs` (`AssetRecord::new_media`/`new_album`) and `flush_tree.rs`                 |
| `unprocessable`                                                                                                                     | model-only; ingest failures are logged via `note_file_failed`/`handle_error`, not a stored set                                                |
| `SweepEnumerate`                                                                                                                    | enumeration loop of `tasks/actor/album_index.rs::sweep_stale_asset_paths`                                                                     |
| `SweepRecheck`                                                                                                                      | inline re-check loop of `sweep_stale_asset_paths` (generation, `is_busy`, `path.exists`, pre-submit)                                          |
| `SweepWriterApply`                                                                                                                  | `flush_tree.rs::sweep_remove_batch`, submitted as `WriteOp::SweepRemoveBatch` via `tasks/queue/writer.rs::submit`; exists-skip + resolve only |
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
| `SweepRoot`                                                                                                                         | sweep trigger `tasks/actor/album_index.rs::sweep_stale_asset_paths` (via the scan job)                                                        |
