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
use walkdir::{DirEntry, WalkDir};

use crate::error::{AppError, ErrorKind};
use crate::model::media::{MediaOutcome, SkipReason, classify_media_file, is_valid_media_file};
use crate::router::AppResult;
use crate::storage::files::get_data_path;
use crate::storage::files::get_resolved_image_home;
use crate::storage::files::get_resolved_users_home;
use crate::tasks::runtime::spawn;

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
#[quint::operation("OP-11")]
pub fn clear_asset_tables_and_caches() -> AppResult<()> {
    crate::storage::db::clear_all_asset_tables_drop()
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("{e:#}")))?;
    crate::storage::db::TREE
        .in_memory
        .write()
        .expect("TREE in_memory lock")
        .clear();
    crate::storage::tree_view::invalidate_tree_view();
    crate::process::dir_album::reset_dir_album_cache();
    Ok(())
}

/// Index all media files under `src` (a directory path relative to
/// `IMAGE_HOME`). Walks recursively, submitting each valid media file to the
/// scan intake; intake workers run `workflow::process_file` on them. Album
/// is always resolved from the file's parent directory.
///
/// Runs as a background job; status can be polled via `album_index_status()`.
/// Index all media files under `src`, optionally clearing first and/or
/// selecting a scan root (`"global"`, `"personal"`, or `"auto"` preserving
/// the historical image-then-users probe order).
static INDEX_NOTIFY: tokio::sync::Notify = tokio::sync::Notify::const_new();

/// Handle to a started scan job. Awaiting it resolves when the job reaches a
/// terminal state; a cancellation request does not resolve it early.
pub(crate) struct IndexJob {
    job_id: u64,
}

impl IndexJob {
    pub(crate) async fn wait(self) {
        wait_for_job(self.job_id).await;
    }
}

/// Start a scan and discard its completion handle. Request-path callers that
/// only need to report acceptance use this.
pub fn index_album_with_root(src: &str, clear: bool, root: Option<&str>) -> AppResult<()> {
    start_index_job(src, clear, root).map(|_| ())
}

#[allow(clippy::too_many_lines)]
pub(crate) fn start_index_job(src: &str, clear: bool, root: Option<&str>) -> AppResult<IndexJob> {
    if crate::tasks::queue::is_shutting_down() {
        return Err(AppError::new(
            ErrorKind::Conflict,
            "Server is shutting down",
        ));
    }

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
    spawn(async move {
        info!(
            "Starting one-time album index: {} root(s)",
            scan_roots_clone.len()
        );
        // Phase boundary: the drain report at the terminal state covers this
        // scan only, not traffic that ran before it started.
        crate::tasks::observe::reset();

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
                        note_file_failed(job_id, None);
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

        // Collected during the single file walk below. Detection runs after
        // the walk (log only, never deletes) and checks sibling media on
        // demand, so the walk holds no per-media state.
        let mut sidecar_paths: Vec<PathBuf> = Vec::new();

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
                        note_file_failed(job_id, None);
                        continue;
                    }
                };

                if !entry.file_type().is_file() {
                    continue;
                }

                increment_scanned(job_id);

                let abs_path = entry.into_path();
                let is_xmp = abs_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("xmp"));
                if is_xmp {
                    sidecar_paths.push(abs_path.clone());
                }

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

                if let Err(err) = crate::workflow::submit_index(&abs_path, job_id).await {
                    warn!(
                        "Album index intake submit failed for {}: {err:#}",
                        abs_path.display()
                    );
                    note_file_failed(job_id, Some(abs_path.display().to_string()));
                }
            }
        }

        debug!("walk done, draining intake (job {job_id})");
        crate::tasks::queue::drain_intake(job_id, &cancel).await;
        debug!("intake drained (job {job_id})");

        // Shutdown and user cancellation stop the job at the first phase
        // boundary: the sweep would otherwise keep the writer busy after the
        // server has stopped accepting requests.
        if cancel.load(Ordering::SeqCst) || crate::tasks::queue::is_shutting_down() {
            finish_job(job_id, AlbumIndexState::Canceled);
            return;
        }

        // Barrier on the serial writer so the in-memory tree reflects
        // all walk results before the sweep reads it.
        debug!("writer barrier pre-sweep (job {job_id})");
        crate::tasks::queue::writer::barrier().await;
        debug!("writer barrier pre-sweep done (job {job_id})");

        // Sweep stale asset paths: remove DB records whose asset file no
        // longer exists on disk under each target root.
        for scan_root in &scan_roots_clone {
            debug!(
                "sweeping stale asset paths under {} (job {job_id})",
                scan_root.display()
            );
            sweep_stale_asset_paths(scan_root, &cancel).await;
            debug!("stale asset path sweep done (job {job_id})");
        }

        // The users_home root itself must never carry an album record.
        if let Some(users_root) = get_resolved_users_home() {
            prune_users_root_album(&users_root).await;
        }

        // Orphan sidecar detection: log only, never delete. Observation is
        // safe while a mutation is in flight; cleanup would not be.
        for orphan in orphan_sidecars(sidecar_paths) {
            warn!("Orphan sidecar (no media file): {}", orphan.display());
        }

        // Barrier on the sweep's writer ops, then on the tree rebuild
        // for album metadata.
        debug!("writer barrier post-sweep (job {job_id})");
        crate::tasks::queue::writer::barrier().await;
        debug!("writer barrier update-tree (job {job_id})");
        crate::tasks::queue::writer::barrier().await;
        debug!("writer drained (job {job_id})");

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

    Ok(IndexJob { job_id })
}

