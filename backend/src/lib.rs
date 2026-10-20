#![deny(unsafe_code)]
#[macro_use]
extern crate rocket;
use redb::{ReadableTable, ReadableTableMetadata};
use std::thread;
use std::time::Instant;

pub mod auth;
mod constant;
mod error;
mod frontend;
mod init;
mod model;
pub mod openapi;
pub mod openapi_components;
pub mod openapi_parity;
pub mod openapi_public;
mod process;
mod router;
pub mod spec_path;
mod storage;
mod tasks;
mod workflow;

// Re-exports for integration tests that need to access the real init path.
pub use model::config::{APP_CONFIG, AppConfig};
pub use model::config::{configured_data_home, configured_trivial_passwords};
pub use router::builder::build_rocket_with_config;
pub use storage::files::DATA_PATH;

use crate::init::initialize;
use crate::init::initialize_logger;
use crate::process::dir_album::init_dir_album_cache;
use crate::storage::db::METADATA_TABLE;
use crate::storage::db::TREE;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::start_watcher::StartWatcherTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use crate::tasks::looper::start_expire_check_loop;
use crate::tasks::runtime::{INDEX_RUNTIME, ROCKET_RUNTIME};
use model::metadata_record::MetadataRecord;

/// Pure boot-freshness predicate: launching is allowed unless the bootstrap
/// flag is set on a store that already has a password file.
///
/// `true` means "ok to launch". Bootstrap is a one-shot window: once
/// `<DATA_HOME>/auth/users.redb` exists the store is either provisioned or
/// needs repair, and re-opening enrollment on it would silently re-expose the
/// install. The failure is loud at boot rather than silent at request time.
#[must_use]
pub fn bootstrap_freshness_ok(flag: bool, passwd_exists: bool) -> bool {
    !(flag && passwd_exists)
}

/// Boot gate for the bootstrap window: called by [`run`] after
/// `initialize()` (config and `DATA_PATH` resolved) and before any thread
/// serves.
///
/// WARN-logs whenever the flag is set, and refuses to launch (returning
/// `Err`, which [`run`] maps to a non-zero exit) when the flag is set and a
/// password store already exists. The error names both remedies.
///
/// # Errors
/// Returns an error when [`bootstrap_freshness_ok`] is false.
pub(crate) fn bootstrap_boot_gate() -> anyhow::Result<()> {
    let flag = crate::model::config::with_config(|c| c.bootstrap_admin);
    if flag {
        warn!(
            "bootstrap_admin is enabled: the first administrator may be enrolled online while the user store is empty"
        );
    }
    let passwd_path = crate::auth::users::user_store_path();
    if bootstrap_freshness_ok(flag, passwd_path.exists()) {
        return Ok(());
    }
    anyhow::bail!(
        "bootstrap_admin is enabled but a password store already exists at {}: bootstrap already happened or the store needs repair. Remove the flag (--bootstrap-admin / PICASU_BOOTSTRAP_ADMIN / config `bootstrap_admin`), or inspect/repair the store with the `picasu-cmd` CLI.",
        passwd_path.display()
    );
}

