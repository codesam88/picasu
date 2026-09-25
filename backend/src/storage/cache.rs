use crate::constant::ROW_BATCH_NUMBER;
use crate::model::response::DisplayElement;
use crate::model::response::Row;
use rayon::prelude::*;
use redb::ReadableTable;
use std::error::Error;
use std::sync::LazyLock;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use dashmap::DashMap;

use crate::model::response::ReducedData;

#[derive(Debug)]
pub struct TreeSnapshot {
    pub in_disk: &'static redb::Database,
    pub in_memory: &'static DashMap<i64, Vec<ReducedData>>,
}

pub static TREE_SNAPSHOT: LazyLock<TreeSnapshot> = LazyLock::new(TreeSnapshot::new);

use crate::storage::files::get_data_path;

static TREE_SNAPSHOT_IN_DISK: LazyLock<redb::Database> = LazyLock::new(|| {
    let path = get_data_path().join("db/temp_db.redb");
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).expect("failed to create db directory for tree snapshots");
    }
    redb::Database::create(path).expect("failed to create tree snapshot database")
});

static TREE_SNAPSHOT_IN_MEMORY: LazyLock<DashMap<i64, Vec<ReducedData>>> =
    LazyLock::new(DashMap::new);

impl TreeSnapshot {
    pub fn new() -> Self {
        Self {
            in_disk: &TREE_SNAPSHOT_IN_DISK,
            in_memory: &TREE_SNAPSHOT_IN_MEMORY,
        }
    }
}

static LAST_SNAPSHOT_ID: AtomicI64 = AtomicI64::new(0);

/// Allocates an id for a new `TREE_SNAPSHOT` entry. The id is also the snapshot
/// timestamp handed to the client in the prefetch token, so it must be unique per
/// snapshot: wall-clock milliseconds are not, and two snapshots sharing an id
/// would make the later one overwrite the earlier one — its rows would be served
/// under the earlier snapshot's still-valid token.
///
/// Ids stay within a millisecond or two of the wall clock and re-sync as soon as
/// the clock passes the last id, so the expiry bookkeeping that compares snapshot
/// timestamps against the tree version keeps working.
pub fn next_snapshot_id() -> i64 {
    let now = Utc::now().timestamp_millis();
    loop {
        let last = LAST_SNAPSHOT_ID.load(Ordering::SeqCst);
        let next = now.max(last.saturating_add(1));
        if LAST_SNAPSHOT_ID
            .compare_exchange(last, next, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return next;
        }
    }
}
use anyhow::{Result, bail};

impl TreeSnapshot {
    pub fn read_row(&'static self, row_index: usize, timestamp: i64) -> Result<Row> {
        let tree_snapshot = self.read_tree_snapshot(timestamp)?;

        let data_length = tree_snapshot.len();
        let chunk_count = data_length.div_ceil(ROW_BATCH_NUMBER); // Calculate total chunks

        if row_index > chunk_count {
            error!("read_rows out of bound");
            bail!("Row index out of bounds");
        }

        let number_vec = (row_index * ROW_BATCH_NUMBER)
            ..(row_index * ROW_BATCH_NUMBER + ROW_BATCH_NUMBER).min(data_length);

        let display_elements: Vec<DisplayElement> = number_vec
            .map(|index| -> Result<DisplayElement> {
                let (width, height) = tree_snapshot.get_width_height(index)?;
                Ok(DisplayElement {
                    display_width: width,
                    display_height: height,
                })
            })
            .collect::<Result<Vec<DisplayElement>>>()?;

        Ok(Row {
            start: row_index * ROW_BATCH_NUMBER,
            end: row_index * ROW_BATCH_NUMBER + ROW_BATCH_NUMBER - 1,
            display_elements,
            row_index,
        })
    }
}

use std::time::Instant;

use crate::model::response::ScrollBarData;

use chrono::{Datelike, TimeZone, Utc};

