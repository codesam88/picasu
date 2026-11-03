use crate::model::abstract_data::AbstractData;
use crate::process::dir_album::{get_album_id_for_dir, get_or_create_dir_album};
use crate::process::index::process_media_info;
use crate::storage::files::get_resolved_image_home;
use crate::storage::files::get_resolved_users_home;
use crate::tasks::{
    INDEX_COORDINATOR,
    actor::{
        deduplicate::DeduplicateTask, hash::HashTask, index::IndexTask, open_file::OpenFileTask,
        video::VideoTask,
    },
};
use anyhow::{Context, Result};
use arrayvec::ArrayString;
use log::warn;
use path_clean::PathClean;
use std::{fs::File, path::Path, path::PathBuf};

/// Ensure directory albums exist for every directory level between the
/// configured image root and the file's parent, returning the deepest one
/// (i.e. the album for the file's immediate parent directory), or `None` if
/// the file isn't under the configured image root or sits directly in it
/// (no sub-directory to album-map).
async fn ensure_dir_albums(file_path: &std::path::Path) -> Option<ArrayString<64>> {
    let file_dir = file_path.parent()?;

    let image_root = get_resolved_image_home().filter(|r| file_dir.starts_with(r));
    let root =
        image_root.or_else(|| get_resolved_users_home().filter(|r| file_dir.starts_with(r)))?;

    // Files directly in the root have no sub-directory to album-map.
    if file_dir == root {
        return None;
    }

    let relative = file_dir.strip_prefix(&root).ok()?;

    let mut current = root.clone();
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

/// Build a fully processed `AbstractData` for one media file: identity from what
/// the caller already resolved (path, content hash, album membership), and
/// metadata plus derived data from the shared [`process_media_info`] pipeline.
///
/// This is the rebuild side of that pipeline's two orchestration modes. The
/// incremental index (`index_image`) resolves identity through the
/// open/hash/deduplicate tasks and reaches the same [`process_media_info`] call
/// via [`IndexTask`]; the filesystem rebuild resolves identity from its own
/// filesystem walk and comes through here. What differs between the modes is
/// only how identity is obtained and how the result is persisted — the EXIF
/// read, the sidecar precedence rules and the hash/thumbnail derivation exist
/// once, so a rebuilt asset cannot be described by different rules than an
/// indexed one.
///
/// A video is marked `pending`: this pipeline produces its thumbnail, hashes and
/// EXIF, but not its compressed form, which is the separate [`VideoTask`]. That
/// matches what [`IndexTask`] does, so a rebuilt video carries the same state an
/// indexed one does; the compressed form is left to the normal video path.
#[allow(dead_code)]
pub fn index_media_file(
    path: &Path,
    hash: ArrayString<64>,
    album_id: Option<ArrayString<64>>,
) -> Result<AbstractData> {
    let mut data = AbstractData::new(path, hash)?;

    if let Some(id) = album_id {
        data.set_album(Some(id));
    }

    process_media_info(&mut data)
        .with_context(|| format!("metadata pipeline failed for {}", path.display()))?;

    if data.is_video() {
        data.set_pending(true);
    }

    Ok(data)
}

/// Index a single image file.
///
/// `src` — path relative to `IMAGE_HOME`. The album is resolved from `src`'s
/// parent directory.
///
/// Direct (inline) entry point for request-path and watcher callers. The
/// walk-driven scan instead enqueues via [`submit_index`] and lets intake
/// workers run [`process_file`].
///
/// If the content hash is already known, the file still gets its own
/// path-primary record — records are never merged. Duplicate content is
/// grouped via `DUPE_INDEX` only.
pub async fn index_image(src: &Path) -> Result<()> {
    let path = resolve_index_path(src)?;
    process_file(path).await
}

/// Resolve `src` (relative to `IMAGE_HOME`, or absolute) to a cleaned
/// absolute path under one of the configured roots.
fn resolve_index_path(src: &Path) -> Result<PathBuf> {
    let path = if src.is_absolute() {
        src.to_path_buf()
    } else {
        let image_root = get_resolved_image_home()
            .ok_or_else(|| anyhow::anyhow!("IMAGE_HOME not configured"))?;
        image_root.join(src)
    }
    .clean();

    // Accept files under either configured root.
    let under_image = get_resolved_image_home().is_some_and(|r| path.starts_with(&r));
    let under_users = get_resolved_users_home().is_some_and(|r| path.starts_with(&r));
    if !under_image && !under_users {
        return Err(anyhow::anyhow!("path outside configured roots"));
    }

    Ok(path)
}

/// Enqueue `src` for background indexing under scan job `job_id`.
///
/// Resolution stays identical to [`index_image`]; per-path coalescing and
/// the per-job completion barrier live in the intake.
pub async fn submit_index(src: &Path, job_id: u64) -> Result<()> {
    let path = resolve_index_path(src)?;
    crate::tasks::queue::submit_scan(path, job_id).await
}

/// Index one already-resolved file: Open → Hash → Dedup → Index → Video.
///
/// Each stage runs through the `INDEX_COORDINATOR` hops, which stay the
/// execution backend in this slice; only the per-file `tokio::spawn`
/// fan-out moved into the intake worker pool.
pub async fn process_file(path: PathBuf) -> Result<()> {
    let album_id_opt = match path.parent().and_then(get_album_id_for_dir) {
        Some(id) => Some(id),
        None => ensure_dir_albums(&path).await,
    };

    let file = stage_open(&path).await?;
    let hash = stage_hash(file).await?;

    let abstract_data_opt = stage_dedup(path, hash, album_id_opt).await?;
    let Some(mut abstract_data) = abstract_data_opt else {
        return Ok(());
    };

    abstract_data = stage_index(abstract_data).await?;

    if abstract_data.is_video() {
        stage_video(abstract_data).await?;
    }

    Ok(())
}

/// Open stage: read the file handle off the blocking pool.
async fn stage_open(path: &Path) -> Result<File> {
    INDEX_COORDINATOR
        .execute_waiting(OpenFileTask::new(path.to_path_buf()))
        .await?
}

/// Hash stage: content-hash the open file.
async fn stage_hash(file: File) -> Result<ArrayString<64>> {
    INDEX_COORDINATOR
        .execute_waiting(HashTask::new(file))
        .await?
}

/// Dedup stage: build the path-primary record for this file.
async fn stage_dedup(
    path: PathBuf,
    hash: ArrayString<64>,
    album_id_opt: Option<ArrayString<64>>,
) -> Result<Option<AbstractData>> {
    INDEX_COORDINATOR
        .execute_waiting(DeduplicateTask::new(path, hash, album_id_opt))
        .await?
}

/// Index stage: metadata pipeline plus thumbnail derivation.
async fn stage_index(abstract_data: AbstractData) -> Result<AbstractData> {
    INDEX_COORDINATOR
        .execute_waiting(IndexTask::new(abstract_data))
        .await?
}

/// Video stage: compressed form for video assets.
async fn stage_video(abstract_data: AbstractData) -> Result<()> {
    INDEX_COORDINATOR
        .execute_waiting(VideoTask::new(abstract_data))
        .await?
}
