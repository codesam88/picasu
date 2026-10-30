use crate::model::album::AlbumCombined;
use crate::storage::cache::{MyCow, SnapshotReadError, TREE_SNAPSHOT};
use crate::storage::files::get_data_path;
use anyhow::Context;
use dashmap::DashMap;
use redb::ReadOnlyTable;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use crate::model::response::DatabaseTimestamp;
use std::sync::{Arc, LazyLock, RwLock, atomic::AtomicI64};

pub struct Tree {
    pub in_disk: &'static redb::Database,
    pub in_memory: &'static Arc<RwLock<Vec<DatabaseTimestamp>>>,
}

pub static TREE: LazyLock<Tree> = LazyLock::new(Tree::new);

pub static VERSION_COUNT_TIMESTAMP: AtomicI64 = AtomicI64::new(0);

static TREE_SNAPSHOT_IN_MEMORY: LazyLock<Arc<RwLock<Vec<DatabaseTimestamp>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(vec![])));

static TREE_SNAPSHOT_IN_DISK: LazyLock<redb::Database> = LazyLock::new(|| {
    let path = get_data_path().join("db/index_v5.redb");
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).expect("failed to create db directory for index database");
    }
    redb::Database::create(path).expect("failed to create index database")
});

impl Tree {
    pub fn new() -> Self {
        Self {
            in_disk: &TREE_SNAPSHOT_IN_DISK,
            in_memory: &TREE_SNAPSHOT_IN_MEMORY,
        }
    }
}

use crate::error::{AppError, ErrorKind, ResultExt};
use rayon::iter::{IntoParallelRefIterator, ParallelBridge, ParallelIterator};
use redb::{ReadableDatabase, ReadableTable};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash, utoipa::ToSchema)]
pub struct TagInfo {
    pub tag: String,
    pub number: usize,
}