impl TreeSnapshot {
    pub fn read_scrollbar(&'static self, timestamp: i64) -> Vec<ScrollBarData> {
        let start_time = Instant::now();
        let tree_snapshot = self
            .read_tree_snapshot(timestamp)
            .expect("failed to read tree snapshot for scrollbar");
        let mut scroll_bar_data_vec = Vec::new();
        let mut last_year = None;
        let mut last_month = None;

        match tree_snapshot {
            MyCow::DashMap(ref_data) => {
                ref_data.iter().enumerate().for_each(|(index, data)| {
                    let datetime = Utc
                        .timestamp_millis_opt(data.date)
                        .single()
                        .expect("invalid timestamp");
                    let year = datetime.year();
                    let month = datetime.month();
                    if last_year != Some(year) || last_month != Some(month) {
                        last_year = Some(year);
                        last_month = Some(month);
                        let scrollbar_data = ScrollBarData {
                            #[allow(clippy::cast_sign_loss)]
                            year: year as usize,
                            #[allow(clippy::cast_sign_loss)]
                            month: month as usize,
                            index,
                        };
                        scroll_bar_data_vec.push(scrollbar_data);
                    }
                });
            }
            MyCow::Redb(redb) => {
                redb.iter()
                    .expect("failed to iterate redb table for scrollbar")
                    .enumerate()
                    .for_each(|(index, result)| {
                        let (_key, value) =
                            result.expect("failed to read row from scrollbar table");
                        let data = value.value();
                        let datetime = Utc
                            .timestamp_millis_opt(data.date)
                            .single()
                            .expect("invalid timestamp");
                        let year = datetime.year();
                        let month = datetime.month();
                        if last_year != Some(year) || last_month != Some(month) {
                            last_year = Some(year);
                            last_month = Some(month);
                            let scrollbar_data = ScrollBarData {
                                #[allow(clippy::cast_sign_loss)]
                                year: year as usize,
                                #[allow(clippy::cast_sign_loss)]
                                month: month as usize,
                                index,
                            };
                            scroll_bar_data_vec.push(scrollbar_data);
                        }
                    });
            }
        }
        info!(duration = &*format!("{:?}", start_time.elapsed()); "Generate scrollbar");
        scroll_bar_data_vec
    }
}

use crate::storage::db::{TagInfo, open_metadata_table};
impl TreeSnapshot {
    pub fn read_tags() -> Result<Vec<TagInfo>> {
        // Concurrent counter for each tag
        let tag_counts: DashMap<String, AtomicUsize> = DashMap::new();

        // Begin read-only transaction and open the METADATA_TABLE
        let metadata_table = open_metadata_table();

        // Walk the table in parallel; stop on first error
        metadata_table
            .iter()
            .context("Create iterator over METADATA_TABLE failed")?
            .par_bridge()
            .try_for_each(|entry| -> Result<()> {
                let (_, data) = entry.context("Read table row failed")?;
                let payload = data.value();

                // Count regular tags only
                for tag in payload.tags() {
                    tag_counts
                        .entry(tag.clone())
                        .or_insert_with(|| AtomicUsize::new(0))
                        .fetch_add(1, Ordering::Relaxed);
                }

                Ok(())
            })?;

        let tag_infos: Vec<TagInfo> = tag_counts
            .par_iter()
            .map(|e| TagInfo {
                tag: e.key().clone(),
                number: e.value().load(Ordering::Relaxed),
            })
            .collect();

        Ok(tag_infos)
    }
}

use anyhow::Context;
use arrayvec::ArrayString;
use dashmap::mapref::one::Ref;
use redb::{ReadOnlyTable, ReadableDatabase, ReadableTableMetadata, TableDefinition};

impl TreeSnapshot {
    pub fn read_tree_snapshot(&'static self, timestamp: i64) -> Result<MyCow> {
        if let Some(data) = self.in_memory.get(&timestamp) {
            return Ok(MyCow::DashMap(data));
        }

        let read_txn = self.in_disk.begin_read()?;

        let binding = timestamp.to_string();
        let table_definition: TableDefinition<u64, ReducedData> = TableDefinition::new(&binding);

        let table = read_txn.open_table(table_definition)?;
        Ok(MyCow::Redb(table))
    }
}

