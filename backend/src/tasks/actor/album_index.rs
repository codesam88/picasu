use chrono::Utc;
use log::{debug, info, warn};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tokio::task::JoinHandle;
use walkdir::{DirEntry, WalkDir};

use crate::error::{AppError, ErrorKind, handle_error};
use crate::model::media::{MediaOutcome, SkipReason, classify_media_file};
use crate::router::AppResult;
use crate::storage::db::TREE;
use crate::storage::files::get_data_path;
use crate::storage::files::get_resolved_image_home;
use crate::storage::files::get_resolved_users_home;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::flush_tree::FlushTreeTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use crate::tasks::runtime::INDEX_RUNTIME;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub enum AlbumIndexState {
    Idle,
    Running,
    Completed,
    Canceled,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct AlbumIndexStatus {
    pub state: AlbumIndexState,
    pub root: Option<String>,
    pub scanned: u64,
    pub matched: u64,
    pub processed: u64,
    pub failed: u64,
    /// Paths of files that failed to index, capped to a bounded size.
    #[serde(default)]
    pub failed_paths: Vec<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub cancel_requested: bool,
}

impl Default for AlbumIndexStatus {
    fn default() -> Self {
        Self {
            state: AlbumIndexState::Idle,
            root: None,
            scanned: 0,
            matched: 0,
            processed: 0,
            failed: 0,
            failed_paths: Vec::new(),
            started_at: None,
            finished_at: None,
            cancel_requested: false,
        }
    }
}

#[derive(Clone)]
struct ActiveIndex {
    job_id: u64,
    cancel: Arc<AtomicBool>,
}

struct IndexStatusSlot {
    job_id: u64,
    status: AlbumIndexStatus,
}

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);
static INDEX_STATUS: LazyLock<Mutex<IndexStatusSlot>> = LazyLock::new(|| {
    Mutex::new(IndexStatusSlot {
        job_id: 0,
        status: AlbumIndexStatus::default(),
    })
});
static ACTIVE_INDEX: LazyLock<Mutex<Option<ActiveIndex>>> = LazyLock::new(|| Mutex::new(None));

/// Drop all asset tables and clear the in-memory tree and directory-album
/// cache, so a following `index_album` rebuilds from an empty state instead
/// of merging into stale rows. Used by the `clear` flag of
/// `POST /post/index/album`.
pub fn clear_asset_tables_and_caches() -> AppResult<()> {
    crate::storage::db::clear_all_asset_tables_drop()
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("{e:#}")))?;
    crate::storage::db::TREE
        .in_memory
        .write()
        .expect("TREE in_memory lock")
        .clear();
    crate::process::dir_album::reset_dir_album_cache();
    Ok(())
}

