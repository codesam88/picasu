//! In-memory asset tree derived from the redb asset tables.
//!
//! Background writes commit to redb and bump a monotonic epoch. The
//! in-memory view is rebuilt only when a caller asks for it and the epoch
//! moved since the last build, so a scan that writes thousands of rows does
//! not rebuild the whole view once per flush.

use chrono::Utc;
use rayon::prelude::ParallelSliceMut;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::model::response::DatabaseTimestamp;
use crate::storage::db::{INVALIDATION_VERSION, TREE, VERSION_COUNT_TIMESTAMP};
use crate::tasks::observe::{Stage, Timer};

/// Advanced after any asset-table write. `snapshot_tree` rebuilds when this
/// differs from the epoch the cached view was built at.
static TREE_EPOCH: AtomicU64 = AtomicU64::new(0);

/// Epoch of the cached view. `u64::MAX` means "never built".
static BUILT_EPOCH: AtomicU64 = AtomicU64::new(u64::MAX);

static REBUILD_LOCK: Mutex<()> = Mutex::new(());

const PRIORITY_LIST: &[&str] = &["DateTimeOriginal", "filename", "modified", "scan_time"];

/// Mark the cached view stale and advance the query-cache version.
///
/// The version timestamp is separate from the epoch: query cache keys and
/// snapshot expiry use wall-clock milliseconds, while the epoch only needs
/// to be monotonic.
pub(crate) fn bump_tree_epoch() {
    TREE_EPOCH.fetch_add(1, Ordering::SeqCst);
    // Concern (b): keep the wall-clock identity current for table names/expiry.
    VERSION_COUNT_TIMESTAMP.fetch_max(Utc::now().timestamp_millis(), Ordering::SeqCst);
    // Concern (a): strictly monotonic invalidation version for cache keys.
    INVALIDATION_VERSION.fetch_add(1, Ordering::SeqCst);
}

/// Clone the current asset tree, rebuilding first when writes moved the
/// epoch since the last build.
pub fn snapshot_tree() -> Vec<DatabaseTimestamp> {
    refresh_if_stale();
    TREE.in_memory.read().expect("lock poisoned").clone()
}

/// Invalidate the view after a request-path write and schedule expiry
/// bookkeeping for the version timestamp the write produced. Request paths
/// do not rebuild the tree: the next reader does.
pub(crate) fn tree_changed() {
    bump_tree_epoch();
    crate::tasks::BATCH_COORDINATOR
        .execute_batch_detached(crate::tasks::batcher::update_expire::UpdateExpireTask);
}

/// Force the next `snapshot_tree` to rebuild. Callers that clear the cached
/// rows directly must advance the epoch *after* the clear, so an in-flight
/// rebuild that already read the old rows can only publish them under the
/// old epoch and is discarded by the next reader.
pub(crate) fn invalidate_tree_view() {
    bump_tree_epoch();
}

fn refresh_if_stale() {
    let epoch = TREE_EPOCH.load(Ordering::SeqCst);
    if BUILT_EPOCH.load(Ordering::SeqCst) == epoch {
        return;
    }
    let _guard = REBUILD_LOCK.lock().expect("tree rebuild lock poisoned");
    let epoch = TREE_EPOCH.load(Ordering::SeqCst);
    if BUILT_EPOCH.load(Ordering::SeqCst) == epoch {
        return;
    }
    rebuild(epoch);
}