#[derive(Debug)]
pub enum MyCow {
    DashMap(Ref<'static, i64, Vec<ReducedData>>),
    Redb(ReadOnlyTable<u64, ReducedData>),
}

impl MyCow {
    #[allow(clippy::cast_possible_truncation)]
    pub fn len(&self) -> usize {
        match self {
            MyCow::DashMap(data) => data.value().len(),
            MyCow::Redb(table) => table.len().expect("failed to get table length") as usize,
        }
    }

    pub fn get_width_height(&self, index: usize) -> Result<(u32, u32)> {
        match self {
            MyCow::DashMap(data) => {
                let data = &data.value()[index];
                Ok((data.width, data.height))
            }
            MyCow::Redb(table) => {
                let data = &table
                    .get(index as u64)?
                    .context(format!(
                        "Fail to find with and height in tree snapshots for index {index}"
                    ))?
                    .value();

                Ok((data.width, data.height))
            }
        }
    }

    #[allow(dead_code)]
    pub fn get_hash(&self, index: usize) -> Result<ArrayString<64>> {
        match self {
            MyCow::DashMap(data) => {
                let data = &data.value()[index];
                Ok(data.hash)
            }
            MyCow::Redb(table) => {
                let data = table
                    .get(index as u64)?
                    .context(format!(
                        "Fail to find hash in tree snapshots for index {index}"
                    ))?
                    .value();
                Ok(data.hash)
            }
        }
    }

    #[allow(dead_code)]
    pub fn get_asset_id(&self, index: usize) -> Result<ArrayString<64>> {
        match self {
            MyCow::DashMap(data) => {
                let data = &data.value()[index];
                Ok(data.asset_id)
            }
            MyCow::Redb(table) => {
                let data = table
                    .get(index as u64)?
                    .context(format!(
                        "Fail to find asset_id in tree snapshots for index {index}"
                    ))?
                    .value();
                Ok(data.asset_id)
            }
        }
    }

    /// Full snapshot entry at `index`: identity, dimensions, tree date, and
    /// the display fields (`update_at`, `pending`) lean list rows need without
    /// a per-row `METADATA_TABLE` read.
    pub fn get_reduced(&self, index: usize) -> Result<ReducedData> {
        match self {
            MyCow::DashMap(data) => {
                let data = &data.value()[index];
                Ok(*data)
            }
            MyCow::Redb(table) => {
                let data = table
                    .get(index as u64)?
                    .context(format!("Fail to find snapshot entry for index {index}"))?
                    .value();
                Ok(data)
            }
        }
    }
}

#[derive(Debug)]
pub struct QuerySnapshot {
    pub in_disk: &'static redb::Database,
    pub in_memory: &'static DashMap<u64, Prefetch>, // hash of query and VERSION_COUNT_TIMESTAMP -> prefetch
}

pub static QUERY_SNAPSHOT: LazyLock<QuerySnapshot> = LazyLock::new(QuerySnapshot::new);

static QUERY_SNAPSHOT_IN_DISK: LazyLock<redb::Database> = LazyLock::new(|| {
    let path = get_data_path().join("db/cache_db.redb");
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).expect("failed to create db directory for query snapshots");
    }
    redb::Database::create(path).expect("failed to create query snapshot database")
});

static QUERY_SNAPSHOT_IN_MEMORY: LazyLock<DashMap<u64, Prefetch>> = LazyLock::new(DashMap::new);

impl QuerySnapshot {
    pub fn new() -> Self {
        Self {
            in_disk: &QUERY_SNAPSHOT_IN_DISK,
            in_memory: &QUERY_SNAPSHOT_IN_MEMORY,
        }
    }
}

