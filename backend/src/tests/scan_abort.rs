#![cfg(test)]

//! Gate for C10 abort half: a scan whose path is busy (or whose generation
//! moved on) is dropped instead of flushing a stale record.

use crate::tests::bootstrap::{
    TEST_ENV, TEST_SERIAL_GUARD, lock, reset_backend_state, setup_image_home, test_image_home,
};
use crate::tests::fixtures::authz::create_album;
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

/// A scan that has already built its snapshot must drop it when a competing
/// metadata edit bumps the path generation: the row keeps the edit instead of
/// accepting the scan's stale payload. Pins OP-1's `aspectsScanSupersedesEditTest`
/// try-and-abort half at the `process_file` flush boundary (the gate pauses the
/// scan after its metadata stage).
#[test]
fn scan_superseded_by_edit_keeps_the_edit() {
    let _guard = lock();
    let client = setup_image_home();
    let (_album_id, asset_id) = create_album(&client, "/scan_edit/photo.jpg");
    let photo = test_image_home().join("scan_edit/photo.jpg");

    // Arm the scan gate, then queue the scan. The worker runs process_file and
    // parks after reading the pre-edit sidecar.
    crate::workflow::scan_gate::arm();
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let job_id = 9_001_001;
    rt.block_on(crate::tasks::queue::submit_scan(photo.clone(), job_id))
        .expect("submit scan");
    rt.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            crate::workflow::scan_gate::reached(),
        )
        .await
    })
    .expect("scan must reach the post-metadata flush gate");

    // Competing edit through the same write-sidecar-then-store contract the API
    // routes use; it leases the path and bumps its generation.
    let mut edited = crate::process::transitor::compose_by_asset_id(&asset_id)
        .expect("compose edited asset")
        .expect("asset present");
    edited.set_rating(Some(5));
    let edited_id = arrayvec::ArrayString::<64>::from(&asset_id).expect("asset id fits");
    crate::process::sidecar_edit::commit_metadata_edits(
        &[crate::process::sidecar_edit::EditedItem {
            asset_id: edited_id,
            data: edited,
        }],
        |id, data: &crate::model::abstract_data::AbstractData| {
            crate::process::transitor::store_metadata_record(id, data, None)
                .map_err(|e| crate::error::AppError::from_err(crate::error::ErrorKind::Database, e))
        },
    )
    .expect("metadata edit must commit");
    assert_eq!(
        crate::process::transitor::compose_by_asset_id(&asset_id)
            .expect("compose after edit")
            .expect("asset present")
            .rating(),
        Some(5),
        "edit must be stored before the scan is released"
    );

    // Release the parked scan; its stale flush must be dropped.
    crate::workflow::scan_gate::release();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    rt.block_on(crate::tasks::queue::drain_intake(job_id, &cancel));
    // A stale flush would land through the serial writer after the worker
    // completes, so wait for the writer before asserting.
    rt.block_on(crate::tasks::queue::writer::barrier());
    crate::workflow::scan_gate::disarm();

    assert_eq!(
        crate::process::transitor::compose_by_asset_id(&asset_id)
            .expect("compose after scan")
            .expect("asset present")
            .rating(),
        Some(5),
        "a superseded scan must not flush its pre-edit snapshot over the edit"
    );
    assert_eq!(
        crate::storage::asset_store::get_asset_id_by_path(&photo.to_string_lossy())
            .expect("read path mapping")
            .expect("path mapping")
            .as_str(),
        asset_id,
        "the edit must not change the path-primary asset identity"
    );
}