/// Index all media files under `src` (a directory path relative to
/// `IMAGE_HOME`).  Walks recursively, calling `index_image` on each valid
/// media file.  Album is always resolved from the file's parent directory.
///
/// Runs as a background job; status can be polled via `album_index_status()`.
/// Index all media files under `src`, optionally clearing first and/or
/// selecting a scan root (`"global"`, `"personal"`, or `"auto"` preserving
/// the historical image-then-users probe order).
#[allow(clippy::too_many_lines)]
pub fn index_album_with_root(src: &str, clear: bool, root: Option<&str>) -> AppResult<()> {
    // Validate selector up front.
    if let Some(r) = root {
        match r {
            "global" | "personal" | "auto" => {}
            _ => {
                return Err(AppError::new(
                    ErrorKind::InvalidInput,
                    format!("Unknown scan root selector: {r}"),
                ));
            }
        }
    }

    let image_root = get_resolved_image_home()
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "No imagePath configured to scan"))?;

    let scan_roots = resolve_scan_roots(src, &image_root, root)?;

    let internal_roots = internal_subtree_roots();
    for root in &scan_roots {
        if is_inside_internal_subtree(root, &internal_roots) {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                "Cannot index Picasu internal data directories",
            ));
        }
    }

    let job_id = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
    let cancel = Arc::new(AtomicBool::new(false));

    {
        let mut slot = INDEX_STATUS.lock().expect("lock poisoned");
        if slot.status.state == AlbumIndexState::Running {
            return Err(AppError::new(
                ErrorKind::Conflict,
                "An index job is already running",
            ));
        }

        slot.job_id = job_id;
        let root_display = src.to_string();
        slot.status = AlbumIndexStatus {
            state: AlbumIndexState::Running,
            root: Some(root_display),
            scanned: 0,
            matched: 0,
            processed: 0,
            failed: 0,
            failed_paths: Vec::new(),
            started_at: Some(Utc::now().timestamp_millis()),
            finished_at: None,
            cancel_requested: false,
        };
    }

    *ACTIVE_INDEX.lock().expect("lock poisoned") = Some(ActiveIndex {
        job_id,
        cancel: cancel.clone(),
    });

    if clear {
        clear_asset_tables_and_caches()?;
    }

    let scan_roots_clone = scan_roots.clone();
    INDEX_RUNTIME.spawn(async move {
        info!(
            "Starting one-time album index: {} root(s)",
            scan_roots_clone.len()
        );

        // First, ensure a dir-album exists for every directory strictly under
        // each scan root, parent-first. This covers empty directories that the
        // file walk below would otherwise skip.
        let mut dir_paths: Vec<PathBuf> = Vec::new();
        for scan_root in &scan_roots_clone {
            for entry in WalkDir::new(scan_root)
                .follow_links(false)
                .into_iter()
                .filter_entry(|entry| should_walk_entry(entry, &internal_roots))
            {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(err) => {
                        warn!("Album index walk error: {err}");
                        increment_failed(job_id, None);
                        continue;
                    }
                };
                if entry.file_type().is_dir() && entry.depth() > 0 {
                    dir_paths.push(entry.into_path());
                }
            }
        }

        for dir in &dir_paths {
            if cancel.load(Ordering::SeqCst) {
                break;
            }
            let dir_clone = dir.clone();
            match tokio::task::spawn_blocking(move || {
                crate::process::dir_album::get_or_create_dir_album(dir_clone)
            })
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(err)) => warn!("Failed to ensure dir album for {}: {err}", dir.display()),
                Err(err) => warn!("spawn_blocking failed in dir album pass: {err}"),
            }
        }

        let mut handles: Vec<JoinHandle<()>> = Vec::new();
        'files: for scan_root in &scan_roots_clone {
            let walker = WalkDir::new(scan_root)
                .follow_links(false)
                .into_iter()
                .filter_entry(|entry| should_walk_entry(entry, &internal_roots));

            for entry in walker {
                if cancel.load(Ordering::SeqCst) {
                    break 'files;
                }

                let entry = match entry {
                    Ok(entry) => entry,
                    Err(err) => {
                        warn!("Album index walk error: {err}");
                        increment_failed(job_id, None);
                        continue;
                    }
                };

                if !entry.file_type().is_file() {
                    continue;
                }

                increment_scanned(job_id);

                let abs_path = entry.into_path();
                if let MediaOutcome::Skip(reason) = classify_media_file(&abs_path) {
                    if !matches!(reason, SkipReason::UnsupportedExtension) {
                        info!(
                            "Ignoring unrecognized file {}: {reason:?}",
                            abs_path.display()
                        );
                    }
                    continue;
                }

                increment_matched(job_id);

                handles.push(tokio::spawn(async move {
                    match crate::workflow::index_image(&abs_path).await {
                        Ok(()) => increment_processed(job_id),
                        Err(err) => {
                            handle_error(err);
                            increment_failed(job_id, Some(abs_path.display().to_string()));
                        }
                    }
                }));
            }
        }

        debug!(
            "walk done, joining {} file handles (job {job_id})",
            handles.len()
        );
        for handle in handles {
            if let Err(err) = handle.await {
                warn!("Album index file task join error: {err}");
                increment_failed(job_id, None);
            }
        }
        debug!("file handles joined (job {job_id})");

        // Drain the per-file FlushTreeTask inserts so the in-memory tree
        // reflects all walk results before the sweep reads it.
        debug!("BATCH flush_tree pre-sweep (job {job_id})");
        let _ = BATCH_COORDINATOR
            .execute_batch_waiting(FlushTreeTask::insert(vec![]))
            .await;
        debug!("BATCH flush_tree pre-sweep done (job {job_id})");

        // Sweep stale asset paths: remove DB records whose asset file no
        // longer exists on disk under each target root.
        for scan_root in &scan_roots_clone {
            debug!(
                "sweeping stale asset paths under {} (job {job_id})",
                scan_root.display()
            );
            sweep_stale_asset_paths(scan_root);
            debug!("stale asset path sweep done (job {job_id})");
        }

        // The users_home root itself must never carry an album record.
        if let Some(users_root) = get_resolved_users_home() {
            prune_users_root_album(&users_root);
        }

        // Drain the sweep's detached remove/update batches, then
        // UpdateTreeTask for album metadata.
        debug!("BATCH flush_tree post-sweep (job {job_id})");
        let _ = BATCH_COORDINATOR
            .execute_batch_waiting(FlushTreeTask::insert(vec![]))
            .await;
        debug!("BATCH update_tree (job {job_id})");
        let _ = BATCH_COORDINATOR
            .execute_batch_waiting(UpdateTreeTask)
            .await;
        debug!("BATCH drained (job {job_id})");

        let state = if cancel.load(Ordering::SeqCst) {
            AlbumIndexState::Canceled
        } else if did_every_matched_file_fail(job_id) {
            AlbumIndexState::Failed
        } else {
            AlbumIndexState::Completed
        };

        debug!("finishing job {job_id} state={state:?}");
        finish_job(job_id, state);
        debug!("job {job_id} finished");
    });

    Ok(())
}

