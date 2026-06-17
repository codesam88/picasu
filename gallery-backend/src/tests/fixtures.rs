use std::sync::{LazyLock, Mutex, RwLock};

use image::{Rgb, RgbImage};
use rocket::http::Cookie;
use rocket::local::blocking::Client;
use tempfile::TempDir;

use crate::operations::dir_album::get_or_create_dir_album;
use crate::public::structure::config::AppConfig;
use crate::public::structure::response::database_timestamp::DatabaseTimestamp;
use crate::router::builder::build_rocket_with_config;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::flush_tree::FlushTreeTask;

pub use std::collections::HashMap;
pub use std::path::{Path, PathBuf};

pub use arrayvec::ArrayString;
pub use redb::{ReadableDatabase, ReadableTable};
pub use rocket::http::{ContentType, Status};
pub use serde_json::Value;

pub use crate::operations::hash::{blake3_hasher, generate_random_hash};
pub use crate::public::constant::redb::DATA_TABLE;
pub use crate::public::constant::storage::DATA_PATH;
pub use crate::public::db::tree::TREE;
pub use crate::public::structure::abstract_data::AbstractData;
pub use crate::public::structure::common::file_modify::FileModify;
pub use crate::public::structure::config::APP_CONFIG;
pub use crate::public::structure::image::combined::ImageCombined;
pub use crate::public::structure::image::metadata::ImageMetadata;
pub use crate::public::structure::object::{ObjectSchema, ObjectType};
pub use crate::tasks::actor::album::album_task;
pub use crate::tasks::actor::folder_import::{
    FolderImportState, folder_import_status, start_image_home_scan,
};
pub use crate::workflow::index_for_watch;

// ─── One-time test environment ────────────────────────────────────────────

/// Holds the tempdir alive for the entire test binary run.
///
/// Scenario A's "empty state" assertions live here so they execute exactly
/// once before any test body has a chance to insert data.
pub struct TestEnv {
    pub _dir: TempDir,
    /// Assertions captured during init (None = passed, Some(msg) = failed)
    pub init_assertions: Vec<String>,
}

pub static TEST_ENV: LazyLock<TestEnv> = LazyLock::new(|| {
    let dir = tempfile::tempdir().expect("create tempdir");

    DATA_PATH
        .set(dir.path().to_path_buf())
        .expect("DATA_PATH already set");

    // No password → GuardAuth auto-succeeds; read_only_mode = false.
    APP_CONFIG
        .set(RwLock::new(AppConfig::default()))
        .expect("APP_CONFIG already set");

    // Create DATA_TABLE so read-only handlers don't fail on an empty DB.
    {
        let txn = TREE.in_disk.begin_write().expect("begin write txn");
        txn.open_table(DATA_TABLE).expect("create DATA_TABLE");
        txn.commit().expect("commit");
    }

    // ── Scenario A: verify initial empty state ────────────────────────────
    // Run here so no concurrent test can pollute the DB before we check.
    let client = Client::tracked(build_rocket_with_config(AppConfig::default())).expect("rocket");
    let mut failures = Vec::new();

    // Config endpoint: hasPassword=false, readOnlyMode=false, hasAuthKey=false
    let config_resp = client.get("/get/config").dispatch();
    if config_resp.status() != Status::Ok {
        failures.push(format!(
            "GET /get/config: expected 200, got {:?}",
            config_resp.status()
        ));
    } else {
        let body: Value = serde_json::from_str(&config_resp.into_string().unwrap()).unwrap();
        for (k, want) in [
            ("hasPassword", false),
            ("readOnlyMode", false),
            ("hasAuthKey", false),
        ] {
            if body[k] != want {
                failures.push(format!("config.{k} expected {want}, got {}", body[k]));
            }
        }
    }

    // Albums list is empty on a fresh DB
    let albums_resp = client.get("/get/get-albums").dispatch();
    if albums_resp.status() != Status::Ok {
        failures.push(format!(
            "GET /get/get-albums: expected 200, got {:?}",
            albums_resp.status()
        ));
    } else {
        let body: Value = serde_json::from_str(&albums_resp.into_string().unwrap()).unwrap();
        let arr = body.as_array().expect("expected JSON array");
        if !arr.is_empty() {
            failures.push(format!(
                "expected empty album list, got {} items",
                arr.len()
            ));
        }
    }

    // Tags list is empty on a fresh DB
    let tags_resp = client.get("/get/get-tags").dispatch();
    if tags_resp.status() != Status::Ok {
        failures.push(format!(
            "GET /get/get-tags: expected 200, got {:?}",
            tags_resp.status()
        ));
    } else {
        let body: Value = serde_json::from_str(&tags_resp.into_string().unwrap()).unwrap();
        let arr = body.as_array().expect("expected JSON array");
        if !arr.is_empty() {
            failures.push(format!("expected empty tag list, got {} items", arr.len()));
        }
    }

    TestEnv {
        _dir: dir,
        init_assertions: failures,
    }
});

