#![cfg(test)]

//! Reproduction: the content-addressed thumbnail is rewritten in place, so a
//! concurrent `GET /object/compressed/...` can observe a truncated file.
//!
//! `generate_thumbnail_for_image` persists with `save_with_format`, which
//! truncates the existing file before writing the new bytes. Re-indexing an
//! already-indexed file runs that pipeline again — the filesystem watcher does
//! this ~1–2 s after an upload, which is the same moment the UI reloads and
//! requests the thumbnails. The response is still HTTP 200, but the body is no
//! longer a complete JPEG: `createImageBitmap` rejects it, and the image worker
//! posts nothing on that path, so the tile stays blank for good.
//!
//! Both halves are driven through HTTP: reader threads poll
//! `/object/compressed/...` while the main thread re-indexes the album that
//! holds the photo through `POST /post/index/album`.

use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use rocket::http::{ContentType, Header, Status};
use rocket::local::blocking::Client;
use snapfab::{PhotoSpec, generate_batch};

use crate::DATA_PATH;
use crate::process::hash::blake3_hasher;
use crate::router::auth::ClaimsHash;
use crate::tests::bootstrap::{
    TEST_ENV, TEST_SERIAL_GUARD, make_client, reset_backend_state, test_image_home,
};
use crate::tests::fixtures::wait_for_album_index;

/// Photo indexed for this test, relative to `IMAGE_HOME`.
const PHOTO_REL: &str = "torn_thumb/album/photo.jpg";
/// Re-index rounds. Each round rewrites the thumbnail exactly once; torn
/// reads cluster inside the rewrite, so a handful of rounds is enough.
const ROUNDS: usize = 6;
/// Reader threads hammering the serving endpoint for the whole test.
const READERS: usize = 2;
/// How long to wait for one re-index job to report `completed`.
const INDEX_TIMEOUT_MS: u64 = 30_000;

/// A complete JPEG starts with the SOI marker and ends with the EOI marker.
/// A body cut short by a concurrent truncate cannot end with EOI, because the
/// encoder writes it last.
fn is_complete_jpeg(body: &[u8]) -> bool {
    body.len() >= 4
        && body[0] == 0xFF
        && body[1] == 0xD8
        && body[body.len() - 2] == 0xFF
        && body[body.len() - 1] == 0xD9
}

fn auth_header(token: &str) -> Header<'static> {
    Header::new("Authorization", format!("Bearer {token}"))
}

/// The `IMAGE_HOME`-relative album directory that holds [`PHOTO_REL`].
fn photo_album() -> String {
    Path::new(PHOTO_REL)
        .parent()
        .expect("PHOTO_REL must sit in a directory")
        .to_string_lossy()
        .into_owned()
}

