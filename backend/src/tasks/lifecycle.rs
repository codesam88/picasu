//! Background subsystem ownership and shutdown ordering.
//!
//! The watcher, expire loop, backstop loop, scan job and intake queue all
//! produce or accept work. Shutdown stops the producers first, discards work
//! that has not started, waits for the in-flight scan job, and only then
//! drains the serial writer. Starting the writer drain while a scanner can
//! still submit flushes turns a bounded drain into an unbounded one.

use tokio::task::JoinHandle;

pub(crate) struct Background {
    backstop: Option<JoinHandle<()>>,
    expire: JoinHandle<()>,
}

/// Start the periodic loops that have no owning request. The watcher starts
/// through its batch task; [`shutdown`] stops it alongside these loops.
pub(crate) fn start() -> Background {
    Background {
        backstop: crate::tasks::looper::start_backstop_loop(),
        expire: crate::tasks::looper::start_expire_check_loop(),
    }
}

/// Stop producers, cancel and await the active scan, then drain the writer.
pub(crate) async fn shutdown(background: Background) {
    // Stop new filesystem events and reject new intake items before the
    // scan is cancelled, so nothing new enters behind the drain.
    crate::tasks::batcher::start_watcher::stop_watcher();
    crate::tasks::queue::begin_shutdown();

    if let Some(handle) = background.backstop {
        handle.abort();
    }
    background.expire.abort();

    let _ = crate::tasks::actor::album_index::cancel_album_index();
    let stopped =
        crate::tasks::actor::album_index::wait_for_active_index(std::time::Duration::from_secs(10))
            .await;
    if !stopped {
        log::warn!("scan job did not stop within 10s; draining writer anyway");
    }

    crate::tasks::queue::shutdown_writer().await;
}
