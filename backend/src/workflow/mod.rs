use crate::process::dir_album::{get_album_id_for_dir, get_or_create_dir_album};
use crate::storage::files::get_resolved_image_home;
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
use std::{path::Path, sync::LazyLock};

static IN_PROGRESS: LazyLock<DashSet<String>> = LazyLock::new(DashSet::new);

pub struct ProcessingGuard(String);
impl Drop for ProcessingGuard {
    fn drop(&mut self) {
        IN_PROGRESS.remove(&self.0);
    }
}

fn try_acquire(path_key: String) -> Option<ProcessingGuard> {
    if IN_PROGRESS.insert(path_key.clone()) {
        Some(ProcessingGuard(path_key))
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
    let image_root = get_resolved_image_home()?;

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

/// Index a single image file.
///
/// `src` — path relative to `IMAGE_HOME`.
/// `dst` — optional target album directory path (also relative to `IMAGE_HOME`).
///         If provided, the file is recorded under that album; otherwise the
///         album is resolved from `src`'s parent directory.
///
/// If the content hash is already known, the file still gets its own
/// path-primary record — records are never merged. Duplicate content is
/// grouped via `DUPE_INDEX` only.
pub async fn index_image(src: &Path, dst: Option<&Path>) -> Result<()> {
    let image_root =
        get_resolved_image_home().ok_or_else(|| anyhow::anyhow!("IMAGE_HOME not configured"))?;

    let path = image_root.join(src).clean();

    let dst_album_id = match dst {
        Some(dst_path) => {
            let abs_dst = image_root.join(dst_path);
            Some(
                tokio::task::spawn_blocking(move || get_or_create_dir_album(abs_dst))
                    .await?
                    .map_err(|e| anyhow::anyhow!("Failed to ensure dst album: {e}"))?,
            )
        }
        None => None,
    };

    let already_known_album_id = path.parent().and_then(get_album_id_for_dir);
    let resolved_dir_album_id = match already_known_album_id {
        Some(id) => Some(id),
        None => ensure_dir_albums(&path).await,
    };
    let album_id_opt = dst_album_id.or(resolved_dir_album_id);

    let file = INDEX_COORDINATOR
        .execute_waiting(OpenFileTask::new(path.clone()))
        .await??;

    let hash = INDEX_COORDINATOR
        .execute_waiting(HashTask::new(file))
        .await??;

    let Some(_guard) = try_acquire(path.to_string_lossy().into_owned()) else {
        warn!(
            "Processing already in progress for path: {}",
            path.display()
        );
        return Ok(());
    };

    let abstract_data_opt = INDEX_COORDINATOR
        .execute_waiting(DeduplicateTask::new(path.clone(), hash, album_id_opt))
        .await??;

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