pub fn cancel_album_index() -> AppResult<()> {
    let active = ACTIVE_INDEX.lock().expect("lock poisoned").clone();
    let Some(active) = active else {
        return Err(AppError::new(ErrorKind::NotFound, "No active index job"));
    };

    active.cancel.store(true, Ordering::SeqCst);
    crate::tasks::queue::notify_job_cancel();
    let mut slot = INDEX_STATUS.lock().expect("lock poisoned");
    if slot.job_id == active.job_id && slot.status.state == AlbumIndexState::Running {
        slot.status.cancel_requested = true;
    }

    Ok(())
}

/// Test-harness quiescence helper: cancel any in-flight index job and wait
/// (polling, no async) until it reaches a terminal state or `timeout_ms`
/// elapses. Operates directly on the module statics so callers do not need
/// an HTTP client or a Tokio runtime handle. Safe to call when no job is
/// running (returns immediately). On timeout, logs via `eprintln` and
/// returns without panicking.
#[cfg(test)]
pub(crate) fn cancel_and_wait_index_idle(timeout_ms: u64) {
    // Best-effort cancel; ignore the "no active job" error.
    let _ = cancel_album_index();

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        let active = ACTIVE_INDEX.lock().expect("lock poisoned").is_some();
        let slot = INDEX_STATUS.lock().expect("lock poisoned");
        let state = slot.status.state.clone();
        let scanned = slot.status.scanned;
        let matched = slot.status.matched;
        let processed = slot.status.processed;
        drop(slot);

        let idle = !active && !matches!(state, AlbumIndexState::Running);
        if idle {
            return;
        }

        if std::time::Instant::now() >= deadline {
            eprintln!(
                "cancel_and_wait_index_idle: timed out after {timeout_ms} ms (active={active}, state={state:?}, scanned={scanned}, matched={matched}, processed={processed})"
            );
            return;
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

pub fn album_index_status() -> AlbumIndexStatus {
    INDEX_STATUS.lock().expect("lock poisoned").status.clone()
}

/// Remove any album record whose directory is the `users_home` root itself.
/// The root must never gain an album record; this prunes one if a prior
/// state created it. The DB writes run on the serial writer; the barrier
/// that follows the sweep covers them.
async fn prune_users_root_album(users_root: &Path) {
    use crate::tasks::queue::writer::submit;

    if let Err(err) = submit(crate::tasks::queue::writer::WriteOp::PruneUsersRoot(
        users_root.to_path_buf(),
    ))
    .await
    {
        warn!("prune users-root album submit failed: {err:#}");
    }
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
/// `root`, check that file. Missing paths are submitted to the serial
/// writer as a batched `SweepRemoveBatch` op that resolves each record by
/// its exact path/id. Per-chunk cancel/shutdown and inline re-checks
/// (generation, mutation lease, filesystem) guard against races.
#[quint::operation("OP-9")]
async fn sweep_stale_asset_paths(root: &Path, cancel: &AtomicBool) {
    const CHUNK: usize = 200;
    use crate::storage::asset_store;

    // File assets: source candidates from the primary ASSET_BY_PATH table,
    // not the in-memory tree view.
    let root_str = root.to_string_lossy().to_string();
    let db_rows = asset_store::list_asset_paths_under(&root_str).unwrap_or_default();

    let mut candidates: Vec<(PathBuf, Option<u64>)> = Vec::new();
    for (path_str, _id) in db_rows {
        let path = PathBuf::from(&path_str);
        if path.exists() {
            continue;
        }
        let generation = crate::tasks::path_epoch::current_generation(&path);
        candidates.push((path, generation));
    }

    // Retained-set strategy (deliberate): chunked global-per-root
    // processing. If a future benchmark/optimization effort shows the
    // retained sets are a problem, page the prefix scan by cursor and/or
    // scope by subdir — see the sweep follow-ups in the plan.
    for chunk in candidates.chunks(CHUNK) {
        if cancel.load(Ordering::SeqCst) || crate::tasks::queue::is_shutting_down() {
            break;
        }
        let mut survivors: Vec<PathBuf> = Vec::new();
        for (path, generation_at_enum) in chunk {
            if crate::tasks::path_epoch::current_generation(path) != *generation_at_enum {
                continue;
            }
            if crate::tasks::path_epoch::is_busy(path) {
                continue;
            }
            if path.exists() {
                continue;
            }
            survivors.push(path.clone());
        }
        if survivors.is_empty() {
            continue;
        }
        if let Err(err) = crate::tasks::queue::writer::submit(
            crate::tasks::queue::writer::WriteOp::SweepRemoveBatch { paths: survivors },
        )
        .await
        {
            warn!("sweep remove batch submit failed: {err:#}");
        }
    }

    // Dir albums under root whose directory no longer exists on disk.
    if cancel.load(Ordering::SeqCst) || crate::tasks::queue::is_shutting_down() {
        return;
    }
    let album_candidates: Vec<_> = {
        // Source candidates from the primary ASSET_BY_ID table (album assets),
        // not the in-memory tree view. Keep the same sequential-collect shape
        // as the asset phase.
        let rows =
            crate::storage::asset_store::list_album_assets_under(&root_str).unwrap_or_default();
        rows.into_iter()
            .filter_map(|(dir_path_str, album_id)| {
                let dir_path = std::path::PathBuf::from(&dir_path_str);
                if dir_path.as_os_str().is_empty() {
                    return None;
                }
                if !dir_path.starts_with(root) {
                    return None;
                }
                Some((
                    dir_path.clone(),
                    album_id,
                    crate::tasks::path_epoch::current_generation(&dir_path),
                ))
            })
            .collect()
    };

    let mut survivors: Vec<(PathBuf, arrayvec::ArrayString<64>)> = Vec::new();
    for (dir_path, album_id, generation_at_enum) in album_candidates {
        if cancel.load(Ordering::SeqCst) || crate::tasks::queue::is_shutting_down() {
            break;
        }
        if dir_path.exists() {
            continue;
        }
        if crate::tasks::path_epoch::current_generation(&dir_path) != generation_at_enum {
            continue;
        }
        if crate::tasks::path_epoch::is_busy(&dir_path) {
            continue;
        }
        if crate::process::dir_album::get_dir_path_for_album(album_id).as_deref()
            != Some(dir_path.as_path())
        {
            continue;
        }
        survivors.push((dir_path, album_id));
    }
    if !survivors.is_empty()
        && let Err(err) = crate::tasks::queue::writer::submit(
            crate::tasks::queue::writer::WriteOp::SweepDirAlbums(survivors),
        )
        .await
    {
        warn!("sweep dir-album remove submit failed: {err:#}");
    }
}

/// Return the sidecar paths that have no media file sharing their stem.
///
/// A sidecar is orphaned when its directory holds no file with the same
/// stem and a supported media extension. Sibling media is checked on demand,
/// so the walk does not retain a second path list for every asset in the
/// library. Album sidecars named `.albuminfo.xmp` are ignored: they describe
/// a directory, not a single media file. Log-only detection: this helper
/// never deletes anything.
fn orphan_sidecars(sidecars: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    sidecars
        .into_iter()
        .filter(|sidecar| {
            if sidecar
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(".albuminfo.xmp"))
            {
                return false;
            }
            let is_sidecar = sidecar
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("xmp"));
            is_sidecar && !sibling_media_exists(sidecar)
        })
        .collect()
}

/// Whether `sidecar`'s directory contains a media file with the same stem.
fn sibling_media_exists(sidecar: &Path) -> bool {
    let Some(parent) = sidecar.parent() else {
        return false;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return false;
    };
    let stem = sidecar.file_stem();
    entries.flatten().any(|entry| {
        let path = entry.path();
        entry.file_type().is_ok_and(|kind| kind.is_file())
            && path.file_stem() == stem
            && is_valid_media_file(&path)
    })
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

/// Record one file finished by an intake worker (called from `tasks::queue`).
pub(crate) fn note_file_processed(job_id: u64) {
    update_status(job_id, |status| status.processed += 1);
}

/// Record one file failed by an intake worker (called from `tasks::queue`).
pub(crate) fn note_file_failed(job_id: u64, path: Option<String>) {
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
    // Full synchronous bump strictly before the terminal state is published, so
    // a client polling status then reading sees the finished scan's rows.
    crate::storage::tree_view::bump_tree_epoch();
    let report_context = {
        let mut slot = INDEX_STATUS.lock().expect("lock poisoned");
        if slot.job_id == job_id {
            let context = format!(
                "scan job={job_id} state={state:?} scanned={} matched={} processed={} failed={}",
                slot.status.scanned, slot.status.matched, slot.status.processed, slot.status.failed
            );
            slot.status.state = state;
            slot.status.finished_at = Some(Utc::now().timestamp_millis());
            Some(context)
        } else {
            None
        }
    };

    let mut active = ACTIVE_INDEX.lock().expect("lock poisoned");
    if active.as_ref().is_some_and(|job| job.job_id == job_id) {
        *active = None;
    }
    drop(active);

    if let Some(context) = report_context {
        crate::tasks::observe::drain_report(&context);
    }

    INDEX_NOTIFY.notify_waiters();
}

/// Wait until `job_id` reaches a terminal state. Waiters register before the
/// status check, so a completion between the check and the wait is not lost.
async fn wait_for_job(job_id: u64) {
    loop {
        let notified = INDEX_NOTIFY.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let done = {
            let slot = INDEX_STATUS.lock().expect("lock poisoned");
            slot.job_id != job_id || slot.status.state != AlbumIndexState::Running
        };
        if done {
            return;
        }
        notified.await;
    }
}

/// Cancel the active job and wait for it to stop, bounded by `timeout`.
/// Returns `true` when no job is running.
pub(crate) async fn wait_for_active_index(timeout: std::time::Duration) -> bool {
    let Some(active) = ACTIVE_INDEX.lock().expect("lock poisoned").clone() else {
        return true;
    };
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let notified = INDEX_NOTIFY.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let done = {
            let slot = INDEX_STATUS.lock().expect("lock poisoned");
            slot.job_id != active.job_id || slot.status.state != AlbumIndexState::Running
        };
        if done {
            return true;
        }
        if tokio::time::timeout_at(deadline, notified).await.is_err() {
            return false;
        }
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

        // drain the writer: the placeholder flush plus its cascade must be
        // visible before asserting.
        crate::tasks::queue::writer::barrier().await;

        let tree = crate::storage::db::snapshot_tree();
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

        let tree = crate::storage::db::snapshot_tree();
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

        let tree = crate::storage::db::snapshot_tree();
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
        let tree = crate::storage::db::snapshot_tree();
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
            crate::storage::db::snapshot_tree().iter().any(|dt| dt
                .abstract_data
                .path()
                .is_some_and(|p| p.file.contains("doomed.jpg"))),
            "doomed personal file should be indexed before removal"
        );
        std::fs::remove_file(&doomed).expect("remove doomed");
        index_album_with_root("/", false, None).expect("index accepted");
        wait_for_index_completion();
        assert!(
            !crate::storage::db::snapshot_tree().iter().any(|dt| dt
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

    #[test]
    fn index_album_unknown_scan_root_selector_is_invalid_input() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        let err = index_album_with_root("/", false, Some("bogus")).expect_err("unknown selector");
        assert_eq!(err.kind, crate::error::ErrorKind::InvalidInput);
        assert!(
            err.to_string().contains("Unknown scan root selector"),
            "error should name the bad selector: {err}"
        );

        reset_backend_state();
    }

    #[test]
    fn index_album_personal_selector_disabled_is_invalid_input() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        // users_home stays None (disabled)
        let err = index_album_with_root("/", false, Some("personal")).expect_err("disabled");
        assert_eq!(err.kind, crate::error::ErrorKind::InvalidInput);

        reset_backend_state();
    }

    #[test]
    fn index_album_sweep_is_scoped_to_scanned_root() {
        use crate::model::asset::{AssetKind, AssetRecord};
        use crate::storage::asset_store;
        use arrayvec::ArrayString;

        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        let image_home = test_image_home();
        let scanned = image_home.join("scanned_root");
        let outside = image_home.join("outside_root");
        fs::create_dir_all(&scanned).expect("create scanned root");
        fs::create_dir_all(&outside).expect("create outside root");

        let scanned_ghost = scanned.join("ghost.jpg");
        let outside_ghost = outside.join("ghost.jpg");

        let scanned_hash: ArrayString<64> = "scannedghosthash"
            .parse()
            .expect("test content hash parses");
        let outside_hash: ArrayString<64> = "outsideghosthash"
            .parse()
            .expect("test content hash parses");
        let rec_scanned = AssetRecord::new_media(
            AssetKind::Image,
            scanned_ghost.to_string_lossy().into_owned(),
            Some(scanned_hash),
            10,
            "jpg".into(),
            0,
        );
        let rec_outside = AssetRecord::new_media(
            AssetKind::Image,
            outside_ghost.to_string_lossy().into_owned(),
            Some(outside_hash),
            10,
            "jpg".into(),
            0,
        );
        asset_store::insert_asset(&rec_scanned).expect("insert scanned ghost");
        asset_store::insert_asset(&rec_outside).expect("insert outside ghost");

        // Make the on-disk rows visible to the sweep's in-memory read.
        crate::storage::tree_view::bump_tree_epoch();

        index_album_with_root("/scanned_root", false, None).expect("index accepted");
        wait_for_index_completion();

        let tree = crate::storage::db::snapshot_tree();
        let scanned_survives = tree.iter().any(|dt| {
            dt.abstract_data
                .path()
                .is_some_and(|p| p.file == scanned_ghost.to_string_lossy())
        });
        let outside_survives = tree.iter().any(|dt| {
            dt.abstract_data
                .path()
                .is_some_and(|p| p.file == outside_ghost.to_string_lossy())
        });
        drop(tree);

        assert!(
            !scanned_survives,
            "stale row under the scanned root must be pruned"
        );
        assert!(
            outside_survives,
            "ghost row outside the scanned root must survive the sweep"
        );

        fs::remove_dir_all(&scanned).ok();
        fs::remove_dir_all(&outside).ok();
        reset_backend_state();
    }

    #[test]
    fn index_album_sweep_same_hash_keeps_sibling_root_ghost() {
        use crate::model::asset::{AssetKind, AssetRecord};
        use crate::storage::asset_store;
        use arrayvec::ArrayString;

        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        let image_home = test_image_home();
        let scanned = image_home.join("scanned_root");
        let outside = image_home.join("outside_root");
        fs::create_dir_all(&scanned).expect("create scanned root");
        fs::create_dir_all(&outside).expect("create outside root");

        let scanned_ghost = scanned.join("ghost.jpg");
        let outside_ghost = outside.join("ghost.jpg");

        // Same content hash under both roots: the sweep must not let the
        // hash group reap the sibling root's ghost row.
        let shared_hash: ArrayString<64> =
            "sharedghosthash".parse().expect("test content hash parses");
        let rec_scanned = AssetRecord::new_media(
            AssetKind::Image,
            scanned_ghost.to_string_lossy().into_owned(),
            Some(shared_hash),
            10,
            "jpg".into(),
            0,
        );
        let rec_outside = AssetRecord::new_media(
            AssetKind::Image,
            outside_ghost.to_string_lossy().into_owned(),
            Some(shared_hash),
            10,
            "jpg".into(),
            0,
        );
        asset_store::insert_asset(&rec_scanned).expect("insert scanned ghost");
        asset_store::insert_asset(&rec_outside).expect("insert outside ghost");

        // Make the on-disk rows visible to the sweep's in-memory read.
        crate::storage::tree_view::bump_tree_epoch();

        index_album_with_root("/scanned_root", false, None).expect("index accepted");
        wait_for_index_completion();

        let tree = crate::storage::db::snapshot_tree();
        let scanned_survives = tree.iter().any(|dt| {
            dt.abstract_data
                .path()
                .is_some_and(|p| p.file == scanned_ghost.to_string_lossy())
        });
        let outside_survives = tree.iter().any(|dt| {
            dt.abstract_data
                .path()
                .is_some_and(|p| p.file == outside_ghost.to_string_lossy())
        });
        drop(tree);

        assert!(
            !scanned_survives,
            "stale row under the scanned root must be pruned"
        );
        assert!(
            outside_survives,
            "ghost row outside the scanned root must survive the sweep even when hashes match"
        );

        fs::remove_dir_all(&scanned).ok();
        fs::remove_dir_all(&outside).ok();
        reset_backend_state();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn index_scan_respects_path_lease_before_pruning() {
        use crate::model::asset::{AssetKind, AssetRecord};
        use crate::storage::asset_store;
        use arrayvec::ArrayString;

        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        let image_home = test_image_home();
        let scanned = image_home.join("scanned_root");
        std::fs::create_dir_all(&scanned).expect("create scanned root");

        let ghost = scanned.join("ghost.jpg");
        let hash: ArrayString<64> = "ghosthash".parse().expect("hash");
        let rec = AssetRecord::new_media(
            AssetKind::Image,
            ghost.to_string_lossy().into_owned(),
            Some(hash),
            10,
            "jpg".into(),
            0,
        );
        asset_store::insert_asset(&rec).expect("insert ghost");
        let hash_str = hash.as_str();
        let thumbnail = crate::storage::files::get_data_path().join(format!(
            "object/compressed/{}/{hash_str}.jpg",
            &hash_str[..2]
        ));
        std::fs::create_dir_all(thumbnail.parent().expect("thumbnail parent"))
            .expect("create thumbnail dir");
        std::fs::write(&thumbnail, b"thumb").expect("write thumbnail");
        crate::storage::tree_view::bump_tree_epoch();

        // A mutation holds the ghost path while the sweep runs.
        let lease = crate::tasks::path_epoch::lease_paths([ghost.clone()]);

        index_album_with_root("/scanned_root", false, None).expect("index accepted");
        wait_for_index_completion();
        tokio::time::sleep(Duration::from_millis(250)).await;

        let survives = |path: &std::path::Path| {
            crate::storage::db::snapshot_tree().iter().any(|dt| {
                dt.abstract_data
                    .path()
                    .is_some_and(|p| p.file == path.to_string_lossy())
            })
        };
        assert!(
            survives(&ghost),
            "ghost row must survive while its path is leased"
        );
        assert!(
            thumbnail.exists(),
            "thumbnail must survive while the path is leased"
        );

        drop(lease);

        index_album_with_root("/scanned_root", false, None).expect("second scan accepted");
        wait_for_index_completion();

        let deadline = Instant::now() + Duration::from_secs(5);
        while survives(&ghost) {
            assert!(
                Instant::now() <= deadline,
                "ghost row must be pruned after the lease is released"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            !thumbnail.exists(),
            "thumbnail must be removed with the confirmed record"
        );

        std::fs::remove_dir_all(&scanned).ok();
        reset_backend_state();
    }

    #[test]
    fn index_album_orphan_sidecar_survives_scan() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        let image_home = test_image_home();
        let sidecar = image_home.join("orphan.xmp");
        std::fs::write(&sidecar, b"<x:xmpmeta/>").expect("write orphan sidecar");

        index_album_with_root("/", false, None).expect("index_album accepted");
        wait_for_index_completion();

        assert!(
            sidecar.exists(),
            "orphan sidecar must not be deleted by the scan"
        );

        std::fs::remove_file(&sidecar).ok();
        reset_backend_state();
    }

    #[test]
    fn orphan_sidecars_returns_sidecar_without_media() {
        let dir = tempfile::tempdir().expect("temp dir");
        let sidecar = dir.path().join("orphan.xmp");
        fs::write(&sidecar, b"<x:xmpmeta/>").expect("write sidecar");

        let out = orphan_sidecars(vec![sidecar.clone()]);
        assert_eq!(out, vec![sidecar]);
    }

    #[test]
    fn orphan_sidecars_ignores_sidecar_with_sibling_media() {
        let dir = tempfile::tempdir().expect("temp dir");
        let sidecar = dir.path().join("photo.xmp");
        fs::write(&sidecar, b"<x:xmpmeta/>").expect("write sidecar");
        fs::write(dir.path().join("photo.jpg"), b"jpeg").expect("write media");

        let out = orphan_sidecars(vec![sidecar]);
        assert!(out.is_empty(), "got {out:?}");
    }

    #[test]
    fn orphan_sidecars_ignores_album_sidecar() {
        let dir = tempfile::tempdir().expect("temp dir");
        let sidecar = dir.path().join(".albuminfo.xmp");
        fs::write(&sidecar, b"<x:xmpmeta/>").expect("write sidecar");

        let out = orphan_sidecars(vec![sidecar]);
        assert!(out.is_empty(), "got {out:?}");
    }

    #[test]
    fn index_album_upload_tmp_not_indexed_and_swept() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();
        dir_album::reset_dir_album_cache();

        let image_home = test_image_home();
        let tmp_path = image_home.join(format!(".upload-residue.{}.0.tmp", u32::MAX));
        std::fs::write(&tmp_path, b"x").expect("write upload tmp residue");

        index_album_with_root("/", false, None).expect("index accepted");
        wait_for_index_completion();

        let tree = crate::storage::db::snapshot_tree();
        let indexed = tree.iter().any(|dt| {
            dt.abstract_data
                .path()
                .is_some_and(|p| p.file == tmp_path.to_string_lossy())
        });
        drop(tree);
        assert!(!indexed, "upload tmp residue must not be indexed");

        let removed = crate::init::sweep_temp_files(std::slice::from_ref(&image_home));
        assert_eq!(removed, 1, "sweep must remove the residue");
        assert!(!tmp_path.exists(), "residue must be gone after sweep");

        reset_backend_state();
    }
}