pub fn cancel_album_index() -> AppResult<()> {
    let active = ACTIVE_INDEX.lock().expect("lock poisoned").clone();
    let Some(active) = active else {
        return Err(AppError::new(ErrorKind::NotFound, "No active index job"));
    };

    active.cancel.store(true, Ordering::SeqCst);
    let mut slot = INDEX_STATUS.lock().expect("lock poisoned");
    if slot.job_id == active.job_id && slot.status.state == AlbumIndexState::Running {
        slot.status.cancel_requested = true;
    }

    Ok(())
}

pub fn album_index_status() -> AlbumIndexStatus {
    INDEX_STATUS.lock().expect("lock poisoned").status.clone()
}

/// Remove any album record whose directory is the `users_home` root itself.
/// The root must never gain an album record; this prunes one if a prior
/// (buggy or pre-existing) state created it.
fn prune_users_root_album(users_root: &Path) {
    use crate::process::dir_album;
    use crate::storage::db::METADATA_TABLE;

    let users_root_str = users_root.to_string_lossy().to_string();
    let Ok(Some(album_id)) = crate::storage::asset_store::get_asset_id_by_path(&users_root_str)
    else {
        return;
    };

    let _ = crate::storage::asset_store::remove_asset_by_id(album_id.as_str());
    let _ = crate::storage::asset_store::remove_asset_by_path(&users_root_str);

    if let Ok(txn) = TREE.in_disk.begin_write() {
        if let Ok(mut table) = txn.open_table(METADATA_TABLE) {
            let _ = table.remove(&*album_id);
        }
        let _ = txn.commit();
    }
    dir_album::evict_dir_album(users_root);
}

// ─── Internal helpers ─────────────────────────────────────────────────────