// ─── Helpers ─────────────────────────────────────────────────────────────

pub fn make_client() -> Client {
    let _ = &*TEST_ENV;
    Client::tracked(build_rocket_with_config(AppConfig::default())).expect("valid rocket instance")
}

pub fn auth_cookie(client: &Client) -> Cookie<'static> {
    let r = client
        .post("/post/authenticate")
        .header(ContentType::JSON)
        .body(r#""""#)
        .dispatch();
    let token = r.into_string().expect("token body");
    Cookie::new("jwt", token.trim_matches('"').to_owned())
}

pub fn json_get(client: &Client, path: &str) -> Value {
    let cookie = auth_cookie(client);
    let r = client.get(path).cookie(cookie).dispatch();
    assert_eq!(r.status(), Status::Ok, "GET {path} failed");
    serde_json::from_str(&r.into_string().unwrap()).expect("valid JSON")
}

/// `TREE_SNAPSHOT` keys snapshots by `Utc::now().timestamp_millis()`
/// (get_prefetch.rs). Two `prefetch` calls from different tests landing
/// in the same millisecond will silently overwrite each other's
/// snapshot, corrupting whichever test reads it second — a real
/// concurrency bug, not just a test artifact (see TODO.md). Tests that
/// call `prefetch_locate` must hold this guard for their whole body to
/// avoid tripping over it while running in parallel with each other.
pub static PREFETCH_SERIAL_GUARD: Mutex<()> = Mutex::new(());

/// Run the same prefetch flow the frontend uses before fetching row data:
/// POST /get/prefetch?locate=<hash>, returning (timestamp, index, bearer
/// token) for the matching item so the caller can hit /get/get-data or
/// /put/edit_tag exactly like a real client would.
pub fn prefetch_locate(client: &Client, hash: &str) -> (i64, usize, String) {
    let cookie = auth_cookie(client);
    let resp = client
        .post(format!("/get/prefetch?locate={hash}"))
        .cookie(cookie)
        .header(ContentType::JSON)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "prefetch must succeed");
    let body: Value = serde_json::from_str(&resp.into_string().unwrap()).expect("valid JSON");
    let timestamp = body["prefetch"]["timestamp"]
        .as_i64()
        .expect("prefetch.timestamp");
    let index = usize::try_from(
        body["prefetch"]["locateTo"]
            .as_u64()
            .expect("prefetch.locateTo must be present for a known hash"),
    )
    .expect("index fits in usize");
    let token = body["token"].as_str().expect("token").to_owned();
    (timestamp, index, token)
}

