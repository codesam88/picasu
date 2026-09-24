---
status: done
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
  asset's path” where the implementation is already path-primary.
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
- Verify stale-path behavior through the asset's path, not a caller
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
now use path wording. The `AbstractData.alias` storage field, the
`alias()` / `alias_mut()` accessors, and external response fields are
unchanged; `remove_compressed_thumbnail` keeps its DUPE_INDEX/shared-thumbnail
semantics.

### 5. Merge and duplicate semantics

**Status:** complete.

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

**Status:** complete.

**Decision:** `METADATA_TABLE` is correctly keyed by `asset_id`, but its value
is still the full legacy `AbstractData` record instead of a metadata-only
struct. Phase 14's own target layout defines the value as “tags, description,
rating, EXIF vec, cover ref”; the implementation reused `AbstractData` (the
plan's parenthesized shortcut) to avoid rewriting every consumer. As a result
four identity fields are stored twice: `FileModify { file, modified,
scan_time, is_trashed }` in the metadata row duplicates
`AssetRecord { path, modified, scan_time, is_trashed }` in
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
  between the metadata row's `path` and `AssetRecord.path` and dies
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

**Status:** complete.

- **B2 — resolved with category 8.** The `hash` field's contract is the content
  hash (compressed-URL segment + GuardHash claim); `assetId` is identity
  (blob-cache key + token-store key). The album callers that sent an asset_id
  into the `hash` slot were fixed under category 8 via `coverServingIds`. Field
  names stand; no rename or split.
- **B3 — withdrawn (2026-09-24).** Per design decision: content hash
  deliberately stays in compressed URLs and the serving-token JWT —
  `GuardHash` validates the URL segment against the token's `hash` claim, and
  content-addressed thumbnails enable skipping regeneration for known hashes.
  Each name is accurate for what it names: `hashToken` / `renew-hash-token` /
  `ClaimsHash` = the hash-bound JWT type; the `assetToken` store/map = storage
  keyed by `asset_id`. Not a half-finished rename; no code change.
- Root-cause note retained for the hardening ticket: `build.rs` never scans
  `router/auth.rs`, so the renew endpoints never enter the generated spec
  (tracked as task 1 in `openapi-contract-hardening.md`).

### 8. Cover-serving identity verification (C1)

**Status:** complete — verified real, fixed.

Confirmed defect chain: compressed files are content-addressed on disk and
`GuardHash` compares the URL segment to the token's `hash` claim, so any
request built from an asset_id fails. Investigation found three distinct bugs
in the set-cover flow:

1. `ItemSetAsCover` read `route.params.assetId` on an album route whose param is
   `albumId` — silent early return, so PUT never fired and no toast ever
   appeared (the user-visible root cause).
2. `refreshAlbumMetadata` depended on the album's own row being in `dataStore`,
   which never happens on the album's contents page (the filter returns its
   media and child albums, not itself) — the watch/toast chain was dead on its
   only call path. The function was deleted; the toast now fires in
   `ItemSetAsCover` immediately after a successful PUT.
3. The remaining live fetch sites sent mixed values into the `hash` slot.
   `coverServingIds(cover, coverHash)` in `getter.ts` pins the contract
   (`hash` = content hash, `assetId` = cover asset id, `null` when either is
   missing): used by `SmallImageContainer` (whose `?? cover` fallback that sent
   an asset_id was removed) and `Display.vue`'s album branch.

Also in scope: `ItemSetAsCover`'s `v-list-item` gained `value="set-as-cover"`
so it exposes `role=option` like its menu siblings (scenario clickability);
`ReducedData.hash` got its missing doc comment (D3).

Tests: `coverServingIds` unit contract (RED→GREEN); new Playwright scenario
`set-cover-updates-album-metadata` (toast + integrity — cover-image identity is
not DSL-observable, so the unit test is the hash-contract guard); full suite
green: backend 265, vitest 70, Playwright 35/35.

### 9. Minor terminology debt (D)

**Status:** complete.

- D1 `DB_VERSION` stays at 2 — dismissed: opening an existing `assetToken` DB
  (created at version 2) with version 1 raises IndexedDB `VersionError`.
- D2 `expression.rs` empty-vec comment — already done in `f2391520`.
- D3 `ReducedData.hash` — missing doc comment added alongside category 8
  (content hash: compressed-thumbnail URLs and the serving-token `hash`
  claim).

## Execution Order

1. Align plans and comments; record superseded alias/G1/merge decisions. **Done.**
2. Review and change the assign request contract. **Done.**
3. Reshape test probes and rename/consolidate scenarios. **Done.**
4. Rename internal path helpers after their callers and test contracts settle. **Done.**
5. Run the full alias/duplicate scenario subset, then the normal checks. **Done.**
6. Slim the metadata-table value and deduplicate identity fields out of
   `FileModify` (category 6, including A1–A4). **Done.**
7. Fix cover-serving identity (category 8) — B2 callers ride along. **Done.**
8. B3 asset-token rename — **Withdrawn** (content hash intentionally stays in
   URLs and JWT claims; see category 7).
9. Category 9 one-liner (`ReducedData.hash` doc). **Done.**

## Progress

- 2026-09-23: Created after reviewing the 74 commits on `rework-asset-index`.
  Path-primary identity and `DUPE_INDEX` are treated as settled; assign API,
  probe shape, and internal alias naming remain separate decisions.
- 2026-09-23: Completed direct alignment of the active/open lifecycle and
  assign plans with path-primary asset identity. Remaining work is the assign
  request contract, test probes, and internal helper naming.
