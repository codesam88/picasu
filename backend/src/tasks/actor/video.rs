use crate::{model::abstract_data::AbstractData, process::video::generate_compressed_video};
use anyhow::Result;
use log::debug;

/// Transcode the compressed form for one video asset. Returns the updated
/// record plus the transcode result; the caller flushes the record through
/// the serial writer on success only (a failed transcode leaves no new
/// state to publish, matching the previous detached-flush behavior).
pub fn video_task(mut abstract_data: AbstractData) -> (AbstractData, Result<()>) {
    let hash = abstract_data.hash();
    match generate_compressed_video(&abstract_data) {
        Ok(()) => {
            abstract_data.set_pending(false);
            debug!("transcoded {hash}");
            (abstract_data, Ok(()))
        }
        Err(err) => {
            let err = err.context(format!(
                "video_task: video compression failed for hash: {hash}"
            ));
            (abstract_data, Err(err))
        }
    }
}