fn resolve_scan_roots(src: &str, image_root: &Path, root: Option<&str>) -> AppResult<Vec<PathBuf>> {
    let users_root = get_resolved_users_home();
    let trimmed = src.trim();
    let mode = root.unwrap_or("auto");

    let global_only = mode == "global";
    let personal_only = mode == "personal";

    // Global scan: walk roots according to selector.
    if trimmed == "/" || trimmed.is_empty() {
        let mut roots = Vec::new();
        if !personal_only {
            roots.push(image_root.to_path_buf());
        }
        if !global_only
            && users_root.is_some()
            && let Some(u) = &users_root
        {
            roots.push(u.clone());
        }
        if roots.is_empty() {
            // personal requested but disabled
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                "Personal areas are disabled (users_home unset)".to_string(),
            ));
        }
        for r in &roots {
            if !r.is_dir() {
                return Err(AppError::new(
                    ErrorKind::InvalidInput,
                    format!("Path does not exist or is not a directory: {}", r.display()),
                ));
            }
        }
        return Ok(roots);
    }

    let trimmed_rel = trimmed.trim_start_matches('/');

    // Relative path.
    if personal_only {
        if let Some(u) = &users_root {
            let usr_candidate = u.join(trimmed_rel);
            if usr_candidate.is_dir() {
                return Ok(vec![usr_candidate]);
            }
        }
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            format!("Path does not exist or is not a directory: {trimmed_rel}"),
        ));
    }

    if global_only {
        let img_candidate = image_root.join(trimmed_rel);
        if img_candidate.is_dir() {
            return Ok(vec![img_candidate]);
        }
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            format!(
                "Path does not exist or is not a directory: {}",
                img_candidate.display()
            ),
        ));
    }

    // auto: image first, then users.
    let img_candidate = image_root.join(trimmed_rel);
    if img_candidate.is_dir() {
        return Ok(vec![img_candidate]);
    }
    if let Some(u) = &users_root {
        let usr_candidate = u.join(trimmed_rel);
        if usr_candidate.is_dir() {
            return Ok(vec![usr_candidate]);
        }
    }

    Err(AppError::new(
        ErrorKind::InvalidInput,
        format!(
            "Path does not exist or is not a directory: {}",
            img_candidate.display()
        ),
    ))
}

fn internal_subtree_roots() -> Vec<PathBuf> {
    let data_path = get_data_path();
    let data_root = std::fs::canonicalize(data_path).unwrap_or_else(|_| {
        if data_path.is_absolute() {
            data_path.clone()
        } else {
            std::env::current_dir().unwrap_or_default().join(data_path)
        }
    });

    ["object", "db"]
        .into_iter()
        .map(|name| {
            let path = data_root.join(name);
            std::fs::canonicalize(&path).unwrap_or(path)
        })
        .collect()
}