fn rebuild(epoch: u64) {
    let start_time = Instant::now();
    let total_timer = Timer::start(Stage::TreeRefreshTotal);

    let build_timer = Timer::start(Stage::TreeRefreshBuild);
    let Ok(mut database_timestamp_vec) = build_from_asset_tables(PRIORITY_LIST) else {
        // Leave the previous view and its built epoch in place so the next
        // caller retries instead of caching an empty/failed read as current.
        log::warn!("tree view rebuild failed; retaining the previous view");
        return;
    };
    let rows = u64::try_from(database_timestamp_vec.len()).unwrap_or(u64::MAX);
    build_timer.finish(rows);

    // Sort by timestamp descending, with a deterministic secondary key
    // (the asset path) so items with equal timestamps have a stable order.
    let sort_timer = Timer::start(Stage::TreeRefreshSort);
    database_timestamp_vec.par_sort_by(|a, b| {
        b.timestamp.cmp(&a.timestamp).then_with(|| {
            let a_path = a.abstract_data.path().map_or("", |a| a.file.as_str());
            let b_path = b.abstract_data.path().map_or("", |a| a.file.as_str());
            a_path.cmp(b_path)
        })
    });
    sort_timer.finish(rows);

    let write_timer = Timer::start(Stage::TreeRefreshWriteWait);
    let mut tree_guard = TREE.in_memory.write().expect("lock poisoned");
    write_timer.finish(rows);
    *tree_guard = database_timestamp_vec;
    drop(tree_guard);

    // Query cache keys and snapshot expiry compare this against wall-clock
    // timestamps, so keep it monotonic and current even when the clock moves
    // backwards.
    VERSION_COUNT_TIMESTAMP.fetch_max(Utc::now().timestamp_millis(), Ordering::SeqCst);

    // Record the epoch observed at the start of this build. A write that
    // lands during the build advances the epoch again, so the next caller
    // rebuilds instead of serving a partially stale view.
    BUILT_EPOCH.store(epoch, Ordering::SeqCst);

    total_timer.finish(rows);

    let duration = format!("{:?}", start_time.elapsed());
    debug!(duration = &*duration; "In-memory tree rebuilt.");
}

/// Build the view from `ASSET_BY_ID`, composing each row from the asset's
/// identity `AssetRecord` plus its optional `METADATA_TABLE` payload.
fn build_from_asset_tables(priority_list: &[&str]) -> Result<Vec<DatabaseTimestamp>, ()> {
    use crate::model::metadata_record::compose_abstract_data;
    use crate::storage::db::{ASSET_BY_ID, METADATA_TABLE};
    use redb::{ReadableDatabase, ReadableTable};

    let Ok(txn) = TREE.in_disk.begin_read() else {
        log::warn!("build_from_asset_tables: failed to begin read transaction");
        return Err(());
    };
    let Ok(table) = txn.open_table(ASSET_BY_ID) else {
        log::warn!("build_from_asset_tables: failed to open ASSET_BY_ID");
        return Err(());
    };
    // One read transaction for identity and payload keeps a row's composed
    // value consistent across a concurrent write commit.
    let metadata_table = txn.open_table(METADATA_TABLE).ok();

    let mut entries = Vec::new();
    for row in table.iter().into_iter().flatten() {
        let Ok((_, value)) = row else { continue };
        let record: crate::model::asset::AssetRecord = match serde_json::from_str(value.value()) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let payload = metadata_table
            .as_ref()
            .and_then(|table| table.get(&*record.asset_id).ok().flatten())
            .map(|guard| guard.value());

        // Composition makes path drift impossible: the file entry is always
        // assembled from the record, never read back from storage.
        let abstract_data = compose_abstract_data(&record, payload.as_ref());

        entries.push(DatabaseTimestamp::with_asset_id(
            abstract_data,
            priority_list,
            record.asset_id,
        ));
    }

    log::debug!(
        "build_from_asset_tables: built {} entries from ASSET_BY_ID",
        entries.len()
    );

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::INVALIDATION_VERSION;
    use std::sync::atomic::Ordering;

    #[test]
    fn two_bumps_yield_distinct_versions() {
        bump_tree_epoch();
        let v1 = INVALIDATION_VERSION.load(Ordering::SeqCst);
        bump_tree_epoch();
        let v2 = INVALIDATION_VERSION.load(Ordering::SeqCst);
        assert!(
            v2 > v1,
            "each bump must advance the version: v1={v1}, v2={v2}"
        );
    }
}
