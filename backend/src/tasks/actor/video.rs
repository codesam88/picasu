use crate::{
    model::abstract_data::AbstractData,
    process::video::generate_compressed_video,
    tasks::{BATCH_COORDINATOR, batcher::flush_tree::FlushTreeTask},
};
use anyhow::{Context, Result};
use log::debug;

pub fn video_task(mut abstract_data: AbstractData) -> Result<()> {
    let hash = abstract_data.hash();
    match generate_compressed_video(&abstract_data) {
        Ok(()) => {
            abstract_data.set_pending(false);
            BATCH_COORDINATOR
                .execute_batch_detached(FlushTreeTask::insert(vec![abstract_data.clone()]));
            debug!("transcoded {hash}");
        }
        Err(err) => Err(err).context(format!(
            "video_task: video compression failed for hash: {hash}"
        ))?,
    }
    Ok(())
}
