use crate::storage::db::open_data_table;
use crate::tasks::{BATCH_COORDINATOR, batcher::flush_tree::FlushTreeTask};
use crate::{error::handle_error, model::abstract_data::AbstractData};
use anyhow::Result;
use arrayvec::ArrayString;
use mini_executor::Task;
use std::{mem, path::Path, path::PathBuf};
use tokio::task::spawn_blocking;

pub struct DeduplicateTask {
    pub path: PathBuf,
    pub hash: ArrayString<64>,
    pub presigned_album_id: Option<ArrayString<64>>,
}

impl DeduplicateTask {
    pub fn new(
        path: PathBuf,
        hash: ArrayString<64>,
        presigned_album_id: Option<ArrayString<64>>,
    ) -> Self {
        Self {
            path,
            hash,
            presigned_album_id,
        }
    }
}

impl Task for DeduplicateTask {
    type Output = Result<Option<AbstractData>>;

    async fn run(self) -> Self::Output {
        spawn_blocking(move || deduplicate_task(&self))
            .await
            .expect("blocking task panicked")
            .map_err(|err| handle_error(err.context("Failed to run deduplicate task")))
    }
}

fn deduplicate_task(task: &DeduplicateTask) -> Result<Option<AbstractData>> {
    let mut abstract_data = AbstractData::new(&task.path, task.hash)?;

    let data_table = open_data_table();

    if let Some(guard) = data_table.get(&*task.hash).expect("failed to get record") {
        let mut data_exist = guard.value();
        if let Some(alias_mut) = abstract_data.alias_mut() {
            let file_modify = mem::take(&mut alias_mut[0]);
            if let Some(exist_alias) = data_exist.alias_mut() {
                // Drop any recorded alias whose file no longer exists on
                // disk. With no content-addressed copy backing each alias
                // (see TODO.md "Storage architecture fix"), a missing
                // aliased file is a real file/DB inconsistency -- external
                // manipulation or data loss outside the app, e.g. the file
                // was moved/deleted without going through assign_album --
                // so surface it instead of pruning silently.
                let (still_present, missing): (Vec<_>, Vec<_>) = mem::take(exist_alias)
                    .into_iter()
                    .partition(|a| Path::new(&a.file).exists());
                if !missing.is_empty() {
                    warn!(
                        "Pruning {} missing alias path(s) for hash {}, no longer on disk: {:?}",
                        missing.len(),
                        task.hash,
                        missing.iter().map(|a| &a.file).collect::<Vec<_>>()
                    );
                }
                *exist_alias = still_present;

                // Add the current path only if it isn't already present --
                // rediscovering the same file at its current, unchanged
                // path on every watcher re-index is routine, not a
                // duplicate. A genuinely new path for already-known
                // content is real duplicate-content detection, worth
                // calling out distinctly.
                if !exist_alias.iter().any(|a| a.file == file_modify.file) {
                    warn!(
                        "Duplicate content detected: {} has the same hash ({}) as already-indexed {:?}",
                        file_modify.file,
                        task.hash,
                        exist_alias.iter().map(|a| &a.file).collect::<Vec<_>>()
                    );
                    exist_alias.push(file_modify);
                }
            }
        }
        if let Some(album_id) = task.presigned_album_id {
            data_exist.set_album(Some(album_id));
        }

        BATCH_COORDINATOR.execute_batch_detached(FlushTreeTask::insert(vec![data_exist]));
        Ok(None)
    } else {
        if let Some(album_id) = task.presigned_album_id {
            abstract_data.set_album(Some(album_id));
        }
        Ok(Some(abstract_data))
    }
}
