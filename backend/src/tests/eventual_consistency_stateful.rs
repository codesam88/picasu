#![cfg(test)]

//! Stateful integration tests for index-pipeline convergence.
//!
//! Each test drives an operation sequence through the real backend (HTTP +
//! filesystem + redb) and asserts the converged state against a tiny
//! in-test model: present/absent per path plus row-exists.
//!
//! Contract mapping (model predicates: `specs/eventual-consistency-assumptions.qnt`;
//! implementation obligations: `specs/requirements.json`):
//! - `upload_modify_rescan_converges`: `[NoLostUpload]` inline upload bumps
//!   generation; `[NoPermanentStale]` external content change is reflected
//!   after rescan; `[identity]` path-keyed identity (same asset id across
//!   modify).
//! - `delete_rescan_prunes`: `[Sweep]` existence-based removal of rows for
//!   missing files; `[NoPermanentStale]` pruned after rescan.
//! - `random_op_sequences_converge`: `[NoLostUpload]` uploads converge to a
//!   row, `[NoPermanentStale]` modifications are reflected after the next
//!   rescan in any interleaving, `[Sweep]` deleted files lose their row —
//!   all asserted against the in-test model at each rescan sync point.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use proptest::prelude::*;
use rocket::http::{ContentType, Header, Status};
use rocket::local::blocking::Client;
use serde_json::Value;
use snapfab::{PhotoSpec, generate_batch};

use crate::storage::asset_store;
use crate::tests::bootstrap::{lock, setup_image_home, test_image_home};
use crate::tests::fixtures::auth_cookie;

fn gen_photo(path: &Path, width: u32, height: u32) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create photo parent");
    }
    generate_batch(&[PhotoSpec {
        output: Some(path.to_string_lossy().into_owned()),
        format: Some("jpeg".into()),
        width: Some(width),
        height: Some(height),
        tags: None,
        exif_date: None,
        minimal: false,
    }])
    .expect("generate photo");
}

