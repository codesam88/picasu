# Database

Picasu uses [redb](https://github.com/cberner/redb) (v4), an embedded
key-value store, for all persistence. There is no SQL or external database
server. The data directory (see `get_data_path()` in
`backend/src/storage/files.rs`) contains one persistent DB and three
disposable cache DBs.

---

## Database files

| File                | Purpose                                  | Persistence        |
| ------------------- | ---------------------------------------- | ------------------ |
| `db/index_v5.redb`  | Store of record for all media/album data | Persistent         |
| `db/temp_db.redb`   | Tree snapshot cache (flat sorted view)   | Deleted at startup |
| `db/cache_db.redb`  | Query prefetch cache                     | Deleted at startup |
| `db/expire_db.redb` | Snapshot expiration timestamps           | Deleted at startup |

The three cache databases are intentionally ephemeral — they are deleted
during startup (`initialize_file()` in `backend/src/init.rs`) and rebuilt on
demand. Only `index_v5.redb` carries data that requires backup.

---

## index_v5.redb — store of record

Identity is path-primary: every lookup keys on `asset_id`. Content hash is
used only for duplicate grouping (`DUPE_INDEX`) and compressed-thumbnail
serving — never as record identity.

redb table definitions live in `backend/src/storage/db.rs`:

| Constant         | On-disk name      | Key → value                                | Role                                                                                                                                               |
| ---------------- | ----------------- | ------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `METADATA_TABLE` | `"metadata"`      | `asset_id` → `AbstractData`                | Fat per-asset record. Read for detail views, metadata edits, and `TREE` construction; lean list rows come from snapshots instead of per-row reads. |
| `ASSET_BY_PATH`  | `"asset_by_path"` | canonical filesystem path → `asset_id`     | One row per physical file or directory. Enforces path uniqueness.                                                                                  |
| `ASSET_BY_ID`    | `"asset_by_id"`   | `asset_id` → JSON-serialized `AssetRecord` | One row per asset: kind, canonical path, content hash, size, album membership, trash flag.                                                         |
| `DUPE_INDEX`     | `"dupe_index"`    | `content_hash` → JSON `Vec<asset_id>`      | Dedup grouping only: asset IDs sharing identical content remain independently addressable. Albums never appear here.                               |

### AbstractData variants

```rust
pub enum AbstractData {
    Image(ImageCombined),
    Video(VideoCombined),
    Album(AlbumCombined),
}
```

Every variant shares a common `ObjectSchema` plus a type-specific metadata
struct.

#### ObjectSchema (common to all variants)

Source: `backend/src/model/object.rs`

| Field         | Type              | Description                            |
| ------------- | ----------------- | -------------------------------------- |
| `id`          | `ArrayString<64>` | Content hash (media) or album asset ID |
| `obj_type`    | `ObjectType`      | `Image`, `Video`, or `Album`           |
| `pending`     | `bool`            | Processing-in-progress flag            |
| `thumbhash`   | `Option<Vec<u8>>` | Binary thumbnail hash                  |
| `description` | `Option<String>`  | User-written description               |
| `tags`        | `HashSet<String>` | User-applied tags                      |
| `is_favorite` | `bool`            | Favorited flag                         |
| `is_archived` | `bool`            | Archived flag                          |
| `rating`      | `Option<u8>`      | Star rating, if set                    |
| `update_at`   | `i64`             | Last-updated timestamp (ms)            |

#### ImageMetadata

Source: `backend/src/model/image.rs`

| Field      | Type                       | Description                   |
| ---------- | -------------------------- | ----------------------------- |
| `id`       | `ArrayString<64>`          | Content hash (`object.id`)    |
| `size`     | `u64`                      | File size in bytes            |
| `width`    | `u32`                      | Pixel width                   |
| `height`   | `u32`                      | Pixel height                  |
| `ext`      | `String`                   | File extension (e.g. `"jpg"`) |
| `phash`    | `Option<Vec<u8>>`          | Perceptual hash               |
| `album`    | `Option<ArrayString<64>>`  | Single album membership       |
| `exif_vec` | `BTreeMap<String, String>` | EXIF key-value pairs          |
| `alias`    | `Vec<FileModify>`          | Known file paths & timestamps |

#### VideoMetadata

Source: `backend/src/model/video.rs`

| Field      | Type                       | Description                   |
| ---------- | -------------------------- | ----------------------------- |
| `id`       | `ArrayString<64>`          | Content hash (`object.id`)    |
| `size`     | `u64`                      | File size in bytes            |
| `width`    | `u32`                      | Pixel width                   |
| `height`   | `u32`                      | Pixel height                  |
| `ext`      | `String`                   | File extension (e.g. `"mp4"`) |
| `duration` | `f64`                      | Video duration in seconds     |
| `album`    | `Option<ArrayString<64>>`  | Single album membership       |
| `exif_vec` | `BTreeMap<String, String>` | EXIF key-value pairs          |
| `alias`    | `Vec<FileModify>`          | Known file paths & timestamps |

#### AlbumMetadata

Source: `backend/src/model/album.rs`

| Field                | Type                              | Description                                                       |
| -------------------- | --------------------------------- | ----------------------------------------------------------------- |
| `id`                 | `ArrayString<64>`                 | Album asset ID (`object.id`)                                      |
| `title`              | `Option<String>`                  | Display title (directory-name default unless customized)          |
| `created_time`       | `i64`                             | Creation timestamp (ms)                                           |
| `start_time`         | `Option<i64>`                     | Earliest media timestamp (ms)                                     |
| `end_time`           | `Option<i64>`                     | Latest media timestamp (ms)                                       |
| `last_modified_time` | `i64`                             | Last metadata update (ms)                                         |
| `cover`              | `Option<ArrayString<64>>`         | Cover image asset ID                                              |
| `item_count`         | `usize`                           | Number of member media items                                      |
| `item_size`          | `u64`                             | Total member file size                                            |
| `share_list`         | `HashMap<ArrayString<64>, Share>` | Named share configurations                                        |
| `dir_path`           | `String`                          | Filesystem path of the album's directory (required)               |
| `custom_title`       | `Option<String>`                  | User-set title override; `None` = derived from the directory name |
| `is_trashed`         | `bool`                            | Record-level trash flag (albums have no per-path alias)           |

Every album is directory-backed: `dir_path` is the album directory's path, and
a media file belongs to the album whose `dir_path` matches the file's
immediate parent directory. Membership is recorded on each media item's
`album` field at index time.

#### FileModify

Source: `backend/src/model/response.rs`

| Field        | Type     | Description                                            |
| ------------ | -------- | ------------------------------------------------------ |
| `file`       | `String` | Absolute file path                                     |
| `modified`   | `i64`    | File modification timestamp (ms)                       |
| `scan_time`  | `i64`    | Last scan timestamp (ms)                               |
| `is_trashed` | `bool`   | Per-path trash flag (buried vs. visible in trash view) |

Each media record carries the file path it was indexed from. Path-primary
indexing creates one record per physical file, so new records hold a single
`FileModify`; the field remains a list so trash state is tracked per path.

#### Share

Source: `backend/src/model/album.rs`

| Field           | Type              | Description                          |
| --------------- | ----------------- | ------------------------------------ |
| `url`           | `ArrayString<64>` | Unique share token                   |
| `description`   | `String`          | Human-readable description           |
| `password`      | `Option<String>`  | Optional access password             |
| `show_metadata` | `bool`            | Allow metadata view                  |
| `show_download` | `bool`            | Allow download                       |
| `show_upload`   | `bool`            | Allow upload                         |
| `exp`           | `i64`             | Expiration timestamp (ms, 0 = never) |

---

## temp_db.redb — tree snapshot

Caches the tree-sorted view of all media, partitioned by timestamp bucket.

Structure: one dynamic table per timestamp value (`i64` as string table name),
each mapping `u64` (sequential index) → `ReducedData`.

```rust
pub struct ReducedData {
    /// Path-primary asset ID.
    pub asset_id: ArrayString<64>,
    pub hash: ArrayString<64>,
    pub width: u32,
    pub height: u32,
    pub date: i64,
    /// Stored `object.update_at` — cache-bust key for image URLs.
    pub update_at: i64,
    /// Stored `object.pending` — thumbnail/frame still generating.
    pub pending: bool,
}
```

Source: `backend/src/storage/cache.rs`

The on-disk DB acts as a backing store when the in-memory `DashMap` evicts
entries.

---

## cache_db.redb — query prefetch

Caches the "locate" result (scroll position + data length) for filtered
queries.

Structure: one dynamic table per `VERSION_COUNT_TIMESTAMP` (string table
name), each mapping `u64` (query hash) → `Prefetch`.

```rust
pub struct Prefetch {
    pub timestamp: i64,
    pub locate_to: Option<usize>,
    pub data_length: usize,
}
```

Source: `backend/src/storage/cache.rs`

The query hash is computed from the filter expression parameters combined
with the current version timestamp.

---

## expire_db.redb — snapshot expiration

Tracks when in-memory snapshots should be expired.

Source: `backend/src/storage/cache.rs`

### Table: `"expire_table"`

Maps `i64` (snapshot timestamp) → `Option<i64>` (expiration timestamp, `None`
= never expires).

The expire-check loop (24h cycle) reads this table and removes expired
snapshot entries from both the in-memory `DashMap` and the on-disk
`temp_db.redb`.

---

## In-memory structures

These supplement the persistent store with fast read-optimized views:

| Structure                  | Type                                  | Content                                                  |
| -------------------------- | ------------------------------------- | -------------------------------------------------------- |
| `TREE.in_memory`           | `Arc<RwLock<Vec<DatabaseTimestamp>>>` | All `AbstractData` records, sorted by computed timestamp |
| `TREE_SNAPSHOT.in_memory`  | `DashMap<i64, Vec<ReducedData>>`      | Bucketed tree views per timestamp                        |
| `QUERY_SNAPSHOT.in_memory` | `DashMap<u64, Prefetch>`              | Query prefetch results                                   |

`DatabaseTimestamp` pairs an `AbstractData` with its path-primary asset ID
and computed sort timestamp:

```rust
pub struct DatabaseTimestamp {
    pub abstract_data: AbstractData,
    pub timestamp: i64,
    /// The path-primary asset ID for this record.
    pub asset_id: ArrayString<64>,
}
```

All three are lazily initialized as module-level statics and rebuilt every
startup (or on demand after config changes).

---

## Per-record schema versioning

`AbstractData` records in `index_v5.redb` are prefixed with a 2-byte header
`[0xFF, version]` (implemented via redb's `Value` trait in
`backend/src/storage/ser_de.rs`).

`0xFF` is safe as a magic marker because `AbstractData` is a 3-variant enum;
bitcode encodes the discriminant in the lowest 2 bits of the first byte
(values 0, 1, 2). A first byte of `0xFF` has bits `[1:0] = 11` = discriminant
3, which is invalid — so no legitimately encoded `AbstractData` can start with
`0xFF`.

Current schema version: **7** (`SCHEMA_VERSION` in `ser_de.rs`).

### Version history

| Version     | Encoding                                                                                                            | Notes                                                                         |
| ----------- | ------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| 1 (legacy)  | `AlbumMetadata` without `dir_path`; media decoded via the v2 types                                                  | No version prefix on disk; detected by absence of `0xFF`                      |
| 2           | Media metadata with `albums: HashSet`                                                                               | Read path collapses the set to `album: Option` (first element)                |
| 3           | Media `album: Option`; `AlbumMetadata.dir_path: Option<String>` (no `custom_title`)                                 |                                                                               |
| 4           | `AlbumMetadata` gains `custom_date`, still no `custom_title`                                                        | Read path treats existing `title` as `custom_title`; `custom_date` is dropped |
| 5           | `AlbumMetadata` gains `custom_title` (still `dir_path: Option<String>`)                                             |                                                                               |
| 6           | `dir_path` becomes required `String` (metadata-only albums removed); record-level `is_trashed`                      |                                                                               |
| 7 (current) | Per-alias trash: `FileModify.is_trashed` added; `ObjectSchema.is_trashed` removed; `AlbumMetadata.is_trashed` added |                                                                               |

### On-read migration

When a record is read via the `Value::from_bytes` impl, the version byte
selects the correct decoder:

```rust
match version {
    1 => AbstractData::from(decode::<AbstractDataV1>(payload)),
    2 => AbstractData::from(decode::<AbstractDataV2>(payload)),
    // 3, 4, 5, 6 likewise decode the frozen type then convert
    7 => decode::<AbstractData>(payload),  // current, no transform needed
    v => panic!("Unknown schema version {v}"),
}
```

Records with no `0xFF` prefix (pre-versioning) fall through to the v1 decoder.

Each old-version type has a `From` impl that converts it to the current
`AbstractData`:

- **v2 → current**: `HashSet<ArrayString<64>> albums` collapses to
  `Option<ArrayString<64>> album` (takes the first element; empty set becomes
  `None`).
- **v1 → current**: album records predate directory-backed albums, so they get
  an empty-string `dir_path` sentinel (purely defensive — no v1 records are
  expected to exist); image/video records pass through v2 first.

Old frozen types are preserved in `ser_de.rs` under `#[derive(bitcode::Decode)]`
(no `Encode` except under `#[cfg(test)]`) so they serve as read-only migration
targets.

---

## Database migration history

Picasu has gone through five on-disk database formats. The current code only
supports opening `index_v5.redb` directly.

| Format       | File            | Storage engine | Schema                                    | Migration |
| ------------ | --------------- | -------------- | ----------------------------------------- | --------- |
| V2           | `index.redb`    | redb 2.6.x     | Flat `Database`/`Album` structs per table | Removed   |
| V3           | `index.redb`    | redb 3.x       | `AbstractData` enum without `update_at`   | Removed   |
| V4           | `index_v4.redb` | redb 3.x       | `AbstractData` with `update_at`           | Removed   |
| V5 (current) | `index_v5.redb` | redb 4.x       | `AbstractData` schema v7                  | Current   |

### V2 → V4 migration (deleted code, commit `7abf4452`)

The old `src/migration/` directory (deleted) contained:

- **`v2_v3.rs`**: Read the old redb 2.6.x database using the `redb_old` crate.
  Opened two tables (`"database"` with `OldDatabase` entries,
  `"album"` with `OldAlbum` entries). Transformed each record:
  - Extracted underscore-prefixed pseudo-tags (`_favorite`, `_archived`,
    `_trashed`) into dedicated boolean fields.
  - Moved `_user_defined_description` from `exif_vec` to the `description`
    field.
  - Converted `u128` timestamps to `i64`.
  - Migrated `HashSet<ArrayString<64>> album` to `HashSet<ArrayString<64>>`
    (same type).
- **`v3_v4.rs`**: Same redb version (3.x), same `AbstractData` enum shape, but
  the `ObjectSchema` lacked `update_at`. The migration stamped
  `Utc::now().timestamp_millis()` on every record.

Both paths wrote into a fresh `index_v4.redb` using `redb::Database::create()`,
processing in batches of 5000 with Rayon parallelism.

### V4 → V5 rename (commit `640c14c5`)

No data transformation — just `std::fs::rename("index_v4.redb",
"index_v5.redb")`. The file path was updated in `tree/new.rs` and the rename
logic was removed along with the rest of the migration code
(commit `7abf4452`).

### Current startup guard

`migration()` in `backend/src/lib.rs` checks for the existence of
`db/index_v4.redb`. If found, startup is blocked with instructions to
downgrade to v1.2.2 first, which is the last release that carried the old
migration pipeline.
