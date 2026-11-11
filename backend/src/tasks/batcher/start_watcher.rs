use crate::error::handle_error;
use crate::model::abstract_data::AbstractData;
use crate::model::media::{MediaOutcome, classify_media_file, is_valid_media_file};
use crate::storage::files::get_resolved_image_home;
use crate::storage::files::get_resolved_users_home;
use crate::tasks::queue::{submit_watcher_index, submit_watcher_remove};
use crate::tasks::runtime::spawn;
use anyhow::Result;
use log::{debug, error, info, warn};
use mini_executor::BatchTask;
use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tokio::time::{Duration, sleep};
use walkdir::WalkDir;

static IS_WATCHING: AtomicBool = AtomicBool::new(false);

static WATCHER_HANDLE: LazyLock<Mutex<Option<RecommendedWatcher>>> =
    LazyLock::new(|| Mutex::new(None));

/// The last trigger time for each path
static DEBOUNCE_POOL: LazyLock<Mutex<HashMap<PathBuf, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct StartWatcherTask;

impl BatchTask for StartWatcherTask {
    fn batch_run(_: Vec<Self>) -> impl std::future::Future<Output = ()> + Send {
        if let Err(e) = start_watcher_task_internal() {
            handle_error(e);
        }
        std::future::ready(())
    }
}

/// Stop watching. Dropping the notify watcher ends its event thread; any
/// debounce task already in flight is rejected by the intake shutdown gate.
pub(crate) fn stop_watcher() {
    *WATCHER_HANDLE.lock().expect("lock poisoned") = None;
    IS_WATCHING.store(false, Ordering::SeqCst);
}

/// Reload watcher with the new path from config
pub fn reload_watcher() {
    info!("Reloading watcher...");

    {
        let mut guard = WATCHER_HANDLE.lock().expect("lock poisoned");
        *guard = None; // Drop old watcher
    }

    // Reset the flag so we can start again
    IS_WATCHING.store(false, Ordering::SeqCst);

    if let Err(e) = start_watcher_task_internal() {
        error!("Failed to reload watcher: {e}");
    }
}

fn start_watcher_task_internal() -> Result<()> {
    // Fast-path: already running.
    if IS_WATCHING.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let (fs_notify_watcher_enabled, raw_image_path, raw_users_path) = {
        let cfg = crate::model::config::with_config(crate::model::config::AppConfig::clone);
        (
            cfg.fs_notify_watcher,
            cfg.image_home.clone(),
            cfg.users_home.clone(),
        )
    };

    if !fs_notify_watcher_enabled {
        info!("fs_notify_watcher disabled — skipping filesystem watcher");
        IS_WATCHING.store(false, Ordering::SeqCst);
        return Ok(());
    }

    let Some(raw_image_path) = raw_image_path else {
        warn!("No path to watch");
        IS_WATCHING.store(false, Ordering::SeqCst);
        return Ok(());
    };

    // Path is already absolute from config
    let image_path = raw_image_path;

    // Build the watcher.
    let mut watcher = new_watcher(image_path.clone(), raw_users_path.clone())?;
    if image_path.exists() {
        watcher
            .watch(&image_path, RecursiveMode::Recursive)
            .map_err(|e| anyhow::anyhow!("Failed to watch path {}: {e}", image_path.display()))?;
        info!("Watching path {}", image_path.display());
    } else {
        error!("Path not found, skipped: {}", image_path.display());
    }

    if let Some(users_path) = raw_users_path
        && users_path.exists()
    {
        watcher
            .watch(&users_path, RecursiveMode::Recursive)
            .map_err(|e| anyhow::anyhow!("Failed to watch path {}: {e}", users_path.display()))?;
        info!("Watching path {}", users_path.display());
    }

    // Store it globally to keep it alive.
    *WATCHER_HANDLE.lock().expect("lock poisoned") = Some(watcher);
    Ok(())
}