fn run_index(client: &Client) {
    let resp = client
        .post("/post/index/album")
        .cookie(auth_cookie(client))
        .header(ContentType::JSON)
        .body(r#"{"album":"/"}"#)
        .dispatch();
    assert_eq!(resp.status(), Status::Accepted, "album index accepted");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let resp = client
            .get("/get/index/status")
            .cookie(auth_cookie(client))
            .dispatch();
        let body: Value =
            serde_json::from_slice(&resp.into_bytes().expect("status body")).expect("status json");
        match body["state"].as_str().unwrap_or("unknown") {
            "completed" => return,
            "failed" => panic!("album index failed: {body}"),
            "canceled" => panic!("album index canceled"),
            _ => {}
        }
        assert!(
            Instant::now() <= deadline,
            "album index did not complete within 30s"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn row_exists(path: &Path) -> bool {
    asset_store::get_asset_id_by_path(&path.to_string_lossy()).is_ok_and(|opt| opt.is_some())
}

/// Wait up to `timeout` for the row of `path` to appear.
///
/// The upload response does not order after the serial-writer flush that
/// commits the uploaded file's row: the handler's batcher hops wait for
/// unrelated batch tasks, while the row write sits in the writer inbox.
/// The protocol contracts require the row to appear after an inline
/// upload, not to appear synchronously at 200, so the test waits for it.
fn wait_for_row(path: &Path, timeout: Duration) -> Option<Duration> {
    let start = Instant::now();
    let deadline = start + timeout;
    loop {
        if row_exists(path) {
            return Some(start.elapsed());
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn content_hash(path: &Path) -> Option<String> {
    let id = asset_store::get_asset_id_by_path(&path.to_string_lossy())
        .ok()
        .flatten()?;
    asset_store::get_asset_by_id(id.as_str())
        .ok()
        .flatten()
        .and_then(|r| r.content_hash.map(|h| h.to_string()))
}

fn upload_photo(client: &Client, src_rel: &str, dest_name: &str) -> PathBuf {
    let image_home = test_image_home();
    let src_path = image_home.join(src_rel.trim_start_matches('/'));
    gen_photo(&src_path, 4, 4);
    let file_data = fs::read(&src_path).expect("read source photo");

    // Snapshot the uploads dir so the file this call added is identifiable
    // even when a sequence uploads more than once.
    let uploads_dir = image_home.join("uploads");
    let before = uploads_dir_files(&uploads_dir);

    let now_ms = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(0);

    let boundary = "----picasu-index-stateful-boundary";
    let mut body = Vec::new();
    // file part
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"file\"; filename=\"");
    body.extend_from_slice(dest_name.as_bytes());
    body.extend_from_slice(b"\"\r\n");
    body.extend_from_slice(b"Content-Type: image/jpeg\r\n\r\n");
    body.extend_from_slice(&file_data);
    body.extend_from_slice(b"\r\n");
    // lastModified part
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"lastModified\"\r\n\r\n");
    body.extend_from_slice(now_ms.to_string().as_bytes());
    body.extend_from_slice(b"\r\n");
    // close
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--\r\n");

    let resp = client
        .post("/post/upload")
        .cookie(auth_cookie(client))
        .header(Header::new(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        ))
        .body(body)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "upload must succeed");

    // The stored extension comes from the upload's Content-Type, not the
    // client filename; discover the file this call added.
    let mut added: Vec<PathBuf> = uploads_dir_files(&uploads_dir)
        .difference(&before)
        .cloned()
        .collect();
    assert_eq!(added.len(), 1, "exactly one new file per upload");
    added.pop().expect("one new file per upload")
}

/// Regular files currently in the uploads dir (empty if it does not exist
/// yet — the upload handler creates it on first use).
fn uploads_dir_files(dir: &Path) -> HashSet<PathBuf> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_file())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn upload_modify_rescan_converges() {
    let _guard = lock();
    let client = setup_image_home();

    // Model: one path that must be present with a row after each step.
    let dest_name = "seq.jpg";
    let uploaded_path = upload_photo(&client, ".scratch/seed.jpg", dest_name);
    assert!(uploaded_path.exists(), "uploaded file must exist");
    eprintln!("UPLOADED_PATH={}", uploaded_path.display());
    for r in asset_store::get_all_assets().unwrap_or_default() {
        eprintln!("ASSET path={}", r.path);
    }
    let upload_latency = wait_for_row(&uploaded_path, Duration::from_secs(10))
        .unwrap_or_else(|| panic!("row must appear after upload: {}", uploaded_path.display()));
    eprintln!("UPLOAD_ROW_VISIBLE_AFTER={upload_latency:?}");
    let id_before = asset_store::get_asset_id_by_path(&uploaded_path.to_string_lossy())
        .ok()
        .flatten()
        .expect("asset id after upload");
    let hash_before = content_hash(&uploaded_path).expect("content hash after upload");

    // Modify: rewrite the same path with different bytes, then rescan.
    gen_photo(&uploaded_path, 8, 8);
    assert!(uploaded_path.exists(), "modified file must exist");
    run_index(&client);

    assert!(
        row_exists(&uploaded_path),
        "row must exist after modify+rescan"
    );
    let hash_after = content_hash(&uploaded_path).expect("content hash after modify");
    assert_ne!(
        hash_before, hash_after,
        "rescan must reflect modified content (hash changed)"
    );

    // [identity] path-keyed identity: the modify re-flushes the same path, so
    // the asset id must survive the content change.
    let id_after = asset_store::get_asset_id_by_path(&uploaded_path.to_string_lossy())
        .ok()
        .flatten()
        .expect("asset id after modify+rescan");
    assert_eq!(
        id_before, id_after,
        "modify+rescan must keep the same asset id for the path"
    );
}

#[test]
fn delete_rescan_prunes() {
    let _guard = lock();
    let client = setup_image_home();

    // Place + index a photo.
    let photo_path = test_image_home().join("prune/photo.jpg");
    gen_photo(&photo_path, 4, 4);
    run_index(&client);
    assert!(photo_path.exists(), "photo must exist before delete");
    assert!(row_exists(&photo_path), "row must exist before delete");

    // Delete the file, then rescan.
    fs::remove_file(&photo_path).expect("remove photo");
    assert!(!photo_path.exists(), "photo must be absent after delete");
    run_index(&client);

    // Model: path absent, row absent.
    assert!(!photo_path.exists(), "photo must stay absent");
    assert!(
        !row_exists(&photo_path),
        "row must be pruned after delete+rescan"
    );
}

// ── Randomized operation sequences ───────────────────────────────────────────

/// Expected converged state of one path, checked by [`assert_converged`]
/// against the filesystem and redb.
#[derive(Clone, Debug)]
struct ModelPath {
    /// The file exists on disk.
    present: bool,
    /// `ASSET_BY_PATH` holds a row for the path.
    row_exists: bool,
    /// Expected stored content hash (blake3 of the file bytes); `None`
    /// exactly when no row exists.
    content_hash: Option<String>,
    /// No sidecar (`.xmp`) exists beside the file. Sidecar operations are
    /// out of scope until sidecar identity lands (REQ-1), so every op keeps
    /// this `true` and the disk check below proves the pipeline never
    /// fabricates one.
    sidecar_absent: bool,
}

/// Operations the generator emits.
#[derive(Clone, Copy, Debug)]
enum Op {
    Upload,
    Modify,
    Delete,
    Rescan,
}

/// Decode one generated byte: the low two bits select the operation
/// (uniform over the four), the upper six bits select the target path among
/// the ones eligible for that operation.
fn decode_op(byte: u8) -> (Op, u8) {
    let op = match byte & 0b11 {
        0 => Op::Upload,
        1 => Op::Modify,
        2 => Op::Delete,
        _ => Op::Rescan,
    };
    (op, byte >> 2)
}

/// Operation sequences of at most six ops; each byte carries an operation
/// plus its path selector (see [`decode_op`]).
fn op_sequence_strategy() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 0..=6)
}

