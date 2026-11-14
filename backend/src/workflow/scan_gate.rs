//! Test-only pause between the scan's metadata stage and its flush.
//!
//! A scan that captured its snapshot and then races a mutation must drop its
//! result instead of flushing stale state (`workflow::process_file`'s
//! try-and-abort). Skinny test hooks here make that window observable: a test
//! arms the gate, submits the scan, waits for [`reached`], performs the
//! competing mutation, and releases the scan to assert it skipped the flush.
//!
//! Mirrors the `FORCE_CASCADE_PANIC` test-hook pattern in
//! `tasks/queue/writer.rs`; nothing here is compiled into release builds.

use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

static ARMED: AtomicBool = AtomicBool::new(false);
static REACHED: Notify = Notify::const_new();
static RELEASE: Notify = Notify::const_new();

/// Arm the gate: the next scan pauses after its metadata stage.
pub(crate) fn arm() {
    ARMED.store(true, Ordering::SeqCst);
}

/// Disarm and release a parked scan.
pub(crate) fn disarm() {
    ARMED.store(false, Ordering::SeqCst);
    RELEASE.notify_one();
}

/// Wait until a scan has reached the pause point.
pub(crate) async fn reached() {
    REACHED.notified().await;
}

/// Release a scan parked at the pause point.
pub(crate) fn release() {
    RELEASE.notify_one();
}

/// Pause here when the test gate is armed; a no-op otherwise.
pub(crate) async fn pause() {
    if !ARMED.load(Ordering::SeqCst) {
        return;
    }
    REACHED.notify_one();
    RELEASE.notified().await;
}