/// Ask the backend to re-index `album` through `POST /post/index/album`, the
/// same handler the production client uses.
fn post_index_album(client: &Client, album: &str) {
    let resp = client
        .post("/post/index/album")
        .header(ContentType::JSON)
        .body(serde_json::json!({ "album": album }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Accepted,
        "POST /post/index/album must accept the request"
    );
}

fn thumbnail_signature(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Block until the job started for this round reports `completed`, then require
/// the round to have produced one complete thumbnail for the readers to race
/// against.
///
/// `GET /get/index/status` settles on `completed` only after every per-file
/// handle of the job has been joined, and a handle holds its file's
/// `ProcessingGuard` until `index_image` returns — so `completed` implies the
/// write finished. A signature-based wait could not say that: it observed the
/// filesystem, where a finished write looks identical to a write still in
/// flight.
///
/// Both the signature and the bytes are still checked, because the assertion's
/// purpose is that the round provably rewrote the file. Completeness has to be
/// judged from the bytes, not the signature — `save_with_format` creates (and
/// truncates) the destination *before* it encodes and flushes in chunks, so
/// mid-write the signature can already differ while the file is empty or
/// partial. A complete JPEG can only exist once the write finished, because the
/// EOI marker is emitted last.
fn assert_round_rewrote_thumbnail(
    path: &Path,
    before: Option<(std::time::SystemTime, u64)>,
    round: usize,
) {
    let now = thumbnail_signature(path);
    assert!(
        now.is_some() && now != before,
        "round {round}: the album index reported completion but the thumbnail signature \
         did not move (before={before:?}, now={now:?})"
    );

    let bytes = std::fs::read(path)
        .unwrap_or_else(|e| panic!("round {round}: read thumbnail {}: {e}", path.display()));
    assert!(
        is_complete_jpeg(&bytes),
        "round {round}: thumbnail is not a complete JPEG — {} bytes \
         (before={before:?}, now={now:?})",
        bytes.len()
    );
}

#[test]
fn compressed_get_never_returns_a_torn_thumbnail_while_it_is_regenerated() {
    let _guard = TEST_SERIAL_GUARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Force the shared test environment before touching backend state: the
    // config bootstrap sets DATA_PATH itself, so ordering matters here the
    // same way it does for scenario tests.
    let _ = &*TEST_ENV;
    reset_backend_state();

    // ── Arrange: a photo whose thumbnail the backend can regenerate ──
    let image_home = test_image_home();
    let photo_abs = image_home.join(PHOTO_REL);
    if let Some(parent) = photo_abs.parent() {
        std::fs::create_dir_all(parent).expect("create photo directory");
    }
    // Just above the thumbnail target: `small_width_height` still scales this
    // down to a 720×720 JPEG, so the write under test keeps its real size
    // while decoding stays cheap in a debug build.
    generate_batch(&[PhotoSpec {
        output: Some(photo_abs.to_string_lossy().into_owned()),
        format: Some("jpeg".into()),
        width: Some(730),
        height: Some(730),
        tags: None,
        exif_date: None,
        minimal: false,
    }])
    .expect("generate photo fixture");

    let client = make_client();

    // First index creates the thumbnail.
    let thumb_path = {
        let data_path = DATA_PATH.get().expect("DATA_PATH set");
        let hash = blake3_hasher(std::fs::File::open(&photo_abs).expect("open photo"))
            .expect("hash photo");
        data_path.join(format!("object/compressed/{}/{}.jpg", &hash[..2], hash))
    };
    let before_first = thumbnail_signature(&thumb_path);
    let album = photo_album();
    post_index_album(&client, &album);
    wait_for_album_index(&client, INDEX_TIMEOUT_MS);
    assert_round_rewrote_thumbnail(&thumb_path, before_first, 0);

    let hash =
        blake3_hasher(std::fs::File::open(&photo_abs).expect("open photo")).expect("hash photo");
    let token = ClaimsHash::new(hash.clone(), hash.clone(), 0, false).encode();
    let url = format!("/object/compressed/{}/{}.jpg", &hash[..2], hash);

    // ── Sanity: the endpoint must serve a complete JPEG before we race it ──
    let resp = client.get(&url).header(auth_header(&token)).dispatch();
    assert_eq!(
        resp.status(),
        Status::Ok,
        "compressed thumbnail must be servable"
    );
    let body = resp.into_bytes().expect("thumbnail body");
    assert!(
        is_complete_jpeg(&body),
        "sanity check: expected a complete JPEG, got {} bytes",
        body.len()
    );

    // ── Act: readers race the writes ──
    let stop = Arc::new(AtomicBool::new(false));
    let failure: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let reads = Arc::new(AtomicUsize::new(0));
    let good_reads = Arc::new(AtomicUsize::new(0));

    let readers: Vec<_> = (0..READERS)
        .map(|_| {
            let stop = Arc::clone(&stop);
            let failure = Arc::clone(&failure);
            let reads = Arc::clone(&reads);
            let good_reads = Arc::clone(&good_reads);
            let url = url.clone();
            let token = token.clone();
            std::thread::spawn(move || {
                let client = make_client();
                while !stop.load(Ordering::Acquire) {
                    let resp = client.get(&url).header(auth_header(&token)).dispatch();
                    reads.fetch_add(1, Ordering::Relaxed);
                    let status = resp.status();
                    let body = resp.into_bytes().unwrap_or_default();
                    let report = if status != Status::Ok {
                        Some(format!("HTTP {status}"))
                    } else if !is_complete_jpeg(&body) {
                        Some(format!(
                            "torn body: {} bytes, head {:02x?}, tail {:02x?}",
                            body.len(),
                            &body[..body.len().min(4)],
                            &body[body.len().saturating_sub(4)..],
                        ))
                    } else {
                        None
                    };
                    if let Some(report) = report {
                        let mut slot = failure.lock().expect("failure lock");
                        slot.get_or_insert(report);
                    } else {
                        good_reads.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })
        })
        .collect();

    for round in 1..=ROUNDS {
        let before = thumbnail_signature(&thumb_path);
        post_index_album(&client, &album);
        wait_for_album_index(&client, INDEX_TIMEOUT_MS);
        assert_round_rewrote_thumbnail(&thumb_path, before, round);
    }

    stop.store(true, Ordering::Release);
    for reader in readers {
        reader.join().expect("reader thread");
    }

    // ── Assert ──
    let reads = reads.load(Ordering::Relaxed);
    let good_reads = good_reads.load(Ordering::Relaxed);
    let failure = failure.lock().expect("failure lock").clone();
    assert!(
        good_reads > 0,
        "test is inconclusive: {reads} reads but none returned a complete JPEG ({failure:?})"
    );
    assert_eq!(
        failure, None,
        "concurrent reads must never observe a partial thumbnail; \
         {good_reads}/{reads} reads were complete"
    );
}