- 2026-09-23: Category 2 done. `alias` removed from `AssignAlbumData`,
  `move_asset_into_album`/`move_album_into_album`, and the frontend
  `assignAlbum` body; asset moves resolve `path` via `ASSET_BY_ID`.
  Contract unit tests pin the OpenAPI schema (no `alias`; required =
  `assetId`/`albumId`/`onConflict`). Scenario request bodies stripped of
  `alias`; stale-path scenario keeps asserting non-200 via the path.
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
  reworded to path terminology; field/wire references to
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
- 2026-09-23: Category 6 done. `METADATA_TABLE` now stores `MetadataRecord`
  (metadata-only payload: object tags/description/rating/flags/update_at/
  pending/thumbhash + image phash/exif/dimensions + video duration + album
  title/times/cover/stats/share_list/custom_title) under the new on-disk
  table name `asset_metadata` — rows written under the previous `metadata`
  name are intentionally orphaned (clean rebuild repopulates; no migration).
  Write paths split: `FlushTreeTask`/`flush_tables` remains the
  index/file-mutation flush (derives `AssetRecord`, stores
  `to_metadata_record(...)`); metadata edits (edit_tag/edit_rating/
  edit_description/edit_flags) now go through `store_metadata_record` (one
  write txn, optional `AssetRecord.is_trashed` update) and dispatch only
  `UpdateTreeTask`; edit_album/edit_share/create_share/album-self-update/
  rebuild-sync/dir-album creation insert payloads. Read paths compose via
  `compose_abstract_data(record, payload)` (get_metadata 404s on missing
  record; TREE, delete, probe, export, shares, read_albums all compose).
  Trash ownership settled on `AssetRecord.is_trashed`; composition projects
  it onto the wire (media file entry, album metadata). A1 removed
  `align_path_to_asset`; A2 removed the dead `to_update`/`path_before`
  sweep branch; A3 dropped `FileModify`'s scan_time-only
  `Eq`/`Ord`/`Hash` (the one `.max()` user in `index.rs` now reads the
  single entry directly) and renamed the type to `FileEntry`
  (backend + frontend `FileEntrySchema`/`FileEntry`, OpenAPI regenerated);
  A4 removed `ImageMetadata.id`/`VideoMetadata.id` and stopped storing
  album `metadata.id` (composition fills it from `asset_id`), so stored
  payloads carry zero `id` fields. `AbstractData` bitcode `SCHEMA_VERSION`
  left at 1 after verifying no snapshot store embeds `AbstractData`
  (snapshots hold `ReducedData`/`Prefetch`); `MetadataRecord` gets its own
  versioned `Value` impl. Startup now pre-creates all four store tables so
  fresh DATA_HOMEs (Playwright) don't hit `TableDoesNotExist`. Wire
  unchanged: `just check` and full `just test` green (backend 265 lib
  tests incl. 100 scenarios + integration, utils 24, vitest 67,
  Playwright 34/34), including the `metadata_only_loaded_on_detail` /
  `metadata_detail_returns_full_metadata` / `abstractData.path.*` wire
  guards and two new stored-payload identity-key tests.
- 2026-09-23: Renamed `AssetRecord.canonical_path` → `path` (review: the
  “canonical” adjective only distinguished multi-form paths, which no longer
  exist; one field covers files and album directories, so plain `path` matches
  the glossary). Stored `ASSET_BY_ID` JSON key changed (`canonicalPath` →
  `path`); no migration — the metadata-table rename already mandates a clean
  rebuild. Prose, docs, scenarios, OpenAPI, and this plan updated;
  `canonicalize_path` and `std::fs::canonicalize` kept (correct verb for
  normalization). Also removed the last old-model reference in active code
  (`expression.rs` empty-vec comment).
- 2026-09-23: OpenAPI rework-adjacent sweep (audit findings tied to this
  branch): test-only probes stripped from the public spec — `--dump-openapi`
  now serves `openapi_public::public_json()` which removes `/get/test/*` and
  the probe schemas, while probe contract tests keep the full generated spec;
  `PUT /put/assign_album` given tag `albums`, single-line summary, and explicit
  description (fixes its multi-line anchors in the reference); `OnConflict`
  enum and `AssignAlbumData.albumId` documented. Reference regenerated;
  remaining generic findings stay in `openapi-contract-hardening.md`.
  Verification: `just check` + full `just test` green (backend 268 incl. the
  new spec-contract tests, utils 24, vitest 67, Playwright 34/34).
- 2026-09-23: Categories 7–9 reviewed against code. C1 confirmed as a real
  bug end-to-end (set-cover refresh requests `/object/compressed/{asset_id}`
  while compressed files are content-hash-named and GuardHash validates the
  content-hash claim); B2 resolved as a value bug in that same path, not a
  field-name problem; B3 decided for a full asset-token rename (JWT claims
  unchanged); renew-endpoint spec omission root-caused to `build.rs` not
  scanning `router/auth.rs`; category 9: D2 already done, D1 dismissed
  (`VersionError` risk), D3 confirmed as an undocumented field. Sections 7–9
  rewritten with verdicts and scopes; implementation pending.
- 2026-09-24: Categories 7–9 closed. B3 withdrawn after the JWT/GuardHash
  review — content hash intentionally stays in compressed URLs and the
  serving-token claims (GuardHash binding; skip-thumbnail-for-known-hash
  future); `hashToken` vs `assetToken` documented as JWT-type vs storage-key,
  both accurate. C1 fixed: `ItemSetAsCover` route-param bug (`assetId` →
  `albumId`), dead `refreshAlbumMetadata` deleted with toast moved after PUT,
  `coverServingIds` contract applied at the live fetch sites, menu-item
  `value` attr, new e2e scenario; D3 doc added. Full `just check` + `just test`
  green (backend 265, vitest 70, Playwright 35/35). All categories 1–9
  complete — cleanup finished.