impl Tree {
    pub fn read_tags(&'static self) -> Vec<TagInfo> {
        // ... (unchanged)
        let tag_counts: DashMap<String, AtomicUsize> = DashMap::new();

        self.in_memory
            .read()
            .expect("lock poisoned")
            .iter()
            .par_bridge()
            .for_each(|database_timestamp| {
                let abstract_data = &database_timestamp.abstract_data;

                // Count regular tags only
                for tag in abstract_data.tag() {
                    let counter = tag_counts
                        .entry(tag.clone())
                        .or_insert_with(|| AtomicUsize::new(0));
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            });

        let tag_infos: Vec<TagInfo> = tag_counts
            .par_iter()
            .map(|entry| TagInfo {
                tag: entry.key().clone(),
                number: entry.value().load(Ordering::Relaxed),
            })
            .collect();

        tag_infos
    }

    /// Return all filesystem-backed (dir) albums, composed from each album's
    /// identity `AssetRecord` plus its stored metadata payload.
    pub fn read_albums(&self) -> Result<Vec<AlbumCombined>, AppError> {
        use crate::model::asset::AssetRecord;
        use crate::model::metadata_record::{MetadataRecord, compose_abstract_data};
        use redb::ReadableDatabase;

        let read_txn = self
            .in_disk
            .begin_read()
            .or_raise(|| (ErrorKind::Database, "Failed to begin read transaction"))?;
        let metadata_table = read_txn
            .open_table(METADATA_TABLE)
            .or_raise(|| (ErrorKind::Database, "Failed to open METADATA_TABLE"))?;
        let id_table = read_txn
            .open_table(ASSET_BY_ID)
            .or_raise(|| (ErrorKind::Database, "Failed to open ASSET_BY_ID"))?;

        let mut albums = Vec::new();
        for entry in metadata_table.iter().or_raise(|| {
            (
                ErrorKind::Database,
                "Failed to create iterator over METADATA_TABLE",
            )
        })? {
            let (key, guard) =
                entry.or_raise(|| (ErrorKind::Database, "Failed to read METADATA_TABLE row"))?;
            let MetadataRecord::Album(payload) = guard.value() else {
                continue;
            };
            let record_json = id_table
                .get(key.value())
                .or_raise(|| (ErrorKind::Database, "Failed to read ASSET_BY_ID row"))?;
            let Some(record_json) = record_json else {
                continue;
            };
            let record: AssetRecord = serde_json::from_str(record_json.value()).or_raise(|| {
                (
                    ErrorKind::Database,
                    "Failed to deserialize AssetRecord for album",
                )
            })?;
            if let AbstractData::Album(album) =
                compose_abstract_data(&record, Some(&MetadataRecord::Album(payload.clone())))
            {
                albums.push(album);
            }
        }
        Ok(albums)
    }
}

use redb::TableDefinition;

use crate::model::abstract_data::AbstractData;
use crate::model::metadata_record::MetadataRecord;

/// `asset_id` → metadata-only [`MetadataRecord`] payload. Identity fields
/// (path, times, trash, size/ext, album, ids) are not stored here — see
/// [`crate::model::metadata_record`]. The on-disk name is `asset_metadata`;
/// rows written under the previous `metadata` name held a different value
/// type and are intentionally left unreferenced (a clean rebuild repopulates
/// this table; there is no migration).
pub const METADATA_TABLE: TableDefinition<&str, MetadataRecord> =
    TableDefinition::new("asset_metadata");

// ── Path-primary asset stores ────────────────────────────────────────────────
//
// These tables implement the path-primary asset model where each physical
// file or directory has exactly one independently addressable asset record.
// See `.plan/asset-path-primary-index.md` for the design contract.

/// Maps filesystem path → `asset_id`.
/// One row per physical file or directory. Ensures path uniqueness.
pub const ASSET_BY_PATH: TableDefinition<&str, &str> = TableDefinition::new("asset_by_path");

/// Maps `asset_id` → JSON-serialized `AssetRecord`.
/// One row per asset. Contains kind, path, hash, and metadata.
pub const ASSET_BY_ID: TableDefinition<&str, &str> = TableDefinition::new("asset_by_id");

/// Maps `content_hash` → JSON-serialized `Vec` of `asset_id`.
/// One row per unique content hash. Albums do not appear here.
/// Multiple asset IDs in a row means those assets share identical content
/// but remain independently addressable.
pub const DUPE_INDEX: TableDefinition<&str, &str> = TableDefinition::new("dupe_index");

/// Maps invite id (blake3 hash hex of the token) → JSON-serialized
/// [`crate::auth::invites::InviteRecord`]. Only the token hash is ever
/// stored; the plaintext token exists solely in the creation response.
pub const INVITES: TableDefinition<&str, &str> = TableDefinition::new("invites");

/// Maps a revocation key → JSON-serialized [`crate::auth::revocations::Revocation`]:
/// the capabilities that must not work any more (a logged-out session, a
/// used invite, a revoked share), each with the expiry of the thing it
/// revokes. Runtime state, not identity.
pub const REVOCATIONS: TableDefinition<&str, &str> = TableDefinition::new("revocations");

use anyhow::Result;

pub fn open_metadata_table() -> ReadOnlyTable<&'static str, MetadataRecord> {
    let read_txn = TREE
        .in_disk
        .begin_read()
        .expect("failed to begin read transaction");
    read_txn
        .open_table(METADATA_TABLE)
        .expect("failed to open data table")
}

pub fn open_tree_snapshot_table(timestamp: i64) -> Result<MyCow, SnapshotReadError> {
    TREE_SNAPSHOT.read_tree_snapshot(timestamp)
}

