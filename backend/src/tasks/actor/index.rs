use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use log::{debug, error, info};
use tokio_rayon::AsyncThreadPool;

use crate::tasks::runtime::WORKER_RAYON_POOL;
use crate::{
    error::handle_error,
    model::abstract_data::AbstractData,
    process::index::{process_image_info, process_video_info},
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
    let newest_path = abstract_data
        .path()
        .iter()
        .max()
        .ok_or_else(|| anyhow!("record has no canonical path for hash: {hash}"))?
        .file
        .clone();

    if !matches!(abstract_data.ext_type(), "image" | "video") {
        return Err(anyhow!(
            "unsupported file type: {}",
            abstract_data.ext_type()
        ));
    }

    info!(
        "indexing {} {hash}: {newest_path}",
        abstract_data.ext_type()
    );

    let is_image = abstract_data.is_image();
    if is_image {
        if let Err(e) = process_image_info(&mut abstract_data) {
            debug!("Failed image data dump: {abstract_data:#?}");
            return Err(e).context(format!(
                "failed to process image metadata pipeline. Hash: {}, Path: {}",
                abstract_data.hash(),
                newest_path
            ));
        }
    } else {
        if let Err(e) = process_video_info(&mut abstract_data) {
            debug!("Failed video data dump: {abstract_data:#?}");
            return Err(e).context(format!(
                "failed to process video metadata pipeline. Hash: {}, Path: {}",
                abstract_data.hash(),
                newest_path
            ));
        }
        abstract_data.set_pending(true);
    }

    BATCH_COORDINATOR.execute_batch_detached(FlushTreeTask::insert(vec![abstract_data.clone()]));

    Ok(abstract_data)
}
