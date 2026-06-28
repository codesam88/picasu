use crate::error::handle_error;
use crate::model::config::APP_CONFIG;
use crate::model::media::is_valid_media_file;
use crate::storage::files::get_resolved_image_home;
use crate::tasks::runtime::INDEX_RUNTIME;
use anyhow::Result;
use log::{error, info};
use mini_executor::BatchTask;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
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

    // Get the raw path from config system
    let raw_image_path = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned")
        .image_home
        .clone();

    let Some(raw_image_path) = raw_image_path else {
        info!("No path to watch");
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

        if should_run
            && is_valid_media_file(&path)
            && let Some(image_root) = get_resolved_image_home()
            && let Ok(relative) = path.strip_prefix(&image_root)
            && let Err(e) = crate::workflow::index_image(relative, None).await
        {
            handle_error(e);
        }
    });
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

                _ => { /* ignore other kinds */ }
            }
        }
        Err(err) => {
            handle_error(anyhow::anyhow!("Watch error: {err:#?}"));
        }
    })
    .map_err(|e| anyhow::anyhow!("Failed to create watcher: {e}"))
}
