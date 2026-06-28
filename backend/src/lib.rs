#![deny(unsafe_code)]
#[macro_use]
extern crate rocket;
use redb::{ReadableTable, ReadableTableMetadata};
use std::thread;
use std::time::Instant;

mod constant;
mod error;
mod frontend;
mod init;
mod model;
// Auto-generated file may not exist on fresh clone, do not try to format it
#[rustfmt::skip]
pub mod openapi;
mod process;
mod router;
mod storage;
mod tasks;
mod workflow;

// Re-exports for integration tests that need to access the real init path.
pub use model::config::{APP_CONFIG, AppConfig};
pub use router::builder::build_rocket_with_config;
pub use storage::files::DATA_PATH;

use crate::init::initialize;
use crate::init::initialize_logger;
use crate::process::dir_album::init_dir_album_cache;
use crate::storage::db::DATA_TABLE;
use crate::storage::db::TREE;
use crate::storage::files::get_data_path;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::start_watcher::StartWatcherTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use crate::tasks::looper::start_expire_check_loop;
use crate::tasks::runtime::{INDEX_RUNTIME, ROCKET_RUNTIME};
use model::abstract_data::AbstractData;

fn migration() {
    let v4_db_path = get_data_path().join("db/index_v4.redb");

    if v4_db_path.exists() {
        eprintln!(
            "Old database format detected at: {}\n\n\
             This version cannot directly migrate from database v4.\n\
             Please follow these steps to upgrade safely:\n\
                           1. Downgrade Picasu to version 1.2.2.\n\
             2. Start the app (v1.2.2) to automatically migrate the database.\n\
             3. Once confirmed working, update back to this latest version.\n\n\
             Press Enter to exit...",
            v4_db_path.display()
        );

        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
        std::process::exit(1);
    }
}

#[allow(clippy::missing_panics_doc)]
pub fn run() {
    use tokio::signal::unix::{SignalKind, signal};

    // Initialize logger first thing
    initialize_logger();

    // Initialize core subsystems (Config, DB, FFmpeg checks).
    // Must run before migration() so DATA_PATH is resolved from config.
    initialize();

    migration();

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
                let table = txn.open_table(DATA_TABLE).expect("failed to open table");
                let total_count = table.len().expect("failed to get length");

                // Constraint: DATA_TABLE stores mixed types (Albums and Media).
                // We must perform an O(N) scan to differentiate counts.
                let album_count = table
                    .iter()
                    .expect("failed to iterate table")
                    .filter_map(std::result::Result::ok)
                    .filter(|(_, guard)| matches!(guard.value(), AbstractData::Album(_)))
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