/// `prefetch_locate` + `get_data_item`, with a fresh re-prefetch on
/// failure rather than blindly re-requesting the same (now possibly
/// invalid) timestamp.
///
/// A `prefetch` snapshot is meant to live for 1 hour
/// (`update_expire_task`), but `Expire::expired_check`
/// (`public/db/expire/expired_check.rs`) treats "no expiry entry
/// recorded yet" (`None`) the same as "already expired" — and a
/// brand-new snapshot's *own* `VERSION_COUNT_TIMESTAMP` slot is
/// recorded as exactly that `None` until the *next* version bump. So
/// as soon as anything else in the process bumps the global,
/// process-wide `VERSION_COUNT_TIMESTAMP` (any indexing or edit
/// anywhere — including other tests running concurrently),
/// `expire_check_task` immediately deletes the just-created query/tree
/// snapshot. This is a real bug (see TODO.md), not a test artifact —
/// but it's not what these particular tests are meant to catch, so we
/// route around it here by re-prefetching instead of trying to outwait
/// it.
pub fn read_current_abstract_data(client: &Client, hash: &str) -> Value {
    for attempt in 0..5 {
        let (timestamp, index, token) = prefetch_locate(client, hash);
        let resp = client
            .get(format!(
                "/get/get-data?timestamp={timestamp}&start={index}&end={}",
                index + 1
            ))
            .header(rocket::http::Header::new(
                "Authorization",
                format!("Bearer {token}"),
            ))
            .dispatch();
        if resp.status() == Status::Ok {
            let body: Value =
                serde_json::from_str(&resp.into_string().unwrap()).expect("valid JSON");
            return body.as_array().expect("array")[0]["abstractData"].clone();
        }
        if attempt == 4 {
            assert_eq!(resp.status(), Status::Ok, "get-data must succeed");
        }
    }
    unreachable!()
}

/// Minimal photo fixture: fake file path, tags, optional EXIF date string.
pub struct PhotoSpec<'a> {
    pub path: &'a str,
    pub tags: &'a [&'a str],
    pub exif_date: Option<&'a str>,
}

/// Write Image records to redb with the given fake paths and refresh
/// TREE.in_memory.  No actual files are needed.
pub fn insert_photos(photos: &[PhotoSpec]) {
    let _ = &*TEST_ENV; // ensure DATA_PATH and APP_CONFIG are initialised
    let txn = TREE.in_disk.begin_write().expect("begin write");
    {
        let mut table = txn.open_table(DATA_TABLE).expect("open table");
        for spec in photos {
            let hash = generate_random_hash();
            let mut obj = ObjectSchema::new(hash, ObjectType::Image);
            for &tag in spec.tags {
                obj.tags.insert(tag.to_owned());
            }
            let mut meta = ImageMetadata::new(hash, 4096, 1920, 1080, "jpg".into());
            meta.alias = vec![FileModify {
                file: spec.path.to_owned(),
                modified: 0,
                scan_time: 0,
            }];
            if let Some(date) = spec.exif_date {
                meta.exif_vec.insert("DateTimeOriginal".into(), date.into());
            }
            table
                .insert(
                    hash.as_str(),
                    &AbstractData::Image(ImageCombined {
                        object: obj,
                        metadata: meta,
                    }),
                )
                .expect("insert photo");
        }
    }
    txn.commit().expect("commit photos");
    refresh_in_memory();
}

/// Re-read all redb records into TREE.in_memory.
/// Replicates the synchronous core of `update_tree_task` without async.
pub fn refresh_in_memory() {
    let txn = TREE.in_disk.begin_read().expect("begin read");
    let table = txn.open_table(DATA_TABLE).expect("open table");
    let priority_list = &["DateTimeOriginal", "filename", "modified", "scan_time"];
    let mut vec: Vec<DatabaseTimestamp> = table
        .iter()
        .expect("iter")
        .map(|entry| {
            let (_, v) = entry.expect("entry");
            DatabaseTimestamp::new(v.value(), priority_list)
        })
        .collect();
    vec.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
    *TREE.in_memory.write().unwrap() = vec;
}

/// Create a dir album for `dir_path` and return its album ID.
pub fn make_dir_album(dir_path: &Path) -> ArrayString<64> {
    get_or_create_dir_album(dir_path.to_path_buf()).expect("create dir album")
}