/// blake3 hex of the file bytes — the value the pipeline stores in
/// `AssetRecord::content_hash` (hash stage in `workflow`, committed by
/// `flush_tables`).
fn file_hash(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(blake3::hash(&bytes).to_hex().to_string())
}

/// Pick an eligible (present) path for a `Modify`/`Delete` op; `None` when
/// the model has no present path, in which case the op is skipped.
fn pick_present(model: &BTreeMap<PathBuf, ModelPath>, selector: u8) -> Option<PathBuf> {
    let eligible: Vec<&PathBuf> = model
        .iter()
        .filter(|(_, entry)| entry.present)
        .map(|(path, _)| path)
        .collect();
    if eligible.is_empty() {
        return None;
    }
    let index = usize::from(selector) % eligible.len();
    eligible.get(index).map(|path| (*path).clone())
}

/// Assert that the filesystem and redb match the model. Called at every
/// rescan (the sync point) and once after the final rescan of a sequence.
fn assert_converged(model: &BTreeMap<PathBuf, ModelPath>, at: &str) {
    for (path, want) in model {
        assert_eq!(
            path.exists(),
            want.present,
            "[{at}] file presence diverged for {}",
            path.display()
        );
        assert_eq!(
            row_exists(path),
            want.row_exists,
            "[{at}] row existence diverged for {}",
            path.display()
        );
        assert_eq!(
            content_hash(path),
            want.content_hash,
            "[{at}] content hash diverged for {}",
            path.display()
        );
        assert!(
            want.sidecar_absent,
            "[{at}] model never expects a sidecar for {}",
            path.display()
        );
        assert!(
            !path.with_extension("xmp").exists(),
            "[{at}] no sidecar may appear for {}",
            path.display()
        );
    }

    // Completeness: every media row in redb belongs to a modelled path
    // expected to hold one, so a stale row for a deleted path or a phantom
    // row cannot hide behind the per-path checks. Album records are
    // directories and never end in a media extension; `.scratch` holds
    // `upload_photo`'s source fixtures, which sit inside the walked image
    // home but are not part of the model.
    for record in asset_store::get_all_assets().unwrap_or_default() {
        let is_media = Path::new(&record.path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("jpg"));
        if !is_media || record.path.contains("/.scratch/") {
            continue;
        }
        let path = PathBuf::from(&record.path);
        let want = model
            .get(&path)
            .unwrap_or_else(|| panic!("[{at}] untracked media row for {}", record.path));
        assert!(
            want.row_exists,
            "[{at}] row present for {} but the model expects none",
            record.path
        );
    }
}