use crate::{model::response::Prefetch, storage::db::VERSION_COUNT_TIMESTAMP};

impl QuerySnapshot {
    pub fn read_query_snapshot(
        &'static self,
        query_hash: u64,
    ) -> Result<Option<Prefetch>, Box<dyn Error>> {
        if let Some(data) = self.in_memory.get(&query_hash) {
            return Ok(Some(*data.value()));
        }

        let read_txn = self
            .in_disk
            .begin_read()
            .expect("failed to begin read transaction for query snapshot");

        let count_version = VERSION_COUNT_TIMESTAMP.load(Ordering::Relaxed).to_string();

        let table_definition: TableDefinition<u64, Prefetch> = TableDefinition::new(&count_version);

        let table = read_txn.open_table(table_definition)?;

        let timestamp = table.get(query_hash)?;

        Ok(timestamp.map(|inner_value| inner_value.value()))
    }
}

pub static EXPIRE_TABLE_DEFINITION: TableDefinition<i64, Option<i64>> =
    TableDefinition::new("expire_table"); // timestamp -> expired time; none means never expired

#[derive(Debug)]
pub struct Expire {
    pub in_disk: &'static redb::Database,
}

pub static EXPIRE: LazyLock<Expire> = LazyLock::new(Expire::new);

static EXPIRE_IN_DISK: LazyLock<redb::Database> = LazyLock::new(|| {
    let path = get_data_path().join("db/expire_db.redb");
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).expect("failed to create db directory for expire database");
    }
    redb::Database::create(path).expect("failed to create expire database")
});

impl Expire {
    pub fn new() -> Self {
        Expire {
            in_disk: &EXPIRE_IN_DISK,
        }
    }
}

// Import necessary modules and items
use log::info;
impl Expire {
    /// Checks if the given `timestamp` has expired.
    ///
    /// This function performs the following steps:
    /// 1. Begins a read transaction to access the expiration table.
    /// 2. Retrieves the expiration time associated with the provided `timestamp`.
    /// 3. Compares the current timestamp with the retrieved expiration time.
    /// 4. If expired, begins a write transaction to remove expired entries.
    /// 5. Logs the deletion of each expired key and the remaining items in the table.
    ///
    /// # Arguments
    ///
    /// * `timestamp` - A `i64` value representing the timestamp to check for expiration.
    ///
    /// # Returns
    ///
    /// * `true` if the `timestamp` has a recorded expiry time that has passed.
    /// * `false` if the `timestamp` has not yet expired, or has no row recorded
    ///   yet (the active version, whose expiry is scheduled on the next rotation).
    pub fn expired_check(&self, timestamp: i64) -> bool {
        // Begin a read transaction on the in-memory disk
        let read_transaction = self
            .in_disk
            .begin_read()
            .expect("failed to begin read transaction for expire check");

        // Open the expiration table using its definition
        let expire_table = read_transaction
            .open_table(EXPIRE_TABLE_DEFINITION)
            .expect("failed to open expire table");

        // Attempt to retrieve the expiration entry for the given timestamp
        match expire_table
            .get(timestamp)
            .expect("failed to get expire entry")
            .and_then(|entry| entry.value())
        {
            // If an expiration time exists and the current time has surpassed it
            Some(expire_time) if Utc::now().timestamp_millis() > expire_time => {
                // Begin a write transaction to modify the expiration table
                let write_transaction = self
                    .in_disk
                    .begin_write()
                    .expect("failed to begin write transaction for expire cleanup");
                {
                    // Open the expiration table for writing
                    let mut write_table = write_transaction
                        .open_table(EXPIRE_TABLE_DEFINITION)
                        .expect("failed to open expire table for writing");

                    // Iterate over all entries in the expiration table
                    for (key, _) in expire_table
                        .iter()
                        .expect("failed to iterate expire table")
                        .flatten()
                    {
                        let key_timestamp = key.value();
                        // If the key's timestamp is less than or equal to the provided timestamp
                        if key_timestamp <= timestamp {
                            // Remove the expired key from the table
                            write_table
                                .remove(key_timestamp)
                                .expect("failed to remove expired key");
                            // Log the deletion of the expired key
                            info!("Deleted expired key: {key_timestamp:?}");
                        }
                    }

                    // Log the number of items remaining in the expiration table
                    info!(
                        "{} items remaining in expire table",
                        write_table
                            .len()
                            .expect("failed to get expire table length")
                    );
                }
                // Commit the write transaction to finalize changes
                write_transaction
                    .commit()
                    .expect("failed to commit expire transaction");
                // Indicate that the timestamp has expired
                true
            }
            // `Some(_)`: an expiration time exists but has not yet been reached.
            // `None`: no row recorded for this timestamp. This is the active version
            // whose expiry hasn't been scheduled yet (see `update_expire_task`'s
            // swap/commit race) — not an already-removed entry, since a removed
            // entry's query snapshot table would already be gone and never reach
            // this check again.
            Some(_) | None => false,
        }
    }
}

