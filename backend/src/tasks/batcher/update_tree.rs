use crate::model::response::DatabaseTimestamp;
use crate::process::dir_album::drain_pending_album_updates;
use crate::storage::db::TREE;
use crate::storage::db::open_data_table;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::actor::album::album_task;
use crate::tasks::batcher::update_expire::UpdateExpireTask;
use chrono::Utc;
use log::warn;
use mini_executor::BatchTask;
use rayon::iter::{ParallelBridge, ParallelIterator};
use rayon::prelude::ParallelSliceMut;
use redb::ReadableTable;
use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Instant;

static ALLOWED_KEYS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "Make",
        "Model",
        "FNumber",
        "ExposureTime",
        "FocalLength",
        "PhotographicSensitivity",
        "DateTimeOriginal",
        "duration",
        "rotation",
    ]
    .iter()
    .copied()
    .collect()
});

pub struct UpdateTreeTask;

impl BatchTask for UpdateTreeTask {
    async fn batch_run(_: Vec<Self>) {
        update_tree_task();

        // Run self-updates for any albums whose members were recently changed.
        let pending = drain_pending_album_updates();
        if !pending.is_empty() {
            for album_id in pending {
                if let Err(e) = tokio::task::spawn_blocking(move || album_task(album_id)).await {
                    warn!("Album self-update task panicked for {album_id}: {e}");
                }
            }
            // Refresh the in-memory tree so updated album stats are visible.
            update_tree_task();
        }
    }
}

fn update_tree_task() {
    let start_time = Instant::now();
    let data_table = open_data_table();

    let priority_list = vec!["DateTimeOriginal", "filename", "modified", "scan_time"];

    let mut database_timestamp_vec: Vec<DatabaseTimestamp> = data_table
        .iter()
        .expect("failed to iterate table")
        .par_bridge()
        .map(|guard| {
            let (_, value) = guard.expect("failed to read record");
            let mut abstract_data = value.value();
            // retain only necessary exif data used for query search
            if let Some(exif_vec) = abstract_data.exif_vec_mut() {
                exif_vec.retain(|k, _| ALLOWED_KEYS.contains(&k.as_str()));
            }
            DatabaseTimestamp::new(abstract_data, &priority_list)
        })
        .collect();

    database_timestamp_vec.par_sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    *TREE.in_memory.write().expect("lock poisoned") = database_timestamp_vec;

    BATCH_COORDINATOR.execute_batch_detached(UpdateExpireTask);

    let current_timestamp = Utc::now().timestamp_millis();
    let duration = format!("{:?}", start_time.elapsed());
    info!(duration = &*duration; "In-memory cache updated ({}).", current_timestamp);
}
