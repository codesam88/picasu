use crate::error::handle_error;
use crate::model::abstract_data::AbstractData;
use crate::model::config::APP_CONFIG;
use crate::model::media::is_valid_media_file;
use crate::storage::db::TREE;
use crate::storage::files::get_resolved_image_home;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::flush_tree::FlushTreeTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use crate::tasks::runtime::INDEX_RUNTIME;
use anyhow::Result;
use log::{error, info, warn};
use mini_executor::BatchTask;
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
    async fn batch_run(_: Vec<Self>) {
        if let Err(e) = start_watcher_task_internal() {
            handle_error(e);
        }
    }
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

    let (fs_notify_watcher_enabled, raw_image_path) = {
        let cfg = APP_CONFIG
            .get()
            .expect("APP_CONFIG not initialized")
            .read()
            .expect("lock poisoned");
        (cfg.fs_notify_watcher, cfg.image_home.clone())
    };

    if !fs_notify_watcher_enabled {
        info!("fs_notify_watcher disabled — skipping filesystem watcher");
        IS_WATCHING.store(false, Ordering::SeqCst);
        return Ok(());
    }

    let Some(raw_image_path) = raw_image_path else {
        info!("No path to watch");
        IS_WATCHING.store(false, Ordering::SeqCst);
        return Ok(());
    };

    // Path is already absolute from config
    let image_path = raw_image_path;

    // Build the watcher.
    let mut watcher = new_watcher()?;
    if image_path.exists() {
        watcher
            .watch(&image_path, RecursiveMode::Recursive)
            .map_err(|e| anyhow::anyhow!("Failed to watch path {}: {e}", image_path.display()))?;
        info!("Watching path {}", image_path.display());
    } else {
        error!("Path not found, skipped: {}", image_path.display());
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

    // Start a task to check after 1 second (running on INDEX_RUNTIME)
    INDEX_RUNTIME.spawn(async move {
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

        let watcher_still_enabled = APP_CONFIG
            .get()
            .expect("APP_CONFIG not initialized")
            .read()
            .expect("lock poisoned")
            .fs_notify_watcher;

        if should_run
            && watcher_still_enabled
            && is_valid_media_file(&path)
            && let Some(image_root) = get_resolved_image_home()
            && let Ok(relative) = path.strip_prefix(&image_root)
            && let Err(e) = crate::workflow::index_image(relative, None).await
        {
            handle_error(e);
        }
    });
}

/// Handle an external file removal: find the DB record that owns `path`,
/// remove that alias, and if no aliases remain remove the record + thumbnail.
fn submit_removal_to_watcher(path: PathBuf) {
    INDEX_RUNTIME.spawn(async move {
        if let Err(e) = tokio::task::spawn_blocking(move || handle_removed_file(&path)).await {
            warn!("Join error in removal handler: {e}");
        }
        let _ = BATCH_COORDINATOR
            .execute_batch_waiting(FlushTreeTask::insert(vec![]))
            .await;
        if let Err(e) = BATCH_COORDINATOR
            .execute_batch_waiting(UpdateTreeTask)
            .await
        {
            warn!("Failed to update tree after file removal: {e}");
        }
    });
}

fn handle_removed_file(removed: &Path) {
    // Scan in-memory tree to find the record that owns this path.
    let removed_str = removed.to_string_lossy();
    let matching: Option<AbstractData> = {
        let tree = TREE.in_memory.read().expect("lock poisoned");
        tree.iter()
            .find(|dt| {
                dt.abstract_data
                    .alias()
                    .iter()
                    .any(|a| a.file == removed_str.as_ref())
            })
            .map(|dt| dt.abstract_data.clone())
    };

    let Some(mut abstract_data) = matching else {
        return; // Unknown file, nothing to do.
    };

    let remaining_aliases: Vec<_> = abstract_data
        .alias()
        .iter()
        .filter(|a| a.file != removed_str.as_ref())
        .cloned()
        .collect();

    if remaining_aliases.is_empty() {
        // Last alias gone — delete thumbnail and remove the DB record.
        let thumb = abstract_data.compressed_path();
        if thumb.exists()
            && let Err(e) = std::fs::remove_file(&thumb)
        {
            warn!("Failed to delete thumbnail {}: {e}", thumb.display());
        }
        BATCH_COORDINATOR.execute_batch_detached(FlushTreeTask::remove(vec![abstract_data]));
    } else {
        // Still other aliases — just prune this one.
        if let Some(alias_mut) = abstract_data.alias_mut() {
            *alias_mut = remaining_aliases;
        }
        BATCH_COORDINATOR.execute_batch_detached(FlushTreeTask::insert(vec![abstract_data]));
    }
}

fn new_watcher() -> Result<RecommendedWatcher> {
    notify::recommended_watcher(move |result: Result<Event, notify::Error>| match result {
        Ok(event) => {
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
        Err(err) => {
            handle_error(anyhow::anyhow!("Watch error: {err:#?}"));
        }
    })
    .map_err(|e| anyhow::anyhow!("Failed to create watcher: {e}"))
}