/// Sweep stale asset paths: for every DB record whose path is under
/// `root`, check that file.  If it no longer exists on disk, prune the path;
/// once the path is gone, remove the entire record (and its compressed
/// thumbnail).
fn sweep_stale_asset_paths(root: &Path) {
    use crate::model::abstract_data::AbstractData;
    use crate::process::path::normalize_asset_path;

    // File assets.
    let candidates: Vec<_> = {
        let tree = TREE.in_memory.read().expect("lock poisoned");
        tree.iter()
            .filter(|dt| {
                dt.abstract_data.path().is_some_and(|a| {
                    let abs = normalize_asset_path(&a.file);
                    abs.starts_with(root)
                })
            })
            .map(|dt| dt.abstract_data.clone())
            .collect()
    };

    let mut to_remove = Vec::new();

    for mut data in candidates {
        let had_path = data.path().is_some();
        let has_path = crate::process::path::prune_stale_asset_path(&mut data);

        if !has_path && had_path {
            to_remove.push(data);
        }
    }

    // Dir albums under root whose directory no longer exists on disk.
    let album_candidates: Vec<_> = {
        let tree = TREE.in_memory.read().expect("lock poisoned");
        tree.iter()
            .filter_map(|dt| match &dt.abstract_data {
                AbstractData::Album(alb) if !alb.metadata.dir_path.is_empty() => {
                    let dir_path = std::path::PathBuf::from(&alb.metadata.dir_path);
                    if dir_path.starts_with(root) {
                        Some((dir_path, alb.object.id))
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect()
    };

    for (dir_path, album_id) in album_candidates {
        if dir_path.exists() {
            continue;
        }
        // Remove from asset tables + metadata table.
        let _ = crate::storage::asset_store::remove_asset_by_id(album_id.as_str());
        let _ =
            crate::storage::asset_store::remove_asset_by_path(dir_path.to_string_lossy().as_ref());
        let write_txn = TREE.in_disk.begin_write();
        if let Ok(txn) = write_txn {
            if let Ok(mut table) = txn.open_table(crate::storage::db::METADATA_TABLE) {
                let _ = table.remove(&*album_id);
            }
            let _ = txn.commit();
        }
        crate::process::dir_album::evict_dir_album(&dir_path);
    }

    if !to_remove.is_empty() {
        debug!("pruning {} stale record(s)", to_remove.len());
        BATCH_COORDINATOR.execute_batch_detached(FlushTreeTask::remove(to_remove));
    }
}

fn is_inside_internal_subtree(path: &std::path::Path, internal_roots: &[PathBuf]) -> bool {
    internal_roots
        .iter()
        .any(|internal| path == internal || path.starts_with(internal))
}

fn should_walk_entry(entry: &DirEntry, internal_roots: &[PathBuf]) -> bool {
    entry.depth() == 0 || !is_inside_internal_subtree(entry.path(), internal_roots)
}

fn update_status(job_id: u64, update: impl FnOnce(&mut AlbumIndexStatus)) {
    let mut slot = INDEX_STATUS.lock().expect("lock poisoned");
    if slot.job_id == job_id {
        update(&mut slot.status);
    }
}

fn increment_scanned(job_id: u64) {
    update_status(job_id, |status| status.scanned += 1);
}

fn increment_matched(job_id: u64) {
    update_status(job_id, |status| status.matched += 1);
}

fn increment_processed(job_id: u64) {
    update_status(job_id, |status| status.processed += 1);
}

fn increment_failed(job_id: u64, path: Option<String>) {
    update_status(job_id, |status| {
        status.failed += 1;
        if let Some(p) = path
            && status.failed_paths.len() < 1000
        {
            status.failed_paths.push(p);
        }
    });
}

fn did_every_matched_file_fail(job_id: u64) -> bool {
    let slot = INDEX_STATUS.lock().expect("lock poisoned");
    slot.job_id == job_id
        && slot.status.matched > 0
        && slot.status.processed == 0
        && slot.status.failed >= slot.status.matched
}

fn finish_job(job_id: u64, state: AlbumIndexState) {
    {
        let mut slot = INDEX_STATUS.lock().expect("lock poisoned");
        if slot.job_id == job_id {
            slot.status.state = state;
            slot.status.finished_at = Some(Utc::now().timestamp_millis());
        }
    }

    let mut active = ACTIVE_INDEX.lock().expect("lock poisoned");
    if active.as_ref().is_some_and(|job| job.job_id == job_id) {
        *active = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::dir_album;
    use crate::tests::bootstrap::{
        TEST_ENV, TEST_SERIAL_GUARD, reset_backend_state, test_image_home,
    };
    use std::fs;
    use std::time::{Duration, Instant};

    fn wait_for_index_completion() {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let status = album_index_status();
            match status.state {
                AlbumIndexState::Running => {}
                _ => return,
            }
            assert!(
                Instant::now() <= deadline,
                "index did not complete within 30s (state={:?})",
                status.state
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    #[test]
    fn index_album_creates_dir_album_for_empty_directory() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        let image_home = test_image_home();
        let empty_dir = image_home.join("empty_album");
        fs::create_dir_all(&empty_dir).expect("create empty album dir");

        index_album_with_root("/", false, None).expect("index_album accepted");
        wait_for_index_completion();

        assert!(
            dir_album::get_album_id_for_dir(&empty_dir).is_some(),
            "empty directory must become a dir-album"
        );

        fs::remove_dir_all(&empty_dir).ok();
        reset_backend_state();
    }

    #[test]
    fn index_album_evicts_deleted_dir_album_after_sweep() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        let image_home = test_image_home();
        let doomed_dir = image_home.join("doomed_album");
        fs::create_dir_all(&doomed_dir).expect("create doomed album dir");

        // First pass: create the dir album.
        index_album_with_root("/", false, None).expect("index_album accepted");
        wait_for_index_completion();
        assert!(
            dir_album::get_album_id_for_dir(&doomed_dir).is_some(),
            "dir album should exist before deletion"
        );

        // Delete the directory from disk.
        fs::remove_dir_all(&doomed_dir).expect("remove doomed album dir");

        // Second pass: sweep should evict the stale dir album.
        index_album_with_root("/", false, None).expect("index_album accepted");
        wait_for_index_completion();

        assert!(
            dir_album::get_album_id_for_dir(&doomed_dir).is_none(),
            "deleted dir's album must be evicted after sweep"
        );

        reset_backend_state();
    }

    #[tokio::test]
    // The serial guard must stay held across the awaits below: dropping it
    // early would let another scenario test interleave while this test's
    // filesystem state is mid-setup. Single-purpose test lock, never held
    // across production awaits.
    #[allow(clippy::await_holding_lock)]
    async fn index_image_truncated_leaves_placeholder() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        let image_home = test_image_home();
        let dir = image_home.join("direct_trunc");
        std::fs::create_dir_all(&dir).expect("create album test dir");
        let path = dir.join("truncated.jpg");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(path.to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate jpeg");
        let bytes = std::fs::read(&path).expect("read generated test photo");
        std::fs::write(&path, &bytes[..100]).expect("truncate test photo");

        let rel = path
            .strip_prefix(&image_home)
            .expect("test photo lives under the image home")
            .to_path_buf();
        let res = crate::workflow::index_image(&rel).await;
        assert!(res.is_err(), "index_image should fail");

        // drain
        let _ = crate::tasks::BATCH_COORDINATOR
            .execute_batch_waiting(crate::tasks::batcher::flush_tree::FlushTreeTask::insert(
                vec![],
            ))
            .await;
        let _ = crate::tasks::BATCH_COORDINATOR
            .execute_batch_waiting(crate::tasks::batcher::update_tree::UpdateTreeTask)
            .await;

        let tree = crate::storage::db::TREE
            .in_memory
            .read()
            .expect("in-memory tree lock");
        assert!(
            tree.iter().any(|dt| dt
                .abstract_data
                .path()
                .is_some_and(|p| p.file.contains("truncated.jpg"))),
            "placeholder row must be visible"
        );
        drop(tree);

        std::fs::remove_dir_all(&dir).ok();
        reset_backend_state();
    }

    #[test]
    fn index_album_failed_path_exposes_truncated_jpeg() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        let image_home = test_image_home();
        let dir = image_home.join("trunc_album");
        std::fs::create_dir_all(&dir).expect("create album test dir");
        let path = dir.join("truncated.jpg");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(path.to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate jpeg");
        let bytes = std::fs::read(&path).expect("read generated test photo");
        std::fs::write(&path, &bytes[..100]).expect("truncate test photo");

        index_album_with_root("/", false, None).expect("index_album accepted");
        wait_for_index_completion();

        let status = album_index_status();
        assert_eq!(status.failed, 1, "one failed file");
        assert!(
            status
                .failed_paths
                .iter()
                .any(|p| p.contains("truncated.jpg")),
            "failed path must be exposed: {:?}",
            status.failed_paths
        );

        let tree = TREE.in_memory.read().expect("in-memory tree lock");
        assert!(
            tree.iter().any(|dt| {
                dt.abstract_data
                    .path()
                    .is_some_and(|p| p.file.contains("truncated.jpg"))
                    && dt.abstract_data.pending()
            }),
            "placeholder row must be visible in tree and pending"
        );
        drop(tree);

        std::fs::remove_dir_all(&dir).ok();
        reset_backend_state();
    }

    #[test]
    fn bug_rebuild_dir_album_cache_reset_scan_returns_rows() {
        // Regression pin for `bug-rebuild-dir-album-cache`: after a reset +
        // scan with no restart, an album-filtered prefetch must report the
        // scanned rows (N), not 0. The stale `DIR_ALBUM_CACHE` entries that
        // survived a table drop were the cause: the scan reused old album ids
        // that no longer existed in the freshly rebuilt tables.
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        let image_home = test_image_home();
        let album_dir = image_home.join("bug_rebuild_cache");
        std::fs::create_dir_all(&album_dir).expect("create album dir under test home");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(album_dir.join("photo.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate jpeg");

        // First pass populates the cache and tables.
        index_album_with_root("/", false, None).expect("initial index accepted");
        wait_for_index_completion();

        // Reset exactly as the HTTP `clear` path does, then scan again.
        clear_asset_tables_and_caches().expect("clear asset tables and caches");
        index_album_with_root("/", false, None).expect("reset scan accepted");
        wait_for_index_completion();

        let album_id = dir_album::get_album_id_for_dir(&album_dir)
            .expect("album must be registered after reset scan");

        let tree = TREE.in_memory.read().expect("TREE in_memory lock");
        let rows = tree
            .iter()
            .filter(|dt| match &dt.abstract_data {
                crate::model::abstract_data::AbstractData::Image(img) => {
                    img.metadata.album == Some(album_id)
                }
                crate::model::abstract_data::AbstractData::Video(vid) => {
                    vid.metadata.album == Some(album_id)
                }
                crate::model::abstract_data::AbstractData::Album(_) => false,
            })
            .count();
        assert_eq!(
            rows, 1,
            "album-filtered prefetch must return N rows after reset+scan, got {rows}"
        );
        drop(tree);

        std::fs::remove_dir_all(&album_dir).ok();
        reset_backend_state();
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn index_album_walks_both_roots_when_users_home_enabled() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        let image_home = test_image_home();
        let data_path = crate::storage::files::DATA_PATH
            .get()
            .expect("DATA_PATH set");
        let users_home = data_path.join("users");
        let _ = std::fs::remove_dir_all(&users_home);
        std::fs::create_dir_all(&users_home).expect("create users home");

        // enable users_home
        {
            let mut c = crate::model::config::APP_CONFIG
                .get()
                .expect("cfg")
                .write()
                .expect("lock");
            c.users_home = Some(users_home.clone());
        }

        let img_album = image_home.join("global_album");
        let usr_album = users_home.join("alice");
        std::fs::create_dir_all(&img_album).expect("create img album");
        std::fs::create_dir_all(&usr_album).expect("create usr album");

        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(img_album.join("photo.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate jpeg");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(usr_album.join("photo.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate jpeg");

        index_album_with_root("/", false, None).expect("index_album accepted");
        wait_for_index_completion();

        assert!(
            dir_album::get_album_id_for_dir(&img_album).is_some(),
            "global album must be indexed"
        );
        assert!(
            dir_album::get_album_id_for_dir(&usr_album).is_some(),
            "personal album must be indexed"
        );
        assert!(
            dir_album::get_album_id_for_dir(&users_home).is_none(),
            "users_home root itself must not gain an album record"
        );

        // asset membership per root
        let tree = crate::storage::db::TREE
            .in_memory
            .read()
            .expect("tree lock");
        let has_img = tree.iter().any(|dt| {
            dt.abstract_data
                .path()
                .is_some_and(|p| p.file.contains("global_album"))
        });
        let has_usr = tree.iter().any(|dt| {
            dt.abstract_data
                .path()
                .is_some_and(|p| p.file.contains("alice"))
        });
        assert!(has_img, "global asset must be present");
        assert!(has_usr, "personal asset must be present");
        drop(tree);

        // stale sweep prunes removed personal file
        let doomed = usr_album.join("doomed.jpg");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(doomed.to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate jpeg");
        index_album_with_root("/", false, None).expect("index accepted");
        wait_for_index_completion();
        assert!(
            crate::storage::db::TREE
                .in_memory
                .read()
                .expect("tree")
                .iter()
                .any(|dt| dt
                    .abstract_data
                    .path()
                    .is_some_and(|p| p.file.contains("doomed.jpg"))),
            "doomed personal file should be indexed before removal"
        );
        std::fs::remove_file(&doomed).expect("remove doomed");
        index_album_with_root("/", false, None).expect("index accepted");
        wait_for_index_completion();
        assert!(
            !crate::storage::db::TREE
                .in_memory
                .read()
                .expect("tree")
                .iter()
                .any(|dt| dt
                    .abstract_data
                    .path()
                    .is_some_and(|p| p.file.contains("doomed.jpg"))),
            "removed personal file must be pruned by sweep"
        );

        std::fs::remove_dir_all(&img_album).ok();
        std::fs::remove_dir_all(&usr_album).ok();
        // reset users_home
        {
            let mut c = crate::model::config::APP_CONFIG
                .get()
                .expect("cfg")
                .write()
                .expect("lock");
            c.users_home = None;
        }
        reset_backend_state();
    }

    #[test]
    fn index_album_disabled_mode_single_root_unchanged() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        // users_home stays None (disabled)
        let image_home = test_image_home();
        let album = image_home.join("disabled_album");
        std::fs::create_dir_all(&album).expect("create album");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(album.join("photo.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate jpeg");

        index_album_with_root("/", false, None).expect("index accepted");
        wait_for_index_completion();

        assert!(dir_album::get_album_id_for_dir(&album).is_some());
        reset_backend_state();
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn index_album_leading_slash_relative_resolves_under_image_root() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        let image_home = test_image_home();
        let album = image_home.join("leading_slash_album");
        std::fs::create_dir_all(&album).expect("create album");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(album.join("photo.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate jpeg");

        index_album_with_root("/leading_slash_album", false, None).expect("index accepted");
        wait_for_index_completion();

        assert!(dir_album::get_album_id_for_dir(&album).is_some());
        reset_backend_state();
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn index_album_users_root_relative_resolves_under_users_root_when_enabled() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        let data_path = crate::storage::files::DATA_PATH
            .get()
            .expect("DATA_PATH set");
        let users_home = data_path.join("users");
        let _ = std::fs::remove_dir_all(&users_home);
        std::fs::create_dir_all(&users_home).expect("create users home");
        {
            let mut c = crate::model::config::APP_CONFIG
                .get()
                .expect("cfg")
                .write()
                .expect("lock");
            c.users_home = Some(users_home.clone());
        }

        let album = users_home.join("onlypersonal");
        std::fs::create_dir_all(&album).expect("create album");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(album.join("photo.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate jpeg");

        index_album_with_root("onlypersonal", false, None).expect("index accepted");
        wait_for_index_completion();

        assert!(dir_album::get_album_id_for_dir(&album).is_some());
        {
            let mut c = crate::model::config::APP_CONFIG
                .get()
                .expect("cfg")
                .write()
                .expect("lock");
            c.users_home = None;
        }
        reset_backend_state();
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn index_album_image_root_wins_on_ambiguous_relative_name() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        let image_home = test_image_home();
        let data_path = crate::storage::files::DATA_PATH
            .get()
            .expect("DATA_PATH set");
        let users_home = data_path.join("users");
        let _ = std::fs::remove_dir_all(&users_home);
        std::fs::create_dir_all(&users_home).expect("create users home");
        {
            let mut c = crate::model::config::APP_CONFIG
                .get()
                .expect("cfg")
                .write()
                .expect("lock");
            c.users_home = Some(users_home.clone());
        }

        let img_album = image_home.join("ambiguous");
        let usr_album = users_home.join("ambiguous");
        std::fs::create_dir_all(&img_album).expect("create img album");
        std::fs::create_dir_all(&usr_album).expect("create usr album");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(img_album.join("photo.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate jpeg");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(usr_album.join("photo.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate jpeg");

        index_album_with_root("ambiguous", false, None).expect("index accepted");
        wait_for_index_completion();

        assert!(dir_album::get_album_id_for_dir(&img_album).is_some());
        assert!(dir_album::get_album_id_for_dir(&usr_album).is_none());

        {
            let mut c = crate::model::config::APP_CONFIG
                .get()
                .expect("cfg")
                .write()
                .expect("lock");
            c.users_home = None;
        }
        reset_backend_state();
    }
}
