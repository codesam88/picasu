use crate::model::album::AlbumCombined;
use crate::storage::cache::{MyCow, TREE_SNAPSHOT};
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

    /// Return all filesystem-backed (dir) albums.
    pub fn read_albums(&self) -> Result<Vec<AlbumCombined>, AppError> {
        self.in_disk
            .begin_read()
            .or_raise(|| (ErrorKind::Database, "Failed to begin read transaction"))?
            .open_table(DATA_TABLE)
            .or_raise(|| (ErrorKind::Database, "Failed to open DATA_TABLE"))?
            .iter()
            .or_raise(|| {
                (
                    ErrorKind::Database,
                    "Failed to create iterator over DATA_TABLE",
                )
            })?
            .par_bridge()
            .filter_map(|entry| {
                entry
                    .map(|(_, guard)| match guard.value() {
                        AbstractData::Album(album) if album.metadata.dir_path.is_some() => {
                            Some(album)
                        }
                        _ => None,
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()
            .or_raise(|| {
                (
                    ErrorKind::Database,
                    "Failed to collect album records in parallel",
                )
            })
    }
}

use redb::TableDefinition;

use crate::model::abstract_data::AbstractData;

pub const DATA_TABLE: TableDefinition<&str, AbstractData> = TableDefinition::new("database");
use anyhow::Result;

pub fn open_data_table() -> ReadOnlyTable<&'static str, AbstractData> {
    let read_txn = TREE
        .in_disk
        .begin_read()
        .expect("failed to begin read transaction");
    read_txn
        .open_table(DATA_TABLE)
        .expect("failed to open data table")
}

pub fn open_tree_snapshot_table(timestamp: i64) -> Result<MyCow> {
    TREE_SNAPSHOT.read_tree_snapshot(timestamp).context(format!(
        "Failed to read tree snapshot for timestamp {timestamp}"
    ))
}