#[cfg(test)]
mod snapshot_id_tests {
    use super::*;
    use std::collections::HashSet;
    use std::thread;

    #[test]
    fn snapshot_ids_are_unique_within_a_millisecond() {
        let ids: HashSet<i64> = (0..10_000).map(|_| next_snapshot_id()).collect();

        assert_eq!(
            ids.len(),
            10_000,
            "snapshot ids must not repeat: a repeated id overwrites the previous \
             snapshot and lets its token serve the newer snapshot's rows"
        );
    }

    #[test]
    fn snapshot_ids_are_unique_across_threads() {
        let ids: Vec<i64> = (0..8)
            .map(|_| thread::spawn(|| (0..1_000).map(|_| next_snapshot_id()).collect::<Vec<i64>>()))
            .flat_map(|handle| handle.join().expect("snapshot id thread panicked"))
            .collect();

        let unique: HashSet<i64> = ids.iter().copied().collect();
        assert_eq!(
            unique.len(),
            ids.len(),
            "concurrent prefetches must not share a snapshot id"
        );
    }

    #[test]
    fn snapshot_ids_never_fall_behind_the_wall_clock() {
        // Ids may run ahead of the clock when many snapshots are created within
        // one millisecond, and re-sync once the clock catches up. They must never
        // be older than the clock: expiry bookkeeping only discards a snapshot
        // once a newer tree version exists, so a stale id would expire a snapshot
        // that is still in use.
        let before = Utc::now().timestamp_millis();
        let id = next_snapshot_id();

        assert!(
            id >= before,
            "snapshot id {id} is older than the wall clock {before}"
        );
    }
}

#[cfg(test)]
mod expire_tests {
    use super::*;

    fn make_expire() -> Expire {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("expire_test.redb");
        // Leak the tempdir so it stays alive for the lifetime of the leaked Database below.
        Box::leak(Box::new(dir));
        let db = redb::Database::create(db_path).expect("failed to create test database");
        Expire {
            in_disk: Box::leak(Box::new(db)),
        }
    }

    #[test]
    fn expired_check_does_not_expire_unscheduled_active_version() {
        let expire = make_expire();

        // The expire table always exists in production (update_expire_task creates it
        // on first use), but simulates the race: `VERSION_COUNT_TIMESTAMP` has already
        // been swapped to a newer value, while the write transaction recording this
        // timestamp's expiry has not committed yet, so no row exists for it.
        let write_txn = expire
            .in_disk
            .begin_write()
            .expect("failed to begin write transaction");
        {
            write_txn
                .open_table(EXPIRE_TABLE_DEFINITION)
                .expect("failed to open expire table");
        }
        write_txn.commit().expect("failed to commit transaction");

        let timestamp = 1_000_i64;

        assert!(
            !expire.expired_check(timestamp),
            "a timestamp with no recorded expiry must not be treated as already expired"
        );
    }
}