#[allow(clippy::missing_panics_doc)]
pub fn run() {
    use tokio::signal::unix::{SignalKind, signal};

    // Initialize logger first thing
    initialize_logger();

    // Initialize core subsystems (Config, DB, FFmpeg checks).
    // Must run first so DATA_PATH is resolved from config.
    if let Err(err) = initialize() {
        error!("{err}");
        std::process::exit(1);
    }

    // Refuse to launch when --bootstrap-admin is set on a store that already
    // exists: either bootstrap happened (remove the flag) or the store needs
    // manual repair via the CLI. Fail loud at boot, before serving.
    if let Err(err) = bootstrap_boot_gate() {
        error!("{err}");
        std::process::exit(1);
    }

    // Load the directory→album mapping cache from disk (must run after initialize()).
    init_dir_album_cache();

    #[cfg(feature = "embed-frontend")]
    info!("Frontend Configuration: EMBEDDED (Assets compiled into binary)");
    #[cfg(not(feature = "embed-frontend"))]
    info!("Frontend Configuration: EXTERNAL (Loading from file system)");

    // Architecture: Isolate the Indexing/TUI runtime from the Rocket server runtime.

    // This prevents heavy blocking operations in the indexer from stalling web requests.
    let worker_handle = thread::spawn(move || {
        INDEX_RUNTIME.block_on(async {
            let start_time = Instant::now();
            let txn = TREE
                .in_disk
                .begin_write()
                .expect("failed to begin write transaction");

            {
                let table = txn
                    .open_table(METADATA_TABLE)
                    .expect("failed to open table");
                let total_count = table.len().expect("failed to get length");

                // Constraint: METADATA_TABLE stores mixed types (Albums and Media).
                // We must perform an O(N) scan to differentiate counts.
                let album_count = table
                    .iter()
                    .expect("failed to iterate table")
                    .filter_map(std::result::Result::ok)
                    .filter(|(_, guard)| matches!(guard.value(), MetadataRecord::Album(_)))
                    .count();

                let media_count = usize::try_from(total_count).unwrap_or(0) - album_count;

                info!(
                    duration = &*format!("{:?}", start_time.elapsed());
                    "Read {} photos/videos and {} albums from database.",
                    media_count, album_count
                );
            }

            txn.commit().expect("failed to commit transaction");

            BATCH_COORDINATOR.execute_batch_detached(StartWatcherTask);
            BATCH_COORDINATOR.execute_batch_detached(UpdateTreeTask);
            start_expire_check_loop();

            {
                let mut sigint =
                    signal(SignalKind::interrupt()).expect("failed to set SIGINT handler");
                let mut sigterm =
                    signal(SignalKind::terminate()).expect("failed to set SIGTERM handler");
                tokio::select! {
                    _ = sigint.recv() => info!("SIGINT received, worker shutting down."),
                    _ = sigterm.recv() => info!("SIGTERM received, worker shutting down."),
                }
            }
            info!("Worker thread shutting down.");
        });
    });

    let rocket_handle = thread::spawn(|| {
        info!("Rocket thread starting.");
        if let Err(e) = ROCKET_RUNTIME.block_on(async {
            let rocket = router::build_rocket().ignite().await?;
            #[cfg(feature = "auto-open-browser")]
            let port = rocket.config().port;
            let shutdown_handle = rocket.shutdown();

            // Manually handle SIGINT/SIGTERM to trigger graceful shutdown
            // since we are running outside the default global runtime.
            ROCKET_RUNTIME.spawn(async move {
                let mut sigint =
                    signal(SignalKind::interrupt()).expect("failed to set SIGINT handler");
                let mut sigterm =
                    signal(SignalKind::terminate()).expect("failed to set SIGTERM handler");
                tokio::select! {
                    _ = sigint.recv() => info!("SIGINT received, shutting down Rocket."),
                    _ = sigterm.recv() => info!("SIGTERM received, shutting down Rocket."),
                }
                shutdown_handle.notify();
            });

            // Open browser after server starts listening
            let launch_future = rocket.launch();
            #[cfg(feature = "auto-open-browser")]
            open_browser(port);
            launch_future.await.map_err(anyhow::Error::from)
        }) {
            error!("Rocket thread exited with an error: {}", e);
        }
    });

    worker_handle.join().expect("Worker thread panicked");
    rocket_handle.join().expect("Rocket thread panicked");
}

#[cfg(feature = "auto-open-browser")]
fn open_browser(port: u16) {
    let url = format!("http://localhost:{}", port);
    info!("Opening browser at {}", url);
    if let Err(e) = webbrowser::open(&url) {
        error!("Failed to open browser: {}", e);
    }
}
#[cfg(test)]
mod tests;
