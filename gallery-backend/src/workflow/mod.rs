use crate::operations::dir_album::get_or_create_dir_album;
use crate::operations::utils::sync_paths::get_resolved_sync_paths;
use crate::public::structure::config::APP_CONFIG;
use crate::tasks::{
    INDEX_COORDINATOR,
    actor::{
        copy::CopyTask, deduplicate::DeduplicateTask, delete_in_update::DeleteTask, hash::HashTask,
        index::IndexTask, open_file::OpenFileTask, video::VideoTask,
    },
};
use anyhow::Result;
use arrayvec::ArrayString;
use dashmap::DashSet;
use log::warn;
use path_clean::PathClean;
use std::{path::PathBuf, sync::LazyLock};

static IN_PROGRESS: LazyLock<DashSet<ArrayString<64>>> = LazyLock::new(DashSet::new);

pub struct ProcessingGuard(ArrayString<64>);
impl Drop for ProcessingGuard {
    fn drop(&mut self) {
        IN_PROGRESS.remove(&self.0);
    }
}

fn try_acquire(hash: ArrayString<64>) -> Option<ProcessingGuard> {
    if IN_PROGRESS.insert(hash) {
        Some(ProcessingGuard(hash))
    } else {
        None
    }
}

/// Ensure directory albums exist for every directory level between the nearest
/// sync root and the file's parent.  Only runs when `album_paths_from_filesystem`
/// is enabled.
///
/// Does NOT return album IDs — directory album membership is path-based and
/// entirely independent from `img.metadata.albums` (the manual-assignment set).
async fn ensure_dir_albums(file_path: &std::path::Path) {
    let enabled = APP_CONFIG
        .get()
        .unwrap()
        .read()
        .unwrap()
        .public
        .album_paths_from_filesystem;

    if !enabled {
        return;
    }

    let sync_paths = get_resolved_sync_paths();

    let Some(file_dir) = file_path.parent() else {
        return;
    };

    let Some(sync_root) = sync_paths
        .iter()
        .find(|root| file_dir.starts_with(root.as_path()))
    else {
        return;
    };

    // Files directly in the sync root have no sub-directory to album-map.
    if file_dir == sync_root.as_path() {
        return;
    }

    let Ok(relative) = file_dir.strip_prefix(sync_root.as_path()) else {
        return;
    };

    let mut current = sync_root.clone();
    for component in relative.components() {
        current.push(component);
        let dir_for_closure = current.clone();
        match tokio::task::spawn_blocking(move || get_or_create_dir_album(dir_for_closure)).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => warn!("Failed to ensure dir album for {}: {e}", current.display()),
            Err(e) => warn!("spawn_blocking failed in ensure_dir_albums: {e}"),
        }
    }
}

pub async fn index_for_watch(
    path: PathBuf,
    presigned_album_id_opt: Option<ArrayString<64>>,
) -> Result<()> {
    let path = path.clean();

    // Ensure filesystem-hierarchy albums exist for this file's directory chain.
    // Album membership is path-based; no IDs are passed to DeduplicateTask.
    ensure_dir_albums(&path).await;

    let file = INDEX_COORDINATOR
        .execute_waiting(OpenFileTask::new(path.clone()))
        .await??;

    let hash = INDEX_COORDINATOR
        .execute_waiting(HashTask::new(file))
        .await??;

    let Some(_guard) = try_acquire(hash) else {
        warn!(
            "Processing already in progress for path: {}, hash: {hash}",
            path.display()
        );
        return Ok(());
    };

    let abstract_data_opt = INDEX_COORDINATOR
        .execute_waiting(DeduplicateTask::new(path.clone(), hash, presigned_album_id_opt))
        .await??;

    // If the file is already in the database, we can skip further processing.
    let Some(mut abstract_data) = abstract_data_opt else {
        INDEX_COORDINATOR.execute_detached(DeleteTask::new(path));
        return Ok(());
    };

    abstract_data = INDEX_COORDINATOR
        .execute_waiting(CopyTask::new(abstract_data))
        .await??;
    abstract_data = INDEX_COORDINATOR
        .execute_waiting(IndexTask::new(abstract_data))
        .await??;

    INDEX_COORDINATOR.execute_detached(DeleteTask::new(PathBuf::from(&path)));
    if abstract_data.is_video() {
        INDEX_COORDINATOR
            .execute_waiting(VideoTask::new(abstract_data))
            .await??;
    }

    Ok(())
}
