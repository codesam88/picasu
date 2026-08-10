use crate::process::dir_album::get_or_create_dir_album;
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
async fn ensure_dir_albums(
    namespace: &str,
    file_path: &std::path::Path,
) -> Option<ArrayString<64>> {
    let file_dir = file_path.parent()?;

    // Files directly in the namespace root have no sub-directory to album-map.
    if file_dir.as_os_str().is_empty() || file_dir == Path::new(".") {
        return None;
    }

    let mut current = std::path::PathBuf::new();
    let mut deepest_album_id = None;
    for component in file_dir.components() {
        current.push(component);
        let ns = namespace.to_string();
        let dir_for_closure = current.clone();
        match tokio::task::spawn_blocking(move || {
            let abs_dir = crate::process::namespace::namespace_resolve(
                &ns,
                &dir_for_closure.to_string_lossy(),
            )?;
            get_or_create_dir_album(abs_dir, &ns).ok()
        })
        .await
        {
            Ok(Some(id)) => deepest_album_id = Some(id),
            Ok(None) => {}
            Err(e) => warn!("spawn_blocking failed in ensure_dir_albums: {e}"),
        }
    }
    deepest_album_id
}

/// Index a single image file.
///
/// `src` — path relative to the namespace root.
/// `dst` — optional target album directory path (also relative to namespace root).
///         If provided, the file is recorded under that album; otherwise the
///         album is resolved from `src`'s parent directory.
///
/// If the content hash is already known, only the alias list is merged — no
/// metadata extraction or thumbnail regeneration is re-run.
pub async fn index_image(namespace: &str, src: &Path, dst: Option<&Path>) -> Result<()> {
    let relative_src = src.clean();

    let dst_album_id = match dst {
        Some(dst_path) => {
            let ns = namespace.to_string();
            let abs_dst = crate::process::namespace::namespace_resolve(
                namespace,
                &dst_path.to_string_lossy(),
            )
            .ok_or_else(|| anyhow::anyhow!("Namespace not found: {namespace}"))?;
            Some(
                tokio::task::spawn_blocking(move || get_or_create_dir_album(abs_dst, &ns))
                    .await?
                    .map_err(|e| anyhow::anyhow!("Failed to ensure dst album: {e}"))?,
            )
        }
        None => None,
    };

    let already_known_album_id = {
        let dir_str = relative_src
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Cache keys are absolute; resolve the relative parent dir first.
        crate::process::namespace::namespace_resolve(namespace, &dir_str)
            .as_deref()
            .and_then(crate::process::dir_album::get_album_id_for_dir)
    };
    let resolved_dir_album_id = match already_known_album_id {
        Some(id) => Some(id),
        None => ensure_dir_albums(namespace, &relative_src).await,
    };
    let album_id_opt = dst_album_id.or(resolved_dir_album_id);

    let abs_path =
        crate::process::namespace::namespace_resolve(namespace, &relative_src.to_string_lossy())
            .ok_or_else(|| anyhow::anyhow!("Namespace not found: {namespace}"))?;

    let file = INDEX_COORDINATOR
        .execute_waiting(OpenFileTask::new(abs_path))
        .await??;

    let hash = INDEX_COORDINATOR
        .execute_waiting(HashTask::new(file))
        .await??;

    let Some(_guard) = try_acquire(hash) else {
        warn!(
            "Processing already in progress for path: {}, hash: {hash}",
            relative_src.display()
        );
        return Ok(());
    };

    let abstract_data_opt = INDEX_COORDINATOR
        .execute_waiting(DeduplicateTask::new(
            namespace.to_string(),
            relative_src.clone(),
            hash,
            album_id_opt,
        ))
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