fn submit_to_debounce_pool(path: PathBuf) {
    let now = Instant::now();

    {
        let mut pool = DEBOUNCE_POOL.lock().expect("lock poisoned");
        pool.insert(path.clone(), now);
    }

    // Start a task to check after 1 second (running on the shared runtime)
    spawn(async move {
        sleep(Duration::from_secs(1)).await;

        // Check if there are any events for the same path within this 1 second (i.e., whether the last time is still now)
        let should_run = {
            let mut pool = DEBOUNCE_POOL.lock().expect("lock poisoned");
            match pool.get(&path).copied() {
                Some(last) if last == now => {
                    // Not updated, remove and execute
                    pool.remove(&path);
                    true
                }
                _ => false, // There are later events or it has been removed, abandon this time
            }
        };

        let watcher_still_enabled = crate::model::config::with_config(|c| c.fs_notify_watcher);

        let under_root = get_resolved_image_home().is_some_and(|r| path.starts_with(&r))
            || get_resolved_users_home().is_some_and(|r| path.starts_with(&r));

        if should_run && watcher_still_enabled && under_root {
            match classify_media_file(&path) {
                MediaOutcome::Index => {
                    if let Err(e) = submit_watcher_index(path).await {
                        handle_error(e);
                    }
                }
                MediaOutcome::Skip(reason) => {
                    debug!("Ignoring unrecognized file {}: {reason:?}", path.display());
                }
            }
        }
    });
}

/// Handle an external file removal: find the DB record that owns `path`,
/// clear the record's path, and if no path remains remove the
/// record + thumbnail.
fn submit_removal_to_watcher(path: PathBuf) {
    spawn(async move {
        // The worker checks the path lease and generation before pruning, so
        // an API move/delete echo cannot remove a row the request still owns.
        if let Err(e) = submit_watcher_remove(path).await {
            warn!("removal submit failed: {e:#}");
        }
    });
}

/// What a watcher-observed removal resolves to. The caller submits the
/// record to the serial writer; this function performs no DB writes itself
/// so the writer stays the sole background `TREE` write path.
pub(crate) enum RemovalAction {
    /// The record keeps a path (multi-file asset lost one file): re-insert.
    Insert(AbstractData),
    /// The record lost its last path: remove the record.
    Remove(AbstractData),
}

/// Handle an external file removal: find the DB record that owns `path`,
/// clear the record's path, and report whether the record (and its
/// thumbnail) survives or must be removed.
pub(crate) fn handle_removed_file(removed: &Path) -> Option<RemovalAction> {
    use crate::process::path::prune_asset_path;

    // Scan in-memory tree to find the record that owns this path.
    let removed_str = removed.to_string_lossy();
    let matching: Option<AbstractData> = {
        // Short watcher-side scan on the blocking pool; no Rayon work is
        // nested while the guard is held.
        let tree = crate::storage::db::snapshot_tree();
        tree.iter()
            .find(|dt| {
                dt.abstract_data
                    .path()
                    .iter()
                    .any(|a| a.file == removed_str.as_ref())
            })
            .map(|dt| dt.abstract_data.clone())
    };

    let Some(mut abstract_data) = matching else {
        return None; // Unknown file, nothing to do.
    };

    // prune_asset_path removes the file (already gone — harmless NotFound),
    // clears the record's path, and removes the thumbnail once the path is
    // gone.
    let has_path = prune_asset_path(&mut abstract_data, removed);

    if has_path {
        Some(RemovalAction::Insert(abstract_data))
    } else {
        Some(RemovalAction::Remove(abstract_data))
    }
}

/// Map a set of watcher-observed paths to a scan-root selector for a full
/// rescan: `"global"` when every path is under the image root, `"personal"`
/// when every path is under the users root, and `"auto"` (both roots) for a
/// mixed or unknown set.
pub(crate) fn rescan_mode_for(
    paths: &[PathBuf],
    image_root: &Path,
    users_root: Option<&Path>,
) -> &'static str {
    if paths.is_empty() {
        return "auto";
    }
    let mut all_image = true;
    let mut all_users = true;
    for path in paths {
        let under_image = path.starts_with(image_root);
        let under_users = users_root.is_some_and(|r| path.starts_with(r));
        if !under_image {
            all_image = false;
        }
        if !under_users {
            all_users = false;
        }
        if !all_image && !all_users {
            return "auto";
        }
    }
    if all_image {
        "global"
    } else if all_users {
        "personal"
    } else {
        "auto"
    }
}

fn new_watcher(image_root: PathBuf, users_root: Option<PathBuf>) -> Result<RecommendedWatcher> {
    notify::recommended_watcher(move |result: Result<Event, notify::Error>| match result {
        Ok(event) => handle_event(event, &image_root, users_root.as_deref()),
        Err(err) => {
            handle_error(anyhow::anyhow!("Watch error: {err:#?}"));
        }
    })
    .map_err(|e| anyhow::anyhow!("Failed to create watcher: {e}"))
}