/// Drop and recreate all four asset tables without decoding any row values.
///
/// Unlike the row-by-row clear helpers, this uses `redb`'s `delete_table`,
/// which removes a table (and all of its pages) by name without ever calling
/// `Value::from_bytes`. That is what allows recovery from a schema bump:
/// a `METADATA_TABLE` row written under an older (or corrupt) version cannot
/// be iterated, so the legacy clear path panics on `from_bytes`, while this
/// path simply discards the whole table and recreates an empty one.
#[allow(dead_code)]
pub fn clear_all_asset_tables_drop() -> anyhow::Result<()> {
    let txn = TREE
        .in_disk
        .begin_write()
        .context("begin write for asset table drop")?;
    let _ = txn
        .delete_table(ASSET_BY_PATH)
        .context("delete ASSET_BY_PATH")?;
    txn.open_table(ASSET_BY_PATH)
        .context("reopen ASSET_BY_PATH")?;

    let _ = txn
        .delete_table(ASSET_BY_ID)
        .context("delete ASSET_BY_ID")?;
    txn.open_table(ASSET_BY_ID).context("reopen ASSET_BY_ID")?;

    let _ = txn.delete_table(DUPE_INDEX).context("delete DUPE_INDEX")?;
    txn.open_table(DUPE_INDEX).context("reopen DUPE_INDEX")?;

    let _ = txn
        .delete_table(METADATA_TABLE)
        .context("delete METADATA_TABLE")?;
    txn.open_table(METADATA_TABLE)
        .context("reopen METADATA_TABLE")?;

    txn.commit().context("commit asset table drop")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::bootstrap::{TEST_ENV, TEST_SERIAL_GUARD};
    use redb::{TableDefinition, TypeName, Value};

    #[derive(Debug)]
    struct RawMetadataValue;

    impl Value for RawMetadataValue {
        type SelfType<'a> = Vec<u8>;
        type AsBytes<'a> = Vec<u8>;

        fn fixed_width() -> Option<usize> {
            None
        }

        fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
        where
            Self: 'a,
        {
            data.to_vec()
        }

        fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
        where
            Self: 'b,
        {
            value.clone()
        }

        fn type_name() -> TypeName {
            TypeName::new("MetadataRecord")
        }
    }

    const RAW_METADATA_TABLE: TableDefinition<&str, RawMetadataValue> =
        TableDefinition::new("asset_metadata");

    #[test]
    fn drop_clear_handles_undecodable_metadata_row() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;

        // Make sure all four tables exist before seeding.
        {
            let txn = TREE
                .in_disk
                .begin_write()
                .expect("begin write for table ensure");
            txn.open_table(ASSET_BY_PATH).expect("open ASSET_BY_PATH");
            txn.open_table(ASSET_BY_ID).expect("open ASSET_BY_ID");
            txn.open_table(DUPE_INDEX).expect("open DUPE_INDEX");
            txn.open_table(METADATA_TABLE).expect("open METADATA_TABLE");
            txn.commit().expect("commit table ensure");
        }

        // Seed an undecodable METADATA_TABLE row (version 9) plus a row in
        // another asset table, using a raw value type that shares the stored
        // type name so redb accepts the write.
        {
            let txn = TREE.in_disk.begin_write().expect("begin write for seed");
            {
                let mut table = txn
                    .open_table(RAW_METADATA_TABLE)
                    .expect("open raw metadata table for seed");
                table
                    .insert("undecodable", vec![0xFF, 9, 0, 0, 0])
                    .expect("insert undecodable row");
            }
            {
                let mut table = txn
                    .open_table(ASSET_BY_PATH)
                    .expect("open ASSET_BY_PATH for seed");
                table.insert("p", "id").expect("insert path row");
            }
            txn.commit().expect("commit seed");
        }

        // The legacy row-iteration clear would panic decoding the row above;
        // the drop-based helper must not.
        clear_all_asset_tables_drop().expect("drop clear must succeed");

        // All four tables must be readable and empty afterwards.
        {
            let txn = TREE.in_disk.begin_read().expect("begin read after clear");
            let path_table = txn.open_table(ASSET_BY_PATH).expect("open ASSET_BY_PATH");
            assert_eq!(
                path_table.iter().expect("iter ASSET_BY_PATH").count(),
                0,
                "ASSET_BY_PATH must be empty after drop clear"
            );
            let id_table = txn.open_table(ASSET_BY_ID).expect("open ASSET_BY_ID");
            assert_eq!(
                id_table.iter().expect("iter ASSET_BY_ID").count(),
                0,
                "ASSET_BY_ID must be empty after drop clear"
            );
            let dupe_table = txn.open_table(DUPE_INDEX).expect("open DUPE_INDEX");
            assert_eq!(
                dupe_table.iter().expect("iter DUPE_INDEX").count(),
                0,
                "DUPE_INDEX must be empty after drop clear"
            );
            let metadata_table = txn.open_table(METADATA_TABLE).expect("open METADATA_TABLE");
            assert_eq!(
                metadata_table.iter().expect("iter METADATA_TABLE").count(),
                0,
                "METADATA_TABLE must be empty after drop clear"
            );
        }
    }
}