/// Insert a photo backed by an actual file on disk and return its hash.
pub fn insert_photo_with_real_file(file_path: &Path) -> ArrayString<64> {
    let _ = &*TEST_ENV; // ensure DATA_PATH and APP_CONFIG are initialised
    assert!(file_path.exists(), "source file must exist: {file_path:?}");
    let hash = generate_random_hash();
    let txn = TREE.in_disk.begin_write().expect("begin write");
    {
        let mut table = txn.open_table(DATA_TABLE).expect("open table");
        let obj = ObjectSchema::new(hash, ObjectType::Image);
        let mut meta = ImageMetadata::new(hash, 1, 1, 1, "jpg".into());
        meta.alias = vec![FileModify {
            file: file_path.to_string_lossy().into_owned(),
            modified: 0,
            scan_time: 0,
        }];
        table
            .insert(
                hash.as_str(),
                &AbstractData::Image(ImageCombined {
                    object: obj,
                    metadata: meta,
                }),
            )
            .expect("insert");
    }
    txn.commit().expect("commit");
    refresh_in_memory();
    hash
}

/// Like `insert_photo_with_real_file`, but with an explicit hash and
/// deliberately stale/placeholder metadata (`width`/`height` 1x1, no
/// tags) — simulating a pre-existing, incomplete index entry (e.g. one
/// written before a metadata-extraction feature existed, or by an
/// older/buggy version). The hash must match `file_path`'s real content
/// hash (computed with the same hasher `HashTask` uses) for a
/// force-reindex to recognise it as "already known" rather than new.
pub fn insert_stale_photo_record(file_path: &Path, hash: ArrayString<64>) {
    assert!(file_path.exists(), "source file must exist: {file_path:?}");
    let txn = TREE.in_disk.begin_write().expect("begin write");
    {
        let mut table = txn.open_table(DATA_TABLE).expect("open table");
        let obj = ObjectSchema::new(hash, ObjectType::Image);
        let mut meta = ImageMetadata::new(hash, 1, 1, 1, "jpg".into());
        meta.alias = vec![FileModify {
            file: file_path.to_string_lossy().into_owned(),
            modified: 0,
            scan_time: 0,
        }];
        table
            .insert(
                hash.as_str(),
                &AbstractData::Image(ImageCombined {
                    object: obj,
                    metadata: meta,
                }),
            )
            .expect("insert");
    }
    txn.commit().expect("commit");
    refresh_in_memory();
}

/// Block until every `FlushTreeTask` queued before this call has been
/// written to disk. `index_task`/`video_task` enqueue their writes via
/// `execute_batch_detached`, which returns immediately; submitting an
/// empty flush with `execute_batch_waiting` guarantees (per
/// `mini_executor`'s ordering contract) that everything submitted
/// earlier on the same queue has already run by the time this resolves.
pub async fn wait_for_flush() {
    BATCH_COORDINATOR
        .execute_batch_waiting(FlushTreeTask::insert(Vec::new()))
        .await
        .expect("flush sync");
}

/// Write a tiny real (decodable) JPEG to `path` — needed for tests that
/// exercise the real indexing pipeline (`process_image_info` decodes
/// pixels), unlike the `b"\xff\xd8\xff fake jpeg"` fixtures used by
/// tests that only exercise `assign_album` (no decode involved).
pub fn write_real_jpeg(path: &Path, color: [u8; 3]) {
    let img = RgbImage::from_pixel(4, 4, Rgb(color));
    img.save(path).expect("encode real jpeg");
}

