use chrono::Utc;
use log::{info, warn};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tokio::task::JoinHandle;
use walkdir::{DirEntry, WalkDir};

use crate::error::{AppError, ErrorKind, handle_error};
use crate::model::media::is_valid_media_file;
use crate::router::AppResult;
use crate::storage::files::get_data_path;
use crate::storage::files::get_resolved_image_home;

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

/// Index all media files under `src` (a directory path relative to
/// `IMAGE_HOME`).  Walks recursively, calling `index_image` on each valid
/// media file.  Album is always resolved from the file's parent directory.
///
/// Runs as a background job; status can be polled via `album_index_status()`.
#[allow(clippy::too_many_lines)]
pub fn index_album(src: &str) -> AppResult<()> {
    let image_root = get_resolved_image_home()
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "No imagePath configured to scan"))?;

    let root = image_root.join(src.trim_start_matches('/'));

    if !root.is_dir() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            format!(
                "Path does not exist or is not a directory: {}",
                root.display()
            ),
        ));
    }

    let internal_roots = internal_subtree_roots();
    if is_inside_internal_subtree(&root, &internal_roots) {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "Cannot index Picasu internal data directories",
        ));
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
        slot.status = AlbumIndexStatus {
            state: AlbumIndexState::Running,
            root: Some(root.to_string_lossy().into_owned()),
            scanned: 0,
            matched: 0,
            processed: 0,
            failed: 0,
            started_at: Some(Utc::now().timestamp_millis()),
            finished_at: None,
            cancel_requested: false,
        };
    }

    *ACTIVE_INDEX.lock().expect("lock poisoned") = Some(ActiveIndex {
        job_id,
        cancel: cancel.clone(),
    });

    let image_root_clone = image_root;
    let root_clone = root.clone();
    tokio::spawn(async move {
        info!("Starting one-time album index: {}", root_clone.display());

        let mut handles: Vec<JoinHandle<()>> = Vec::new();
        let walker = WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| should_walk_entry(entry, &internal_roots));

        for entry in walker {
            if cancel.load(Ordering::SeqCst) {
                break;
            }

            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    warn!("Album index walk error: {err}");
                    increment_failed(job_id);
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            increment_scanned(job_id);

            let abs_path = entry.into_path();
            if !is_valid_media_file(&abs_path) {
                continue;
            }

            increment_matched(job_id);

            let relative = if let Ok(r) = abs_path.strip_prefix(&image_root_clone) {
                r.to_path_buf()
            } else {
                warn!("File outside IMAGE_HOME, skipping: {}", abs_path.display());
                increment_failed(job_id);
                continue;
            };

            handles.push(tokio::spawn(async move {
                match crate::workflow::index_image(&relative, None).await {
                    Ok(()) => increment_processed(job_id),
                    Err(err) => {
                        handle_error(err);
                        increment_failed(job_id);
                    }
                }
            }));
        }

        for handle in handles {
            if let Err(err) = handle.await {
                warn!("Album index file task join error: {err}");
                increment_failed(job_id);
            }
        }

        let state = if cancel.load(Ordering::SeqCst) {
            AlbumIndexState::Canceled
        } else if did_every_matched_file_fail(job_id) {
            AlbumIndexState::Failed
        } else {
            AlbumIndexState::Completed
        };

        finish_job(job_id, state);
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

// ─── Internal helpers ─────────────────────────────────────────────────────

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

fn increment_failed(job_id: u64) {
    update_status(job_id, |status| status.failed += 1);
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
