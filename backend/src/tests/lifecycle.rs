#![cfg(test)]

//! Deterministic in-process composition test for shutdown ordering.
//!
//! Replaces the removed process-level `tests/shutdown.rs` timing assertion
//! with a check that `lifecycle::shutdown` returns and leaves the system in
//! a quiescent, closed state. No wall-clock bounds are asserted here; the
//! only timeouts are harness backstops.

use crate::tasks::lifecycle;
use crate::tests::bootstrap::{TEST_ENV, TEST_SERIAL_GUARD, reset_backend_state, test_image_home};
use snapfab::{PhotoSpec, generate_batch};
use std::fs;

const SCAN_ITEM_COUNT: usize = 8;

fn gen_photo(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create photo parent");
    }
    generate_batch(&[PhotoSpec {
        output: Some(path.to_string_lossy().into_owned()),
        format: Some("jpeg".into()),
        width: Some(4),
        height: Some(4),
        tags: None,
        exif_date: None,
        minimal: false,
    }])
    .expect("generate photo");
}

#[test]
fn shutdown_returns_and_closes_intake() {
    let _guard = TEST_SERIAL_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = &*TEST_ENV;
    reset_backend_state();

    let image_home = test_image_home();
    let dir = image_home.join("shutdown_compose");
    fs::create_dir_all(&dir).expect("create scan dir");
    let mut paths = Vec::with_capacity(SCAN_ITEM_COUNT);
    for index in 0..SCAN_ITEM_COUNT {
        let path = dir.join(format!("asset-{index:04}.jpg"));
        gen_photo(&path);
        paths.push(path);
    }

    let background = lifecycle::start();

    let runtime = &crate::tasks::runtime::RUNTIME;

    // Submit a known number of scan items before shutdown.
    for path in &paths {
        let path = path.clone();
        runtime.block_on(async {
            crate::tasks::queue::submit_scan(path, 77)
                .await
                .expect("submit accepted before shutdown");
        });
    }

    // (a) shutdown returns.
    runtime.block_on(lifecycle::shutdown(background));

    // (b) post-shutdown submits are rejected.
    let extra = dir.join("extra.jpg");
    gen_photo(&extra);
    let rejected_scan = runtime.block_on(crate::tasks::queue::submit_scan(extra.clone(), 77));
    assert!(
        rejected_scan.is_err(),
        "post-shutdown scan submit must be rejected"
    );
    let rejected_watcher =
        runtime.block_on(crate::tasks::queue::submit_watcher_index(extra.clone()));
    assert!(
        rejected_watcher.is_err(),
        "post-shutdown watcher submit must be rejected"
    );

    // (c) no scan is active afterwards.
    let idle = runtime.block_on(crate::tasks::actor::album_index::wait_for_active_index(
        std::time::Duration::from_secs(0),
    ));
    assert!(idle, "no scan should be active after shutdown");

    // (d) a writer barrier passes immediately after shutdown.
    runtime.block_on(crate::tasks::queue::writer::barrier());

    let _ = fs::remove_dir_all(&dir);
}