/// Execute one generated sequence: apply each op to the filesystem and the
/// model, asserting convergence at every rescan sync point, then rescan
/// once more so the final state is quiescent before the last assertion
/// (a sequence may legitimately end on an unsubmitted change — the
/// transient window the spec's simulator note documents).
fn drive_op_sequence(client: &Client, ops: &[u8]) {
    let mut model: BTreeMap<PathBuf, ModelPath> = BTreeMap::new();
    // Last written dimension per path, alternated 4/8 so every modify
    // rewrites different bytes.
    let mut last_size: BTreeMap<PathBuf, u32> = BTreeMap::new();
    let mut uploads = 0u32;

    for (i, &byte) in ops.iter().enumerate() {
        let (op, selector) = decode_op(byte);
        match op {
            Op::Upload => {
                let stem = format!("propseq_{uploads}");
                uploads += 1;
                let path = upload_photo(
                    client,
                    &format!(".scratch/{stem}.jpg"),
                    &format!("{stem}.jpg"),
                );
                let hash = file_hash(&path).expect("hash uploaded photo");
                model.insert(
                    path.clone(),
                    ModelPath {
                        present: true,
                        row_exists: true,
                        content_hash: Some(hash),
                        sidecar_absent: true,
                    },
                );
                last_size.insert(path, 4); // `upload_photo` generates 4x4
            }
            Op::Modify => {
                let Some(path) = pick_present(&model, selector) else {
                    continue;
                };
                let prev = last_size.get(&path).copied().unwrap_or(4);
                let size = if prev == 4 { 8 } else { 4 };
                gen_photo(&path, size, size);
                last_size.insert(path.clone(), size);
                let entry = model.get_mut(&path).expect("model entry for modified path");
                entry.content_hash = Some(file_hash(&path).expect("hash modified photo"));
            }
            Op::Delete => {
                let Some(path) = pick_present(&model, selector) else {
                    continue;
                };
                fs::remove_file(&path).expect("remove photo");
                let entry = model.get_mut(&path).expect("model entry for deleted path");
                entry.present = false;
                entry.row_exists = false;
                entry.content_hash = None;
            }
            Op::Rescan => {
                run_index(client);
                assert_converged(&model, &format!("op {i}"));
            }
        }
    }

    run_index(client);
    assert_converged(&model, "final");
}

/// Case cap: one case (backend reset + up to six ops, each rescan walking
/// the image home and polling the index status) measures at ≈0.37 s, so 30
/// cases keep the whole test around 10 s — well inside the wall-time budget.
const RANDOM_OP_CASES: u32 = 30;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(RANDOM_OP_CASES))]

    /// Randomized replacement for the fixed scripts above: short operation
    /// histories over upload/modify/delete/rescan must converge to the
    /// in-test model at every rescan sync point and after a final
    /// converging rescan. `Modify`/`Delete` target a present path chosen by
    /// the op byte's selector bits and are skipped when none is eligible.
    #[test]
    fn random_op_sequences_converge(ops in op_sequence_strategy()) {
        let _guard = lock();
        let client = setup_image_home();
        drive_op_sequence(&client, &ops);
    }
}