/// Pure mapping of a watcher event to (old path, new path) for renames.
///
/// Returns `Some` only for the rename modes that carry a known direction;
/// other events yield `(None, None)` so callers fall back to the generic
/// modify handling.
pub(crate) fn rename_paths(event: &Event) -> (Option<PathBuf>, Option<PathBuf>) {
    match event.kind {
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            let mut it = event.paths.iter();
            (it.next().cloned(), it.next().cloned())
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            (event.paths.first().cloned(), None)
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => (None, event.paths.first().cloned()),
        _ => (None, None),
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn handle_event(event: Event, image_root: &Path, users_root: Option<&Path>) {
    // Overflow / rescan: a backend may have missed events, so force a
    // full rescan of the affected root instead of reacting to the
    // (possibly incomplete) event stream.
    if event.need_rescan() {
        let mode = rescan_mode_for(&event.paths, image_root, users_root);
        if let Err(e) =
            crate::tasks::actor::album_index::index_album_with_root("/", false, Some(mode))
        {
            if e.kind == crate::error::ErrorKind::Conflict {
                debug!("rescan submit skipped (index already running): {e}");
            } else {
                warn!("rescan submit failed: {e:#}");
            }
        } else {
            info!("submitted full rescan ({mode}) after watcher overflow/rescan");
        }
    }

    match event.kind {
        EventKind::Create(_) => {
            let mut path_list: HashSet<PathBuf> = HashSet::new();

            for path in event.paths {
                if path.is_file() {
                    path_list.insert(path);
                } else if path.is_dir() {
                    WalkDir::new(&path)
                        .into_iter()
                        .filter_map(std::result::Result::ok)
                        .filter(|dir_entry| dir_entry.file_type().is_file())
                        .for_each(|dir_entry| {
                            path_list.insert(dir_entry.into_path());
                        });
                }
            }

            for path in path_list {
                if is_valid_media_file(&path) {
                    submit_to_debounce_pool(path);
                }
            }
        }

        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            // notify delivers (from, to) in this order.
            let (old, new) = rename_paths(&event);
            // Directory rename (REQ-3): rewrite every descendant row to the
            // new prefix in one step, preserving assetId and sidecar
            // association, without decoding. Detected after the FS move via
            // `new.is_dir()`.
            if let (Some(old), Some(new)) = (&old, &new)
                && new.is_dir()
            {
                let old = old.clone();
                let new = new.clone();
                spawn(async move {
                    let result = tokio::task::spawn_blocking(move || {
                        // A subtree lease makes every verify or watcher removal
                        // under the old or new prefix skip while rows are
                        // rewritten.
                        let _lease = crate::tasks::path_epoch::lease_subtree(old.clone());
                        let _new_lease = crate::tasks::path_epoch::lease_subtree(new.clone());
                        crate::tasks::path_epoch::bump(&old);
                        crate::tasks::path_epoch::bump(&new);
                        crate::process::dir_album::rewrite_dir_album_cache_prefix(&old, &new);
                        crate::router::put::assign_album::update_asset_tables_after_dir_move(
                            &old, &new,
                        )
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                        crate::storage::tree_view::bump_tree_epoch();
                        Ok::<(), anyhow::Error>(())
                    })
                    .await;
                    match result {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => {
                            warn!("directory rename row rewrite failed: {e:#}");
                        }
                        Err(e) => warn!("directory rename task panicked: {e}"),
                    }
                });
            } else {
                // File rename (REQ-2): submit a removal for the old path and
                // an index for the new path so the row follows without
                // waiting for a sweep.
                if let Some(old) = old
                    && is_valid_media_file(&old)
                {
                    submit_removal_to_watcher(old);
                }
                if let Some(new) = new
                    && is_valid_media_file(&new)
                {
                    submit_to_debounce_pool(new);
                }
            }
        }

        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            // Directory case: the old path is already gone and there is no
            // destination here; a RenameMode::To/Both event carries the
            // destination, so directory renames are ignored in this arm.
            for path in &event.paths {
                if is_valid_media_file(path) {
                    submit_removal_to_watcher(path.clone());
                }
            }
        }

        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            for path in &event.paths {
                if is_valid_media_file(path) {
                    submit_to_debounce_pool(path.clone());
                }
            }
        }

        EventKind::Modify(_) => {
            let mut path_list: HashSet<PathBuf> = HashSet::new();

            for path in event.paths {
                if path.is_file() {
                    path_list.insert(path);
                }
            }

            for path in path_list {
                if is_valid_media_file(&path) {
                    submit_to_debounce_pool(path);
                }
            }
        }

        EventKind::Remove(_) => {
            for path in event.paths {
                if is_valid_media_file(&path) {
                    submit_removal_to_watcher(path);
                }
            }
        }

        _ => { /* ignore other kinds */ }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::actor::album_index::{AlbumIndexState, album_index_status};
    use notify::event::Flag;
    use std::time::{Duration, Instant};

    #[test]
    fn rescan_event_submits_a_scan() {
        let _guard = crate::tests::bootstrap::lock();
        let _client = crate::tests::bootstrap::setup_image_home();
        let image_home = crate::tests::bootstrap::test_image_home();
        let event = Event::new(EventKind::Other)
            .set_flag(Flag::Rescan)
            .add_path(image_home.join("a.jpg"));

        handle_event(event, &image_home, None);

        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let status = album_index_status();
            if !matches!(status.state, AlbumIndexState::Running) {
                assert!(
                    matches!(status.state, AlbumIndexState::Completed),
                    "rescan state: {:?}",
                    status.state
                );
                break;
            }
            assert!(
                Instant::now() <= deadline,
                "rescan did not complete within 30s"
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        crate::tests::bootstrap::reset_backend_state();
    }

    #[test]
    fn rescan_mode_image_path_is_global() {
        let image_root = PathBuf::from("/data/images");
        let users_root = Some(PathBuf::from("/data/users"));
        let event = Event::new(EventKind::Other)
            .set_flag(Flag::Rescan)
            .add_path(image_root.join("a.jpg"));
        assert_eq!(
            rescan_mode_for(&event.paths, &image_root, users_root.as_deref()),
            "global"
        );
    }

    #[test]
    fn rescan_mode_users_path_is_personal() {
        let image_root = PathBuf::from("/data/images");
        let users_root = Some(PathBuf::from("/data/users"));
        let event = Event::new(EventKind::Other)
            .set_flag(Flag::Rescan)
            .add_path(users_root.as_deref().expect("users root").join("u/a.jpg"));
        assert_eq!(
            rescan_mode_for(&event.paths, &image_root, users_root.as_deref()),
            "personal"
        );
    }

    #[test]
    fn rescan_mode_mixed_or_unknown_is_auto() {
        let image_root = PathBuf::from("/data/images");
        let users_root = Some(PathBuf::from("/data/users"));
        let event = Event::new(EventKind::Other)
            .set_flag(Flag::Rescan)
            .add_path(image_root.join("a.jpg"))
            .add_path(users_root.as_deref().expect("users root").join("u/b.jpg"));
        assert_eq!(
            rescan_mode_for(&event.paths, &image_root, users_root.as_deref()),
            "auto"
        );

        let event2 = Event::new(EventKind::Other)
            .set_flag(Flag::Rescan)
            .add_path(PathBuf::from("/tmp/other.jpg"));
        assert_eq!(
            rescan_mode_for(&event2.paths, &image_root, users_root.as_deref()),
            "auto"
        );
    }
    #[test]
    fn rename_paths_both_returns_old_and_new() {
        let old = PathBuf::from("/tmp/a.jpg");
        let new = PathBuf::from("/tmp/b.jpg");
        let event = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(old.clone())
            .add_path(new.clone());
        assert_eq!(rename_paths(&event), (Some(old), Some(new)));
    }

    #[test]
    fn rename_paths_from_returns_old_only() {
        let old = PathBuf::from("/tmp/a.jpg");
        let event =
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From))).add_path(old.clone());
        assert_eq!(rename_paths(&event), (Some(old), None));
    }

    #[test]
    fn rename_paths_to_returns_new_only() {
        let new = PathBuf::from("/tmp/b.jpg");
        let event =
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To))).add_path(new.clone());
        assert_eq!(rename_paths(&event), (None, Some(new)));
    }

    #[test]
    fn rename_paths_other_returns_none() {
        let event = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any)))
            .add_path(PathBuf::from("/tmp/a.jpg"));
        assert_eq!(rename_paths(&event), (None, None));
    }
}
