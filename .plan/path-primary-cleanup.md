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

### 4. Internal naming and path helpers

**Status:** complete.

**Decision:** the model/wire field is `path: Option<FileModify>` on
`ImageMetadata`/`VideoMetadata`, exposed as `AbstractData::path()` /
`path_mut()` and serialized as `abstractData.path`. The earlier first-pass
deferral of the field rename is superseded; only genuine alias meanings
(serde `alias` attributes, resolve-alias configs, legacy field-name strings
in rejection tests) remain under the old word.

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

### 6. Metadata table value slimming (`FileModify` deduplication)

**Status:** open — identified 2026-09-23 as a missing Phase 14 migration
artifact.

**Decision:** `METADATA_TABLE` is correctly keyed by `asset_id`, but its value
is still the full legacy `AbstractData` record instead of a metadata-only
struct. Phase 14's own target layout defines the value as “tags, description,
rating, EXIF vec, cover ref”; the implementation reused `AbstractData` (the
plan's parenthesized shortcut) to avoid rewriting every consumer. As a result
four identity fields are stored twice: `FileModify { file, modified,
scan_time, is_trashed }` in the metadata row duplicates
`AssetRecord { canonical_path, modified, scan_time, is_trashed }` in
`ASSET_BY_ID`, and `flush_tree` copies metadata-row fields _back_ into the
identity tables on every metadata edit. The metadata row must stop being a
write-side source of identity.

**Scope:**

- Extract a metadata-only value type (tags, description, rating, favorite,
  archived, `exif_vec`, `phash`, album title/cover, `update_at`, `pending`) as
  the `METADATA_TABLE` value; keep the `asset_id` key.
- Edit endpoints read `AssetRecord` + metadata row in one transaction, mutate
  metadata only, and write identity fields exclusively from `AssetRecord` —
  removing the `FileModify` mirror-back in `flush_tree` and the drift-repair
  role of `align_path_to_asset`.
- `GET /get/metadata/{asset_id}` composes `AssetRecord` + metadata at the edge
  so the wire response shape stays stable.
- Demote `FileModify` to a view type assembled from `AssetRecord` (or remove it
  from storage entirely); rename it for its role if it remains on the wire.
- Consumers to convert: edit endpoints, `write_sidecar_for`,
  `clear_abstract_data_metadata`, expression filters over the in-memory TREE,
  `get_metadata`, and scenario assertions on `path.*`.
- Risks: dual-row write atomicity; trash-flag ownership (media currently on the
  file entry, albums at record level — pick `AssetRecord.is_trashed` as sole
  owner); filters that currently scan full `AbstractData` in the TREE.
- A1 — remove `align_path_to_asset` (`update_tree.rs`): it only repairs drift
  between the metadata row's `path` and `AssetRecord.canonical_path` and dies
  when identity writes come from `AssetRecord` alone.
- A2 — delete the `to_update`/`path_before` guard branch in
  `sweep_stale_asset_paths` (`album_index.rs`): unreachable under the
  single-path model, and its stated fear (“re-flushing an unchanged clone would
  overwrite fresher writes”) is a dual-write symptom.
- A3 — drop `FileModify`'s `scan_time`-only `Eq`/`Ord`/`Hash` impls
  (`response.rs`): multi-alias `Vec` de-duplication residue with actively
  misleading semantics; goes away when the struct shrinks to a view type.
- A4 — collapse the three identifiers per metadata row: `ObjectSchema.id` and
  `ImageMetadata.id` both hold `display_id` and both flatten into the wire JSON
  under the name `id`, and the row is additionally keyed by `asset_id`; decide
  the single identity (key = `asset_id`) and stop storing content-hash-as-id
  inside the metadata store.

### 7. Worker payload and token identity naming (B2, B3)

**Status:** open — required review/decision before implementation.

**Decision pending:** two coupled naming questions that must not be half-done.

- **B2 — the `hash` field on `ProcessImagePayload`/`ProcessSmallImagePayload`
  carries two different values:** content hash for media rows
  (`abstractData.id` = `display_id`) but the cover's `asset_id` on the album
  refresh path (`refreshAlbumMetadata`). Required review: settle the correct
  value via category 8 first, then either rename the field to `servingId` or
  split it into `contentHash`/`coverAssetId`. Do not rename blind.
- **B3 — `hashToken` vs `assetToken`:** the store and IndexedDB are
  `assetToken`, but worker payloads (`workerApi`, `toDataWorker`, `toImgWorker`,
  `types`, call sites) and the backend route `/post/renew-hash-token` with
  `expiredHashToken` still say hash. Required decision: rename end-to-end
  including the backend route (OpenAPI/wire churn) **or** keep `hashToken`
  deliberately because the JWT's claim is a GuardHash content-hash claim and
  document that rationale. Either is acceptable; a split naming is not.

**Dependency:** B2 is blocked on category 8; B3 is independent.

### 8. Cover-serving identity verification (C1)

**Status:** open — verification required; possible latent bug.

Initial gallery tiles fetch album covers with `coverHash` (content hash), pinned
by the getter test “album cover thumbnail URL uses content hash, not
asset_id”. The album metadata **refresh** path sends `hash: data.cover` into
the img-worker, whose compressed-URL builder does
`getSrc(event.hash, original=false)` → `/object/compressed/…/{data.cover}.jpg`.
If `data.cover` is the cover asset's `asset_id` (as established earlier), that
URL is built from an asset_id where a content hash is expected.

**Required review before any B2 rename:**

- Read the backend `/object/compressed` resolver: does it accept an asset_id
  fallback, or does it require the content hash?
- If asset_id is accepted, document the fallback; if not, add an E2E scenario
  covering album cover display after a metadata refresh (likely failing today)
  and fix the caller to send `coverHash`.
- Outcome decides the correct value for B2's `hash`/`servingId` field.

### 9. Minor terminology debt (D)

**Status:** backlog — low priority; no blocking decision.

- `DB_VERSION = 2` on the freshly renamed `assetToken` IndexedDB: works (fresh
  create runs the upgrade path 0→2) but could reset to 1 since no legacy DB
  exists under the new name. Cosmetic; no migration concern.
- `expression.rs:463` test doc still explains behavior by reference to the
  “old empty-vec” multi-entry model (historical-keep today; may be simplified
  once no reader remembers the old model).
- `ReducedData.hash` (content hash beside `asset_id`) is intentional for
  compressed URLs — documented, no action.

## Execution Order

1. Align plans and comments; record superseded alias/G1/merge decisions. **Done.**
2. Review and change the assign request contract. **Done.**
3. Reshape test probes and rename/consolidate scenarios. **Done.**
4. Rename internal path helpers after their callers and test contracts settle. **Done.**
5. Run the full alias/duplicate scenario subset, then the normal checks. **Done.**
6. Slim the metadata-table value and deduplicate identity fields out of
   `FileModify` (category 6, including A1–A4).
7. Verify cover-serving identity end to end (category 8) — required before B2.
8. Review and apply worker/token payload naming (category 7: B2 after 7, B3
   whenever).
9. Optional minor sweep (category 9).

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
- 2026-09-23: Model/wire field rename done (category 4 decision updated).
  `AbstractData.alias` storage field → `path`, accessors `alias()` /
  `alias_mut()` → `path()` / `path_mut()`, wire field `abstractData.alias`
  → `abstractData.path`, frontend `AliasSchema`/`Alias` →
  `FileModifySchema`/`FileModify`. Scenario assertions, doc comments, and
  docs reworded; genuine alias meanings (serde attributes, resolve-alias
  configs, legacy rejection-test strings) kept. OpenAPI reference
  regenerated.
- 2026-09-23: Added category 6. `METADATA_TABLE` stores the full legacy
  `AbstractData` value instead of the metadata-only payload its Phase 14 target
  specifies, duplicating path/timestamps/trash between the metadata row and
  `AssetRecord`. `flush_tree` mirrors those fields from the metadata row into
  the identity tables on write; the fix inverts ownership (identity from
  `AssetRecord` only) and slims the table value.
- 2026-09-23: Diff review folded A1–A4 into category 6 scope (drift-repair
  helper, dead sweep guard branch, scan_time-only `Eq`/`Ord`/`Hash` on
  `FileModify`, triple-id metadata rows). Added categories 7 (worker/token
  payload naming — decision required), 8 (cover-serving identity — verification
  required, possible latent bug), and 9 (minor debt). Applied B1 locally:
  `refreshAlbumMetadata` `coverHash` → `coverAssetId` (the value is the cover
  asset's `asset_id`, not a content hash).