/// Write a real decodable JPEG with an embedded XMP packet declaring
/// `dc:subject` keywords, as Lightroom/digiKam/exiftool would write
/// them. The packet is spliced into an APP1 segment right after the
/// SOI marker; JPEG decoders skip unrecognised APPn segments, so the
/// image still decodes normally.
pub fn write_real_jpeg_with_xmp_keywords(path: &Path, color: [u8; 3], keywords: &[&str]) {
    let img = RgbImage::from_pixel(4, 4, Rgb(color));
    let mut jpeg_bytes: Vec<u8> = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut jpeg_bytes),
        image::ImageFormat::Jpeg,
    )
    .expect("encode jpeg to memory");

    let items: String = keywords
        .iter()
        .map(|k| format!("<rdf:li>{k}</rdf:li>"))
        .collect();
    let xmp_packet = format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:subject><rdf:Bag>{items}</rdf:Bag></dc:subject></rdf:Description></rdf:RDF></x:xmpmeta>"#
    );

    let mut segment: Vec<u8> = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    segment.extend_from_slice(xmp_packet.as_bytes());
    let segment_len = u16::try_from(segment.len() + 2).expect("xmp segment too large");
    let mut app1: Vec<u8> = vec![0xFF, 0xE1];
    app1.extend_from_slice(&segment_len.to_be_bytes());
    app1.extend_from_slice(&segment);

    // jpeg_bytes[0..2] is the SOI marker (FF D8); splice APP1 right after it.
    let mut spliced = jpeg_bytes[..2].to_vec();
    spliced.extend_from_slice(&app1);
    spliced.extend_from_slice(&jpeg_bytes[2..]);

    std::fs::write(path, &spliced).expect("write spliced jpeg");
}

/// Scan `DATA_TABLE` for the record whose first alias entry is `path`.
/// Used after running the real indexing pipeline, where the hash is
/// content-derived (blake3) and not known to the test ahead of time.
pub fn find_hash_by_alias_path(path: &Path) -> ArrayString<64> {
    let target = path.to_string_lossy().into_owned();
    let txn = TREE.in_disk.begin_read().expect("begin read");
    let table = txn.open_table(DATA_TABLE).expect("open table");
    table
        .iter()
        .expect("iter")
        .flatten()
        .find_map(|(_, v)| {
            let data = v.value();
            data.alias()
                .iter()
                .any(|a| a.file == target)
                .then(|| data.hash())
        })
        .unwrap_or_else(|| panic!("no indexed record found with alias path {target}"))
}

/// Fetch the original-file bytes via the real client flow: prefetch +
/// get-data (to obtain the hash-scoped, `allow_original` token embedded
/// in the response) + `GET /object/imported/<hash-prefix>/<hash>.<ext>`.
pub fn fetch_original_bytes(client: &Client, hash: &str, ext: &str) -> Vec<u8> {
    let (timestamp, index, ts_token) = prefetch_locate(client, hash);
    let resp = client
        .get(format!(
            "/get/get-data?timestamp={timestamp}&start={index}&end={}",
            index + 1
        ))
        .header(rocket::http::Header::new(
            "Authorization",
            format!("Bearer {ts_token}"),
        ))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "get-data must succeed");
    let body: Value = serde_json::from_str(&resp.into_string().unwrap()).expect("valid JSON");
    let hash_token = body.as_array().expect("array")[0]["token"]
        .as_str()
        .expect("token")
        .to_owned();

    let resp = client
        .get(format!("/object/imported/{}/{hash}.{ext}", &hash[0..2]))
        .header(rocket::http::Header::new(
            "Authorization",
            format!("Bearer {hash_token}"),
        ))
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Ok,
        "GET /object/imported must succeed"
    );
    resp.into_bytes().expect("response body bytes")
}

/// Build a `multipart/form-data` body for `POST /upload`: one `file`
/// part (binary, with the given filename/content-type) and one
/// `lastModified` text part, matching what the frontend sends.
pub fn build_upload_multipart_body(
    filename: &str,
    file_bytes: &[u8],
    content_type: &str,
    last_modified_ms: u64,
) -> (String, Vec<u8>) {
    let boundary = "e2eUploadBoundary".to_string();
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(file_bytes);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"lastModified\"\r\n\r\n{last_modified_ms}\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (boundary, body)
}
