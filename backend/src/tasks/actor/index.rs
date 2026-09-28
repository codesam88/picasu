use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use log::{debug, error, info};
use tokio_rayon::AsyncThreadPool;

use crate::tasks::runtime::WORKER_RAYON_POOL;
use crate::{
    error::handle_error,
    model::abstract_data::AbstractData,
    process::index::process_media_info,
    tasks::{BATCH_COORDINATOR, batcher::flush_tree::FlushTreeTask},
};
use mini_executor::Task;

pub struct IndexTask {
    pub abstract_data: AbstractData,
}

impl IndexTask {
    pub fn new(abstract_data: AbstractData) -> Self {
        Self { abstract_data }
    }
}

impl Task for IndexTask {
    type Output = Result<AbstractData>;

    async fn run(self) -> Self::Output {
        WORKER_RAYON_POOL
            .spawn_async(move || index_task_match(self.abstract_data))
            .await
            .map_err(|err| handle_error(err.context("Failed to run index task")))
    }
}

fn index_task_match(abstract_data: AbstractData) -> Result<AbstractData> {
    let hash = abstract_data.hash();
    match index_task(abstract_data) {
        Ok(data) => {
            info!("indexed {hash}");
            Ok(data)
        }
        Err(e) => {
            error!("indexing failed {hash}: {e:#}");
            Err(e)
        }
    }
}

fn index_task(mut abstract_data: AbstractData) -> Result<AbstractData> {
    let hash = abstract_data.hash();
    // Path-primary records hold a single file entry.
    let path = abstract_data
        .path()
        .map(|entry| entry.file.clone())
        .ok_or_else(|| anyhow!("record has no path for hash: {hash}"))?;

    if !matches!(abstract_data.ext_type(), "image" | "video") {
        return Err(anyhow!(
            "unsupported file type: {}",
            abstract_data.ext_type()
        ));
    }

    info!("indexing {} {hash}: {path}", abstract_data.ext_type());

    let is_image = abstract_data.is_image();
    // One shared pipeline for both orchestration modes: the incremental index
    // here, the filesystem rebuild in `crate::process::rebuild`. Only the
    // orchestration around it differs.
    if let Err(e) = process_media_info(&mut abstract_data) {
        debug!(
            "Failed {} data dump: {abstract_data:#?}",
            abstract_data.ext_type()
        );
        return Err(e).context(format!(
            "failed to process {} metadata pipeline. Hash: {}, Path: {}",
            abstract_data.ext_type(),
            abstract_data.hash(),
            path
        ));
    }
    if !is_image {
        abstract_data.set_pending(true);
    }

    BATCH_COORDINATOR.execute_batch_detached(FlushTreeTask::insert(vec![abstract_data.clone()]));

    Ok(abstract_data)
}
