---
status: in-progress
type: chore
priority: high
area: backend
---

# Path-Primary Cleanup

The `rework-asset-index` branch replaces hash-primary, multi-alias presentation
with one asset record per physical filesystem path. `asset_id` is the API
identity. `DUPE_INDEX` groups independent assets with the same content hash.
Independent filesystem paths must never be merged into one presented asset.

This cleanup removes or reshapes work that was written for the old alias model,
without changing settled path-primary behavior accidentally.

## Categories

### 1. Plan and documentation alignment

**Status:** complete.

**Decision:** update stale plans and comments to distinguish a physical asset
path from the old multi-alias identity. Preserve historical progress notes, but
mark superseded decisions rather than silently rewriting history.

**Scope:**

- Align `assign-album-conflict` with `asset_id` identity and remove its
  alias-required contract.
- Remove merge implementation work from its remaining actionable checklist;
  retain a short historical note that identity-based merge was rejected.
- Update watcher and release-plan wording from “remaining aliases” to “the
  asset's canonical path” where the implementation is already path-primary.
- Identify backlog plans whose designs still assume `alias: Vec<...>` and mark
  them for rewrite before implementation (`scrub-endpoint` first).

### 2. Assign-album request contract

**Status:** needs focused review before implementation.

**Decision:** likely remove `alias` from `AssignAlbumData` and the frontend
request. `asset_id` already identifies the one physical file to move. Keep
`on_conflict` and the `moved` / `renamedFrom` / `skipped` result contract.

**Scope after approval:**

- Remove `alias` validation and plumbing from backend and frontend.
- Update OpenAPI and all assign callers.
- Replace alias-required and album-alias-rejected scenarios with asset-ID/path
  validation scenarios only where they test actual behavior.
- Verify stale-path behavior through the asset's canonical path, not a caller
  supplied path.

### 3. Test probes and scenario contracts

**Status:** needs focused review before implementation.

**Decision:** test-only probes must expose path-primary terminology and must not
preserve a fake multi-alias API. Duplicate behavior should be asserted using
separate `asset_id` values and, where necessary, the duplicate-group probe.

**Scope after approval:**

- Replace `TestRecordProbe.aliases: Vec<FileModify>` with a singular path/file
  representation, and rename the probe's `hash` field if it is actually an
  asset ID.
- Keep the existing duplicate and asset-ID scenarios as the replacement
  coverage for independent same-hash files.
- Keep behavior tests for moving, deleting, and preserving same-hash assets,
  but express them as independent assets.
- Remove scenarios whose only purpose was multi-alias list ordering, alias
  selection, or alias-count validation.

### 4. Internal alias naming and path helpers

**Status:** defer until categories 1–3 settle.

**Decision:** preserve the current `alias: Option<FileModify>` model field in
the first pass if it is still the storage location for file metadata. Rename
helper functions and comments only when the new name is unambiguous and the
change has no API consequence. A full field rename is a separate migration,
not incidental cleanup.

**Candidate scope:** `process/alias.rs`, `prune_alias_paths`,
`prune_stale_aliases`, `normalize_alias_path`, `sweep_stale_aliases`,
`trim_aliases_to_path`, and stale “remaining aliases” comments.

### 5. Merge and duplicate semantics

**Status:** mostly complete; audit only.

**Decision:** do not reintroduce or emulate identity-based merge. Keep
`DeduplicateTask` only as the path-primary indexing step that updates
`DUPE_INDEX`; it must never collapse physical assets. Keep shared-thumbnail
reference behavior and duplicate-preservation tests.

**Scope:**

- Confirm no `OnConflict::Merge`, merge-dedup upload path, recursive directory
  merge, or `DeduplicatedRemoved` code remains.
- Remove stale merge action items from plans and generated references.
- Do not rename ordinary Rust/frontend metadata merge helpers; those are not
  duplicate-identity merge behavior.

## Execution Order

1. Align plans and comments; record superseded alias/G1/merge decisions. **Done.**
2. Review and change the assign request contract.
3. Reshape test probes and rename/consolidate scenarios.
4. Rename internal path helpers after their callers and test contracts settle.
5. Run the full alias/duplicate scenario subset, then the normal checks.

## Progress

- 2026-09-23: Created after reviewing the 74 commits on `rework-asset-index`.
  Path-primary identity and `DUPE_INDEX` are treated as settled; assign API,
  probe shape, and internal alias naming remain separate decisions.
- 2026-09-23: Completed direct alignment of the active/open lifecycle and
  assign plans with path-primary asset identity. Remaining work is the assign
  request contract, test probes, and internal helper naming.
