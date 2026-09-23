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

**Status:** complete.

**Decision:** remove `alias` from `AssignAlbumData` and the frontend
request. `asset_id` already identifies the one physical file to move. Keep
`on_conflict` and the `moved` / `renamedFrom` / `skipped` result contract.
Reject unknown request fields (`deny_unknown_fields`); a legacy body that
still sends `alias` is a 400, not a silent ignore — backward compatibility
with the multi-alias shape is not a goal.

**Scope:**

- Remove `alias` validation and plumbing from backend and frontend.
- Update OpenAPI and all assign callers.
- Alias-required and album-alias-rejected scenarios are obsolete under the
  path-primary contract and have been removed from request bodies; no
  replacement scenarios are needed for a field that no longer exists.
- Verify stale-path behavior through the asset's canonical path, not a caller
  supplied path.

### 3. Test probes and scenario contracts

**Status:** complete.

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

**Status:** complete.

**Decision:** preserve the current `alias: Option<FileModify>` model field in
the first pass if it is still the storage location for file metadata. Rename
helper functions and comments only when the new name is unambiguous and the
change has no API consequence. A full field rename is a separate migration,
not incidental cleanup.

**Candidate scope:** `process/path.rs`, `normalize_asset_path`,
`prune_asset_path`, `prune_stale_asset_path`, `sweep_stale_asset_paths`,
`align_path_to_asset`, and stale “remaining aliases” comments. These are the
current names; they replace the previous `process/alias.rs`,
`normalize_alias_path`, `prune_alias_paths`, `prune_stale_aliases`,
`sweep_stale_aliases`, and `trim_aliases_to_path` (full mapping in the Result
note below).

**Result (2026-09-23):** renamed to path/asset terminology. `process/alias.rs`
is now `process/path.rs`, hosting `normalize_asset_path`, `prune_asset_path`,
`prune_stale_asset_path`, and the private `remove_asset_file`;
`sweep_stale_aliases` → `sweep_stale_asset_paths` (`album_index.rs`);
`trim_aliases_to_path` → `align_path_to_asset` (`update_tree.rs`). Stale
“remaining aliases” / “alias path” comments and the index-task error string
now use canonical-path wording. The `AbstractData.alias` storage field, the
`alias()` / `alias_mut()` accessors, and external response fields are
unchanged; `remove_compressed_thumbnail` keeps its DUPE_INDEX/shared-thumbnail
semantics.

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
2. Review and change the assign request contract. **Done.**
3. Reshape test probes and rename/consolidate scenarios. **Done.**
4. Rename internal path helpers after their callers and test contracts settle. **Done.**
5. Run the full alias/duplicate scenario subset, then the normal checks. **Done.**

## Progress

- 2026-09-23: Created after reviewing the 74 commits on `rework-asset-index`.
  Path-primary identity and `DUPE_INDEX` are treated as settled; assign API,
  probe shape, and internal alias naming remain separate decisions.
- 2026-09-23: Completed direct alignment of the active/open lifecycle and
  assign plans with path-primary asset identity. Remaining work is the assign
  request contract, test probes, and internal helper naming.
- 2026-09-23: Category 2 done. `alias` removed from `AssignAlbumData`,
  `move_asset_into_album`/`move_album_into_album`, and the frontend
  `assignAlbum` body; asset moves resolve `canonical_path` via `ASSET_BY_ID`.
  Contract unit tests pin the OpenAPI schema (no `alias`; required =
  `assetId`/`albumId`/`onConflict`). Scenario request bodies stripped of
  `alias`; stale-path scenario keeps asserting non-200 via the canonical path.
  OpenAPI reference regenerated. Categories 3–4 untouched.
- 2026-09-23: Review follow-up: `AssignAlbumData` now uses
  `deny_unknown_fields`, so a legacy body carrying `alias` fails
  deserialization (unit-tested) instead of being silently ignored.
  Category 2 scope no longer asks to replace alias-required /
  album-alias-rejected scenarios — those cases are obsolete and removed.
- 2026-09-23: Test probes and scenario contracts done. `TestRecordProbe` is
  now `{ assetId, path: Option<FileModify> }` (mirrors `AbstractData::alias()`),
  replacing `hash` + `aliases: Vec<FileModify>`; response description reworded.
  Probe contract unit tests pin the OpenAPI schema and JSON wire shape;
  `assign_self_move_noop` asserts `path.file`; the harness selftest was renamed
  to `test_probe_catches_wrong_path`. No alias-ordering/selection/count
  scenarios remained to remove; duplicate coverage stays on separate asset IDs
  plus the DUPE_INDEX group probe. OpenAPI reference regenerated. Category 4
  (internal helper renaming) untouched.
- 2026-09-23: Category 4 (internal alias naming) done. `process/alias.rs` →
  `process/path.rs` (module `path`) with `normalize_asset_path`,
  `prune_asset_path`, `prune_stale_asset_path`, private `remove_asset_file`;
  callers renamed `sweep_stale_aliases` → `sweep_stale_asset_paths` and
  `trim_aliases_to_path` → `align_path_to_asset`. Stale “remaining
  aliases”/“alias path” comments, debug logs, and the index-task error string
  reworded to canonical-path terminology; field/wire references to
  `AbstractData.alias` intentionally left as-is. `remove_compressed_thumbnail`
  DUPE_INDEX/shared-thumbnail behavior unchanged. Symbol references updated in
  `album-index-sweep-concurrency`, `delete-from-disk`, and
  `path-primary-asset-execution`. Verification: `just check` and full
  `just test` green (backend 265 unit + 3 integration tests, 34 Playwright
  scenarios including the stale-path/duplicate subset, utils + frontend
  vitest).
