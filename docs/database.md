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

Current schema version: **1** (`SCHEMA_VERSION` in `ser_de.rs`). The structs
in `backend/src/model/` are the version-1 schema.

### Decode policy

When a record is read via the `Value::from_bytes` impl, the version byte
selects the decoder:

```rust
let [0xFF, version, payload @ ..] = data else { rebuild_required(...) };
if version != SCHEMA_VERSION {
    rebuild_required(...); // unknown/unsupported version
}
decode::<AbstractData>(payload) // version 1: current structs, no transform
```

- Version `1` (the `SCHEMA_VERSION` prefix) decodes the current structs
  directly — no `From` conversions.
- Any other version byte, or input with no `0xFF` prefix at all, panics via
  `rebuild_required`, which tells the operator to rebuild
  (`POST /post/rebuild`) or start with a fresh `DATA_HOME`. Prefixless input
  is rejected rather than treated as version 1, because version 1 means the
  _current_ schema — silently decoding ancient prefixless bytes against it
  would corrupt data.

redb 4.2's `Value::from_bytes` returns the decoded value directly (no
associated error type), so a decode failure can only surface as a panic —
the same mechanism the other `Value` impls in `ser_de.rs` use.

### Changing the schema

Migration between schema versions is **not supported**; a clean rebuild is
the only upgrade path. When the schema changes (new fields, removed fields,
reordered variants):

1. Increment `SCHEMA_VERSION`.
2. Copy the current structs to frozen `AbstractDataVN` / `AlbumCombinedVN` /
   etc. types.
3. Add a match arm in `from_bytes` for the previous version.

---

## Database file formats

Picasu has gone through several on-disk database formats. The current code
opens `index_v5.redb` directly.

| Format       | File            | Storage engine | Schema           |
| ------------ | --------------- | -------------- | ---------------- |
| V2           | `index.redb`    | redb 2.6.x     | Removed          |
| V3           | `index.redb`    | redb 3.x       | Removed          |
| V4           | `index_v4.redb` | redb 3.x       | Removed          |
| V5 (current) | `index_v5.redb` | redb 4.x       | Schema version 1 |

Older formats are not supported and no migration path exists between them
(or between per-record schema versions) — rebuild from the filesystem
(`POST /post/rebuild`) or start with a fresh `DATA_HOME`. A stale
`index_v4.redb` file, if present, is simply ignored: the app opens
`index_v5.redb`.
