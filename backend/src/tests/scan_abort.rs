#![cfg(test)]

//! Gate for C10 abort half: a scan whose path is busy (or whose generation
//! moved on) is dropped instead of flushing a stale record.

use crate::tests::bootstrap::{TEST_ENV, TEST_SERIAL_GUARD, reset_backend_state, test_image_home};
use std::fs;

#[test]
fn scan_result_dropped_when_path_busy() {
    let _guard = TEST_SERIAL_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = &*TEST_ENV;
    reset_backend_state();

    let image_home = test_image_home();
    let dir = image_home.join("scan_abort");
    fs::create_dir_all(&dir).expect("create dir");
    let photo = dir.join("missing.jpg");
    let _ = fs::remove_file(&photo);

    // A mutation holds the path for the whole scan attempt.
    let lease = crate::tasks::path_epoch::lease_paths([photo.clone()]);

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let result = rt.block_on(crate::workflow::process_file(photo.clone()));

    drop(lease);

    assert!(
        result.is_ok(),
        "a scan on a busy path must be dropped, not fail: {result:?}"
    );
}
