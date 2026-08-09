use crate::error::handle_error;
use crate::model::abstract_data::AbstractData;
use crate::model::config::APP_CONFIG;
use crate::model::media::is_valid_media_file;
use crate::storage::db::TREE;
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

/// One watcher handle per namespace (keyed by namespace name).
static WATCHER_HANDLES: LazyLock<Mutex<HashMap<String, RecommendedWatcher>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The last trigger time for each `(namespace, relative_path)`.
static DEBOUNCE_POOL: LazyLock<Mutex<HashMap<(String, PathBuf), Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct StartWatcherTask;

impl BatchTask for StartWatcherTask {
    async fn batch_run(_: Vec<Self>) {
        if let Err(e) = start_watcher_task_internal() {
            handle_error(e);
        }
    }
}

/// Reload watchers for all non-trash namespace roots.
pub fn reload_watcher() {
    info!("Reloading watchers...");

    {
        let mut guard = WATCHER_HANDLES.lock().expect("lock poisoned");
        guard.clear(); // Drop all old watchers
    }

    // Reset the flag so we can start again
    IS_WATCHING.store(false, Ordering::SeqCst);

    if let Err(e) = start_watcher_task_internal() {
        error!("Failed to reload watchers: {e}");
    }
}

fn start_watcher_task_internal() -> Result<()> {
    // Fast-path: already running.
    if IS_WATCHING.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let (fs_notify_watcher_enabled, namespaces) = {
        let cfg = APP_CONFIG
            .get()
            .expect("APP_CONFIG not initialized")
            .read()
            .expect("lock poisoned");
        (cfg.fs_notify_watcher, cfg.namespaces.clone())
    };

    if !fs_notify_watcher_enabled {
        info!("fs_notify_watcher disabled — skipping filesystem watcher");
        IS_WATCHING.store(false, Ordering::SeqCst);
        return Ok(());
    }

    if namespaces.is_empty() {
        info!("No namespaces configured — skipping filesystem watcher");
        IS_WATCHING.store(false, Ordering::SeqCst);
        return Ok(());
    }

    let mut handles = WATCHER_HANDLES.lock().expect("lock poisoned");

    for ns in &namespaces {
        // Trash namespace is not watched
        if ns.name == "trash" {
            info!("Skipping trash namespace for watching");
            continue;
        }

        let root = &ns.path;
        if !root.exists() {
            warn!(
                "Namespace root not found, skipped: {} ({})",
                ns.name,
                root.display()
            );
            continue;
        }

        let namespace = ns.name.clone();
        let mut watcher = new_namespace_watcher(namespace.clone())?;
        if let Err(e) = watcher.watch(root, RecursiveMode::Recursive) {
            warn!(
                "Failed to watch namespace root {} ({}): {e}",
                namespace,
                root.display()
            );
            continue;
        }
        info!("Watching namespace '{}' at {}", namespace, root.display());
        handles.insert(namespace, watcher);
    }

    Ok(())
}

fn submit_to_debounce_pool(namespace: String, relative: PathBuf) {
    let now = Instant::now();
    let key = (namespace.clone(), relative.clone());

    {
        let mut pool = DEBOUNCE_POOL.lock().expect("lock poisoned");
        pool.insert(key.clone(), now);
    }

    // Start a task to check after 1 second (running on INDEX_RUNTIME)
    INDEX_RUNTIME.spawn(async move {
        sleep(Duration::from_secs(1)).await;

        // Check if there are any events for the same path within this 1 second (i.e., whether the last time is still now)
        let should_run = {
            let mut pool = DEBOUNCE_POOL.lock().expect("lock poisoned");
            match pool.get(&key).copied() {
                Some(last) if last == now => {
                    // Not updated, remove and execute
                    pool.remove(&key);
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
            && let Err(e) =
                crate::workflow::index_image(&namespace, std::path::Path::new(&relative), None)
                    .await
        {
            handle_error(e);
        }
    });
}

/// Handle an external file removal: find the DB record that owns
/// `(namespace, relative)`, remove that alias, and if no aliases remain
/// remove the record + thumbnail.
fn submit_removal_to_watcher(namespace: String, relative: PathBuf) {
    INDEX_RUNTIME.spawn(async move {
        if let Err(e) =
            tokio::task::spawn_blocking(move || handle_removed_file(&namespace, &relative)).await
        {
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

fn handle_removed_file(namespace: &str, relative: &Path) {
    // Scan in-memory tree to find the record that owns this (namespace, relative) alias.
    let matching: Option<AbstractData> = {
        let tree = TREE.in_memory.read().expect("lock poisoned");
        tree.iter()
            .find(|dt| {
                dt.abstract_data.alias().iter().any(|a| {
                    a.namespace == namespace && a.file == relative.to_string_lossy().as_ref()
                })
            })
            .map(|dt| dt.abstract_data.clone())
    };

    let Some(mut abstract_data) = matching else {
        return; // Unknown file, nothing to do.
    };

    let remaining_aliases: Vec<_> = abstract_data
        .alias()
        .iter()
        .filter(|a| !(a.namespace == namespace && a.file == relative.to_string_lossy().as_ref()))
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

fn new_namespace_watcher(namespace: String) -> Result<RecommendedWatcher> {
    let ns_label = namespace.clone();
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
                        // Resolve the namespace root to compute relative path
                        if let Some(root) = crate::process::namespace::namespace_root(&namespace)
                            && let Ok(relative) = path.strip_prefix(&root)
                            && is_valid_media_file(&path)
                        {
                            submit_to_debounce_pool(namespace.clone(), relative.to_path_buf());
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
                        if let Some(root) = crate::process::namespace::namespace_root(&namespace)
                            && let Ok(relative) = path.strip_prefix(&root)
                            && is_valid_media_file(&path)
                        {
                            submit_to_debounce_pool(namespace.clone(), relative.to_path_buf());
                        }
                    }
                }

                EventKind::Remove(_) => {
                    for path in event.paths {
                        if let Some(root) = crate::process::namespace::namespace_root(&namespace)
                            && let Ok(relative) = path.strip_prefix(&root)
                            && is_valid_media_file(&path)
                        {
                            submit_removal_to_watcher(namespace.clone(), relative.to_path_buf());
                        }
                    }
                }

                _ => { /* ignore other kinds */ }
            }
        }
        Err(err) => {
            handle_error(anyhow::anyhow!(
                "Watch error in namespace '{namespace}': {err:#?}"
            ));
        }
    })
    .map_err(|e| anyhow::anyhow!("Failed to create watcher for namespace '{ns_label}': {e}"))
}
