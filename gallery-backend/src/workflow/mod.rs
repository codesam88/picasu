use crate::operations::dir_album::{get_album_id_for_dir, get_or_create_dir_album};
use crate::operations::utils::image_path::get_resolved_image_path;
use crate::tasks::{
    INDEX_COORDINATOR,
    actor::{
        deduplicate::DeduplicateTask, hash::HashTask, index::IndexTask, open_file::OpenFileTask,
        video::VideoTask,
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

/// Ensure directory albums exist for every directory level between the
/// configured image root and the file's parent, returning the deepest one
/// (i.e. the album for the file's immediate parent directory), or `None` if
/// the file isn't under the configured image root or sits directly in it
/// (no sub-directory to album-map).
async fn ensure_dir_albums(file_path: &std::path::Path) -> Option<ArrayString<64>> {
    let image_root = get_resolved_image_path()?;

    let file_dir = file_path.parent()?;

    if !file_dir.starts_with(&image_root) {
        return None;
    }

    // Files directly in the image root have no sub-directory to album-map.
    if file_dir == image_root {
        return None;
    }

    let relative = file_dir.strip_prefix(&image_root).ok()?;

    let mut current = image_root.clone();
    let mut deepest_album_id = None;
    for component in relative.components() {
        current.push(component);
        let dir_for_closure = current.clone();
        match tokio::task::spawn_blocking(move || get_or_create_dir_album(dir_for_closure)).await {
            Ok(Ok(id)) => deepest_album_id = Some(id),
            Ok(Err(e)) => warn!("Failed to ensure dir album for {}: {e}", current.display()),
            Err(e) => warn!("spawn_blocking failed in ensure_dir_albums: {e}"),
        }
    }
    deepest_album_id
}

pub async fn index_for_watch(
    path: PathBuf,
    presigned_album_id_opt: Option<ArrayString<64>>,
) -> Result<()> {
    let path = path.clean();

    // Resolve the dir-album for the file's immediate parent directory —
    // unless an explicit (presigned) album was given, e.g. by an upload
    // targeting a specific album, which takes priority. Check the cache
    // first (the directory may already be a known dir-album, e.g. loaded
    // at startup or created some other way) before falling back to
    // `ensure_dir_albums`'s sync-root walk, which also creates any missing
    // intermediate albums for a brand-new directory.
    let already_known_album_id = path.parent().and_then(get_album_id_for_dir);
    let resolved_dir_album_id = match already_known_album_id {
        Some(id) => Some(id),
        None => ensure_dir_albums(&path).await,
    };
    let album_id_opt = presigned_album_id_opt.or(resolved_dir_album_id);

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
        .execute_waiting(DeduplicateTask::new(path.clone(), hash, album_id_opt))
        .await??;

    // If the file is already in the database, we can skip further processing.
    let Some(mut abstract_data) = abstract_data_opt else {
        return Ok(());
    };

    abstract_data = INDEX_COORDINATOR
        .execute_waiting(IndexTask::new(abstract_data))
        .await??;

    if abstract_data.is_video() {
        INDEX_COORDINATOR
            .execute_waiting(VideoTask::new(abstract_data))
            .await??;
    }

    Ok(())
}
