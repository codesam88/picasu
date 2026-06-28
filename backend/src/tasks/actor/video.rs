use crate::{
    error::handle_error,
    model::abstract_data::AbstractData,
    process::video::generate_compressed_video,
    tasks::runtime::WORKER_RAYON_POOL,
    tasks::{BATCH_COORDINATOR, batcher::flush_tree::FlushTreeTask},
};
use anyhow::{Context, Result};
use log::info;
use mini_executor::Task;
use tokio_rayon::AsyncThreadPool;

pub struct VideoTask {
    abstract_data: AbstractData,
}

impl VideoTask {
    pub fn new(abstract_data: AbstractData) -> Self {
        Self { abstract_data }
    }
}

impl Task for VideoTask {
    type Output = Result<()>;

    async fn run(self) -> Self::Output {
        WORKER_RAYON_POOL
            .spawn_async(move || video_task(self.abstract_data))
            .await
            .map_err(|err| handle_error(err.context("Failed to run video task")))
    }
}

pub fn video_task(mut abstract_data: AbstractData) -> Result<()> {
    let hash = abstract_data.hash();
    match generate_compressed_video(&abstract_data) {
        Ok(()) => {
            abstract_data.set_pending(false);
            BATCH_COORDINATOR
                .execute_batch_detached(FlushTreeTask::insert(vec![abstract_data.clone()]));
            info!("transcoded {hash}");
        }
        Err(err) => Err(err).context(format!(
            "video_task: video compression failed for hash: {hash}"
        ))?,
    }
    Ok(())
}
