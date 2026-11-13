//! In-memory asset tree derived from the redb asset tables.
//!
//! Background writes commit to redb and bump a monotonic epoch. The
//! in-memory view is rebuilt only when a caller asks for it and the epoch
//! moved since the last build, so a scan that writes thousands of rows does
//! not rebuild the whole view once per flush.
//!
//! # Freshness contract
//!
//! - **Background writes** (writer flush/sweep/expire batches): mark the tree
//!   dirty on every batch, but advance the request-visible invalidation
//!   counter ([`INVALIDATION_VERSION`]) at most once per ~1 s via a
//!   leading-edge throttle. The first background write after a quiet window
//!   longer than ~1 s invalidates immediately; further writes inside the
//!   window only dirty the tree. A reader may therefore see a prefetch cache
//!   that is up to ~1-2 s stale during a scan — explicitly acceptable.
//! - **Request-path commits**: full synchronous bump on every commit, so
//!   read-your-write stays intact and the CACHE-01 same-millisecond case keeps
//!   working.
//! - **Scan terminal transition** (completed/failed/canceled): a full
//!   synchronous bump runs strictly before the terminal state is published, so
//!   a client that polls status and then reads (~20 ms later) always observes
//!   the finished scan's rows.

use chrono::Utc;
use rayon::prelude::ParallelSliceMut;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

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

/// Wall-clock millisecond of the last background invalidation. 0 means "never",
/// so the first background write invalidates immediately.
static LAST_BG_INVALIDATION_MS: AtomicU64 = AtomicU64::new(0);

/// Mark the cached view stale: advance the epoch and keep the wall-clock
/// identity current. Does NOT touch the invalidation counter, so prefetch
/// cache keys stay stable across background writes that fall inside the
/// throttle window.
pub(crate) fn mark_tree_dirty() {
    TREE_EPOCH.fetch_add(1, Ordering::SeqCst);
    // Concern (b): keep the wall-clock identity current for table names/expiry.
    VERSION_COUNT_TIMESTAMP.fetch_max(Utc::now().timestamp_millis(), Ordering::SeqCst);
}

/// Full bump: dirty the tree AND advance the invalidation counter. Used by
/// request-path commits and scan terminal transitions; never throttled.
pub(crate) fn bump_tree_epoch() {
    mark_tree_dirty();
    // Concern (a): strictly monotonic invalidation version for cache keys.
    INVALIDATION_VERSION.fetch_add(1, Ordering::SeqCst);
    // Keep the background throttle window aligned with this invalidation.
    LAST_BG_INVALIDATION_MS.store(bg_now_ms(), Ordering::SeqCst);
}

/// Background write entry point: always dirty the tree, advance the
/// invalidation counter only on the leading edge of a ~1 s quiet window.
pub(crate) fn mark_tree_dirty_background() {
    mark_tree_dirty();
    let now = bg_now_ms();
    // Leading-edge throttle: invalidate only if at least ~1 s elapsed since
    // the last background (or full) invalidation.
    let mut last = LAST_BG_INVALIDATION_MS.load(Ordering::SeqCst);
    loop {
        if now.saturating_sub(last) < 1000 {
            break;
        }
        match LAST_BG_INVALIDATION_MS.compare_exchange(
            last,
            now,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => {
                INVALIDATION_VERSION.fetch_add(1, Ordering::SeqCst);
                break;
            }
            Err(actual) => last = actual,
        }
    }
}

fn bg_now_ms() -> u64 {
    #[cfg(test)]
    {
        let override_now = BG_INVALIDATION_NOW_OVERRIDE_MS.load(Ordering::SeqCst);
        if override_now != 0 {
            return override_now;
        }
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0u64, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
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

// Test-only seam for the background throttle clock.
#[cfg(test)]
static BG_INVALIDATION_NOW_OVERRIDE_MS: AtomicU64 = AtomicU64::new(0);

/// Reset background throttle state. Test-only.
#[cfg(test)]
pub(crate) fn reset_bg_invalidation_state() {
    LAST_BG_INVALIDATION_MS.store(0, Ordering::SeqCst);
    BG_INVALIDATION_NOW_OVERRIDE_MS.store(0, Ordering::SeqCst);
}

/// Set the clock used by the background throttle. 0 restores real time.
/// Test-only.
#[cfg(test)]
pub(crate) fn set_bg_invalidation_now_ms(ms: u64) {
    BG_INVALIDATION_NOW_OVERRIDE_MS.store(ms, Ordering::SeqCst);
}

/// Snapshot of the request-visible invalidation counter. Test-only, for
/// contract tests that assert background writes do not advance it.
#[cfg(test)]
pub(crate) fn bg_invalidation_version_for_test() -> u64 {
    INVALIDATION_VERSION.load(Ordering::SeqCst)
}

/// Snapshot of the tree dirtiness epoch. Test-only.
#[cfg(test)]
pub(crate) fn tree_epoch_for_test() -> u64 {
    TREE_EPOCH.load(Ordering::SeqCst)
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

    #[test]
    fn mark_tree_dirty_does_not_advance_invalidation_version() {
        let before = INVALIDATION_VERSION.load(Ordering::SeqCst);
        mark_tree_dirty();
        let after = INVALIDATION_VERSION.load(Ordering::SeqCst);
        assert_eq!(
            before, after,
            "dirty-only must not advance the invalidation counter"
        );
    }

    #[test]
    fn background_notes_throttle_invalidation() {
        reset_bg_invalidation_state();
        let before = INVALIDATION_VERSION.load(Ordering::SeqCst);

        set_bg_invalidation_now_ms(1000);
        for _ in 0..10 {
            mark_tree_dirty_background();
        }
        // leading edge at 1000: exactly one invalidation so far
        let mid = INVALIDATION_VERSION.load(Ordering::SeqCst);
        assert!(
            mid - before <= 1,
            "rapid background notes within one window must not each invalidate: before={before}, mid={mid}"
        );

        // after ~1 s quiet window, the next note invalidates again
        set_bg_invalidation_now_ms(2000);
        mark_tree_dirty_background();
        let after_second = INVALIDATION_VERSION.load(Ordering::SeqCst);
        assert!(
            after_second > mid,
            "note after the window must invalidate: mid={mid}, after={after_second}"
        );

        // a note 500 ms later is throttled
        set_bg_invalidation_now_ms(2500);
        mark_tree_dirty_background();
        let final_v = INVALIDATION_VERSION.load(Ordering::SeqCst);
        assert_eq!(
            final_v, after_second,
            "note inside the new window must be throttled: after_second={after_second}, final={final_v}"
        );

        let total = final_v - before;
        assert!(
            total <= 2,
            "N rapid background notes must advance invalidation <=2 times: total={total}"
        );
    }
}
