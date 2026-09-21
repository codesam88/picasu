use crate::{error::handle_error, model::abstract_data::AbstractData};
use anyhow::Result;
use arrayvec::ArrayString;
use mini_executor::Task;
use std::path::PathBuf;
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

    if let Some(album_id) = task.presigned_album_id {
        abstract_data.set_album(Some(album_id));
    } else if let Some(parent) = task.path.parent() {
        // For filesystem-indexed photos, set the album to the parent
        // directory's dir-album so album-contents page filters match.
        if let Ok(album_id) =
            crate::process::dir_album::get_or_create_dir_album(parent.to_path_buf())
        {
            abstract_data.set_album(Some(album_id));
        }
    }

    // Path-primary model: each physical file gets its own record.
    // Same-hash files are tracked via DUPE_INDEX, never merged into one record.
    // Always return Some so IndexTask processes this file independently.
    Ok(Some(abstract_data))
}
