/// Scenario-based end-to-end API tests.
///
/// Self-contained scenarios that inject their own fixtures with unique
/// namespaced paths/tags and assert only on what they created.
///
/// Scenario A checks the initial empty state and must run before any data is
/// inserted; it does so atomically inside the TEST_ENV LazyLock initialiser
/// so it is guaranteed to run exactly once, before any other test body.
#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{LazyLock, Mutex, RwLock};

    use arrayvec::ArrayString;
    use image::{Rgb, RgbImage};
    use redb::{ReadableDatabase, ReadableTable};
    use rocket::http::{ContentType, Cookie, Status};
    use rocket::local::blocking::Client;
    use serde_json::Value;
    use tempfile::TempDir;

    use crate::operations::dir_album::get_or_create_dir_album;
    use crate::operations::hash::{blake3_hasher, generate_random_hash};
    use crate::public::constant::redb::DATA_TABLE;
    use crate::public::constant::storage::DATA_PATH;
    use crate::public::db::tree::TREE;
    use crate::public::structure::abstract_data::AbstractData;
    use crate::public::structure::common::file_modify::FileModify;
    use crate::public::structure::config::{APP_CONFIG, AppConfig};
    use crate::public::structure::image::combined::ImageCombined;
    use crate::public::structure::image::metadata::ImageMetadata;
    use crate::public::structure::object::{ObjectSchema, ObjectType};
    use crate::public::structure::response::database_timestamp::DatabaseTimestamp;
    use crate::router::builder::build_rocket_with_config;
    use crate::tasks::BATCH_COORDINATOR;
    use crate::tasks::actor::album::album_task;
    use crate::tasks::actor::folder_import::{
        FolderImportState, folder_import_status, start_image_home_scan,
    };
    use crate::tasks::batcher::flush_tree::FlushTreeTask;
    use crate::workflow::index_for_watch;

    // ─── One-time test environment ────────────────────────────────────────────

    /// Holds the tempdir alive for the entire test binary run.
    ///
    /// Scenario A's "empty state" assertions live here so they execute exactly
    /// once before any test body has a chance to insert data.
    struct TestEnv {
        _dir: TempDir,
        /// Assertions captured during init (None = passed, Some(msg) = failed)
        init_assertions: Vec<String>,
    }

    static TEST_ENV: LazyLock<TestEnv> = LazyLock::new(|| {
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
        let client =
            Client::tracked(build_rocket_with_config(AppConfig::default())).expect("rocket");
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

    fn make_client() -> Client {
        let _ = &*TEST_ENV;
        Client::tracked(build_rocket_with_config(AppConfig::default()))
            .expect("valid rocket instance")
    }

    fn auth_cookie(client: &Client) -> Cookie<'static> {
        let r = client
            .post("/post/authenticate")
            .header(ContentType::JSON)
            .body(r#""""#)
            .dispatch();
        let token = r.into_string().expect("token body");
        Cookie::new("jwt", token.trim_matches('"').to_owned())
    }

    fn json_get(client: &Client, path: &str) -> Value {
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
    static PREFETCH_SERIAL_GUARD: Mutex<()> = Mutex::new(());

    /// Run the same prefetch flow the frontend uses before fetching row data:
    /// POST /get/prefetch?locate=<hash>, returning (timestamp, index, bearer
    /// token) for the matching item so the caller can hit /get/get-data or
    /// /put/edit_tag exactly like a real client would.
    fn prefetch_locate(client: &Client, hash: &str) -> (i64, usize, String) {
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
    fn read_current_abstract_data(client: &Client, hash: &str) -> Value {
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
    struct PhotoSpec<'a> {
        path: &'a str,
        tags: &'a [&'a str],
        exif_date: Option<&'a str>,
    }

    /// Write Image records to redb with the given fake paths and refresh
    /// TREE.in_memory.  No actual files are needed.
    fn insert_photos(photos: &[PhotoSpec]) {
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
    fn refresh_in_memory() {
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
        vec.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        *TREE.in_memory.write().unwrap() = vec;
    }

    /// Create a dir album for `dir_path` and return its album ID.
    fn make_dir_album(dir_path: &Path) -> ArrayString<64> {
        get_or_create_dir_album(dir_path.to_path_buf()).expect("create dir album")
    }

    // ─── Scenario A: initial empty state ─────────────────────────────────────

    /// Verify the assertions captured during TEST_ENV initialisation.
    /// Those assertions run before any test body, so they see a clean DB.
    #[test]
    fn scenario_a_initial_empty_state() {
        let env = &*TEST_ENV;
        if !env.init_assertions.is_empty() {
            panic!("Scenario A failed:\n{}", env.init_assertions.join("\n"));
        }
    }

    // ─── Scenario B: photo repository with tags ───────────────────────────────

    /// Inject a small photo collection with uniquely prefixed tags and verify
    /// the tag endpoint returns correct names and AT LEAST the expected counts.
    /// Tag names are namespaced to this test to avoid interference with other
    /// scenarios that also insert photos.
    #[test]
    fn scenario_b_photo_tags_reflect_injected_metadata() {
        insert_photos(&[
            PhotoSpec {
                path: "/e2e_b/2023/summer/beach.jpg",
                tags: &["e2e_b_nature", "e2e_b_summer", "e2e_b_beach"],
                exif_date: Some("2023:07:15 10:00:00"),
            },
            PhotoSpec {
                path: "/e2e_b/2023/summer/sunset.jpg",
                tags: &["e2e_b_nature", "e2e_b_summer"],
                exif_date: Some("2023:07:16 20:00:00"),
            },
            PhotoSpec {
                path: "/e2e_b/2024/winter/snow.jpg",
                tags: &["e2e_b_nature", "e2e_b_winter"],
                exif_date: Some("2024:01:10 09:00:00"),
            },
            PhotoSpec {
                path: "/e2e_b/2024/city/skyline.jpg",
                tags: &["e2e_b_architecture", "e2e_b_city"],
                exif_date: None,
            },
        ]);

        let client = make_client();
        let tags: Vec<Value> =
            serde_json::from_value(json_get(&client, "/get/get-tags")).expect("array");

        let counts: HashMap<String, u64> = tags
            .iter()
            .map(|t| {
                (
                    t["tag"].as_str().unwrap().to_owned(),
                    t["number"].as_u64().unwrap(),
                )
            })
            .collect();

        // Use "at least" so this test stays valid even if other photos end up
        // in the DB from other concurrently running scenarios.
        for (tag, min) in [
            ("e2e_b_nature", 3u64),
            ("e2e_b_summer", 2),
            ("e2e_b_beach", 1),
            ("e2e_b_winter", 1),
            ("e2e_b_architecture", 1),
            ("e2e_b_city", 1),
        ] {
            let got = counts.get(tag).copied().unwrap_or(0);
            assert!(got >= min, "tag '{tag}': expected >= {min}, got {got}");
        }
    }

    // ─── Scenario D: directory-based photo hierarchy ──────────────────────────

    /// Verify that the parent→child relationship is correctly exposed via
    /// parentAlbumId and dirPath.
    ///
    /// Uses a unique base path (/e2e_d/) so this test does not conflict with
    /// Scenario E's generated tree.
    #[test]
    fn scenario_d_dir_album_parent_child_relationship() {
        let parent_dir = PathBuf::from("/e2e_d/vacation");
        let child_dir = PathBuf::from("/e2e_d/vacation/day1");

        insert_photos(&[
            PhotoSpec {
                path: "/e2e_d/vacation/img1.jpg",
                tags: &["e2e_d_travel"],
                exif_date: None,
            },
            PhotoSpec {
                path: "/e2e_d/vacation/day1/img2.jpg",
                tags: &["e2e_d_travel", "e2e_d_beach"],
                exif_date: None,
            },
        ]);

        let parent_id = make_dir_album(&parent_dir);
        let child_id = make_dir_album(&child_dir);

        refresh_in_memory();
        album_task(parent_id).expect("parent album_task");
        album_task(child_id).expect("child album_task");
        refresh_in_memory();

        let client = make_client();
        let albums: Vec<Value> =
            serde_json::from_value(json_get(&client, "/get/get-albums")).expect("array");

        let by_dir: HashMap<String, Value> = albums
            .into_iter()
            .filter_map(|a| a["dirPath"].as_str().map(|d| (d.to_owned(), a.clone())))
            .collect();

        let parent = by_dir
            .get(parent_dir.to_str().unwrap())
            .expect("vacation album missing");
        let child = by_dir
            .get(child_dir.to_str().unwrap())
            .expect("day1 album missing");

        assert_eq!(
            parent["parentAlbumId"],
            Value::Null,
            "vacation: parentAlbumId must be null"
        );
        assert_eq!(parent["albumId"].as_str().unwrap(), parent_id.as_str());

        assert_eq!(
            child["parentAlbumId"].as_str().unwrap(),
            parent_id.as_str(),
            "day1.parentAlbumId must equal vacation.albumId"
        );
        assert_eq!(child["albumId"].as_str().unwrap(), child_id.as_str());

        // The parent album counts only its direct photo (img1.jpg), not day1/img2.jpg.
        {
            let txn = TREE.in_disk.begin_read().expect("begin read");
            let table = txn.open_table(DATA_TABLE).expect("open table");
            let guard = table
                .get(parent_id.as_str())
                .expect("redb get")
                .expect("parent album in redb");
            let AbstractData::Album(parent_album) = guard.value() else {
                panic!("not an album")
            };
            assert_eq!(
                parent_album.metadata.item_count, 1,
                "vacation album must count only its direct photo (not day1/img2)"
            );
        }
    }

    // ─── Scenario E: generated multi-level dir tree ───────────────────────────

    /// Build a known three-level directory tree and verify that the hierarchy
    /// properties hold for all nodes:
    ///   - root albums have parentAlbumId == null,
    ///   - each non-root album's parentAlbumId matches its parent's albumId,
    ///   - no album is its own parent.
    #[test]
    fn scenario_e_generated_dir_tree_hierarchy_properties() {
        let dirs: &[(&str, Option<&str>)] = &[
            ("/e2e_e/root", None),
            ("/e2e_e/root/alpha", Some("/e2e_e/root")),
            ("/e2e_e/root/beta", Some("/e2e_e/root")),
            ("/e2e_e/root/alpha/deep", Some("/e2e_e/root/alpha")),
        ];

        let photo_paths: Vec<String> = dirs
            .iter()
            .map(|(dir, _)| format!("{dir}/e2e_e_photo.jpg"))
            .collect();
        let specs: Vec<PhotoSpec> = photo_paths
            .iter()
            .map(|p| PhotoSpec {
                path: p,
                tags: &[],
                exif_date: None,
            })
            .collect();
        insert_photos(&specs);

        let mut id_by_dir: HashMap<&str, ArrayString<64>> = HashMap::new();
        for (dir, _) in dirs {
            id_by_dir.insert(dir, make_dir_album(Path::new(dir)));
        }

        refresh_in_memory();

        let client = make_client();
        let albums: Vec<Value> =
            serde_json::from_value(json_get(&client, "/get/get-albums")).expect("array");

        let by_dir: HashMap<String, Value> = albums
            .into_iter()
            .filter_map(|a| a["dirPath"].as_str().map(|d| (d.to_owned(), a.clone())))
            .collect();

        for (dir, expected_parent) in dirs {
            let album = by_dir
                .get(*dir)
                .unwrap_or_else(|| panic!("album for {dir} not found"));

            match expected_parent {
                None => assert_eq!(
                    album["parentAlbumId"],
                    Value::Null,
                    "{dir}: root must have null parentAlbumId"
                ),
                Some(parent_dir) => {
                    let expected_id = id_by_dir[parent_dir].as_str();
                    let actual = album["parentAlbumId"]
                        .as_str()
                        .unwrap_or_else(|| panic!("{dir}: parentAlbumId was null"));
                    assert_eq!(actual, expected_id, "{dir}: wrong parentAlbumId");
                    assert_ne!(
                        actual,
                        album["albumId"].as_str().unwrap(),
                        "{dir}: album is its own parent"
                    );
                }
            }
        }
    }

    /// Insert a photo backed by an actual file on disk and return its hash.
    fn insert_photo_with_real_file(file_path: &Path) -> ArrayString<64> {
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
    fn insert_stale_photo_record(file_path: &Path, hash: ArrayString<64>) {
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
    async fn wait_for_flush() {
        BATCH_COORDINATOR
            .execute_batch_waiting(FlushTreeTask::insert(Vec::new()))
            .await
            .expect("flush sync");
    }

    /// Write a tiny real (decodable) JPEG to `path` — needed for tests that
    /// exercise the real indexing pipeline (`process_image_info` decodes
    /// pixels), unlike the `b"\xff\xd8\xff fake jpeg"` fixtures used by
    /// tests that only exercise `assign_album` (no decode involved).
    fn write_real_jpeg(path: &Path, color: [u8; 3]) {
        let img = RgbImage::from_pixel(4, 4, Rgb(color));
        img.save(path).expect("encode real jpeg");
    }

    /// Write a real decodable JPEG with an embedded XMP packet declaring
    /// `dc:subject` keywords, as Lightroom/digiKam/exiftool would write
    /// them. The packet is spliced into an APP1 segment right after the
    /// SOI marker; JPEG decoders skip unrecognised APPn segments, so the
    /// image still decodes normally.
    fn write_real_jpeg_with_xmp_keywords(path: &Path, color: [u8; 3], keywords: &[&str]) {
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
    fn find_hash_by_alias_path(path: &Path) -> ArrayString<64> {
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

    // ─── Scenario G: PUT /put/assign_album endpoint registered ───────────────

    /// The new assign_album endpoint must be registered.
    /// Dummy IDs are sent so the route itself can respond with any non-routing
    /// error.  Currently the route is absent → 404; this test fails until it
    /// is registered.
    #[test]
    fn scenario_g_assign_album_endpoint_is_registered() {
        let client = make_client();
        let cookie = auth_cookie(&client);
        let resp = client
            .put("/put/assign_album")
            .cookie(cookie)
            .header(ContentType::JSON)
            .body(r#"{"hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","albumId":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}"#)
            .dispatch();
        assert_ne!(
            resp.status(),
            Status::NotFound,
            "PUT /put/assign_album must be a registered route (currently absent — 404)"
        );
    }

    // ─── Scenario H: assign_album moves file and updates alias + membership ───

    /// Full workflow: file on disk → DB record → assign_album → file moved to
    /// album directory → alias in DB updated → album item_count is 1.
    #[test]
    fn scenario_h_assign_album_moves_file_and_updates_membership() {
        let data = {
            let _ = &*TEST_ENV;
            DATA_PATH.get().expect("DATA_PATH initialised")
        };

        let import_dir = data.join("e2e_h_import");
        std::fs::create_dir_all(&import_dir).expect("create import dir");
        let src = import_dir.join("e2e_h_photo.jpg");
        std::fs::write(&src, b"\xff\xd8\xff fake jpeg").expect("write source file");

        let album_dir = data.join("e2e_h_album");
        std::fs::create_dir_all(&album_dir).expect("create album dir");
        let album_id = make_dir_album(&album_dir);

        let hash = insert_photo_with_real_file(&src);

        let client = make_client();
        let cookie = auth_cookie(&client);
        let resp = client
            .put("/put/assign_album")
            .cookie(cookie)
            .header(ContentType::JSON)
            .body(format!(r#"{{"hash":"{hash}","albumId":"{album_id}"}}"#))
            .dispatch();

        assert_eq!(
            resp.status(),
            Status::Ok,
            "assign_album must return 200 (currently 404 — not yet implemented)"
        );

        // File must be at new location, gone from source
        let dst = album_dir.join("e2e_h_photo.jpg");
        assert!(dst.exists(), "file must be moved into album dir: {dst:?}");
        assert!(!src.exists(), "source path must be vacated: {src:?}");

        // Alias in DB must reflect the new path
        {
            let txn = TREE.in_disk.begin_read().expect("begin read");
            let table = txn.open_table(DATA_TABLE).expect("open table");
            let guard = table
                .get(hash.as_str())
                .expect("redb get")
                .expect("image in redb");
            let AbstractData::Image(img) = guard.value() else {
                panic!("not an image")
            };
            assert_eq!(
                img.metadata.alias[0].file,
                dst.to_string_lossy().as_ref(),
                "alias must point to new path after move"
            );
        }

        // Album item_count must be 1 (read redb directly — race-free)
        refresh_in_memory();
        album_task(album_id).expect("album_task");
        {
            let txn = TREE.in_disk.begin_read().expect("begin read");
            let table = txn.open_table(DATA_TABLE).expect("open table");
            let guard = table
                .get(album_id.as_str())
                .expect("redb get")
                .expect("album in redb");
            let AbstractData::Album(alb) = guard.value() else {
                panic!("not an album")
            };
            assert_eq!(alb.metadata.item_count, 1, "album must count 1 item");
        }
    }

    // ─── Scenario I: album membership is singular ─────────────────────────────

    /// Reassigning an image from album A to album B must leave A with 0 items
    /// and B with 1.  The old HashSet model would leave it in both.
    #[test]
    fn scenario_i_album_membership_is_singular() {
        let data = {
            let _ = &*TEST_ENV;
            DATA_PATH.get().expect("DATA_PATH initialised")
        };

        let import_dir = data.join("e2e_i_import");
        std::fs::create_dir_all(&import_dir).expect("create import dir");
        let src = import_dir.join("e2e_i_photo.jpg");
        std::fs::write(&src, b"\xff\xd8\xff fake").expect("write source file");

        let album_a_dir = data.join("e2e_i_album_a");
        let album_b_dir = data.join("e2e_i_album_b");
        std::fs::create_dir_all(&album_a_dir).expect("create album A dir");
        std::fs::create_dir_all(&album_b_dir).expect("create album B dir");
        let album_a = make_dir_album(&album_a_dir);
        let album_b = make_dir_album(&album_b_dir);

        let hash = insert_photo_with_real_file(&src);
        let client = make_client();

        // Assign to A
        let r = client
            .put("/put/assign_album")
            .cookie(auth_cookie(&client))
            .header(ContentType::JSON)
            .body(format!(r#"{{"hash":"{hash}","albumId":"{album_a}"}}"#))
            .dispatch();
        assert_eq!(r.status(), Status::Ok, "assign → A must return 200");

        // Reassign to B (file is now in A's directory)
        let r = client
            .put("/put/assign_album")
            .cookie(auth_cookie(&client))
            .header(ContentType::JSON)
            .body(format!(r#"{{"hash":"{hash}","albumId":"{album_b}"}}"#))
            .dispatch();
        assert_eq!(r.status(), Status::Ok, "assign → B must return 200");

        // Read item_counts directly from redb (race-free)
        refresh_in_memory();
        album_task(album_a).expect("album_task A");
        album_task(album_b).expect("album_task B");
        {
            let txn = TREE.in_disk.begin_read().expect("begin read");
            let table = txn.open_table(DATA_TABLE).expect("open table");

            let ga = table
                .get(album_a.as_str())
                .expect("get A")
                .expect("A in redb");
            let AbstractData::Album(alb_a) = ga.value() else {
                panic!("A not an album")
            };
            assert_eq!(
                alb_a.metadata.item_count, 0,
                "album A must have 0 items after image reassigned to B"
            );

            let gb = table
                .get(album_b.as_str())
                .expect("get B")
                .expect("B in redb");
            let AbstractData::Album(alb_b) = gb.value() else {
                panic!("B not an album")
            };
            assert_eq!(
                alb_b.metadata.item_count, 1,
                "album B must have 1 item after reassign"
            );
        }
    }

    // ─── Scenario J: assign_album with stale alias path returns error ─────────

    /// If the file is not at the path recorded in the DB, assign_album must
    /// return a 4xx error and leave the DB unchanged.
    #[test]
    fn scenario_j_assign_album_rejects_stale_file_path() {
        let data = {
            let _ = &*TEST_ENV;
            DATA_PATH.get().expect("DATA_PATH initialised")
        };
        let album_dir = data.join("e2e_j_album");
        std::fs::create_dir_all(&album_dir).expect("create album dir");
        let album_id = make_dir_album(&album_dir);

        // Insert an image whose alias points at a non-existent file
        let ghost = data.join("e2e_j_ghost_does_not_exist.jpg");
        assert!(!ghost.exists(), "ghost path must not exist for this test");
        let hash = generate_random_hash();
        {
            let txn = TREE.in_disk.begin_write().expect("begin write");
            {
                let mut table = txn.open_table(DATA_TABLE).expect("open table");
                let obj = ObjectSchema::new(hash, ObjectType::Image);
                let mut meta = ImageMetadata::new(hash, 0, 1, 1, "jpg".into());
                meta.alias = vec![FileModify {
                    file: ghost.to_string_lossy().into_owned(),
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
        }
        refresh_in_memory();

        let client = make_client();
        let cookie = auth_cookie(&client);
        let resp = client
            .put("/put/assign_album")
            .cookie(cookie)
            .header(ContentType::JSON)
            .body(format!(r#"{{"hash":"{hash}","albumId":"{album_id}"}}"#))
            .dispatch();

        assert!(
            resp.status().code >= 400,
            "must return 4xx when file is missing (got {:?})",
            resp.status()
        );

        // DB alias must be unchanged
        {
            let txn = TREE.in_disk.begin_read().expect("begin read");
            let table = txn.open_table(DATA_TABLE).expect("open table");
            let guard = table
                .get(hash.as_str())
                .expect("get")
                .expect("image still in redb");
            let AbstractData::Image(img) = guard.value() else {
                panic!("not an image")
            };
            assert_eq!(
                img.metadata.alias[0].file,
                ghost.to_string_lossy().as_ref(),
                "alias must be unchanged after failed assign"
            );
        }
    }

    // ─── Scenario K: manual album creation endpoint removed ───────────────────

    /// POST /post/create_empty_album must not exist in the new design — dir
    /// albums are created implicitly by the indexer, not via API.
    /// Currently returns 200; this test fails until the endpoint is removed.
    #[test]
    fn scenario_k_create_empty_album_endpoint_removed() {
        let client = make_client();
        let cookie = auth_cookie(&client);
        let resp = client
            .post("/post/create_empty_album")
            .cookie(cookie)
            .dispatch();
        assert_eq!(
            resp.status(),
            Status::NotFound,
            "POST /post/create_empty_album must be removed (currently returns 200)"
        );
    }

    // ─── Scenario L: dir-album membership is reflected in the explicit
    // `album` field at index time ───────────────────────────────────────────

    /// Regression test for a fixed bug (see TODO.md "Known bugs"): the
    /// explicit per-photo `album` field — the one the info sidepane and
    /// `AssignAlbumModal`'s "current album" badge read — used to be set
    /// only by `PUT /put/assign_album`. Normal indexing (including the
    /// filesystem-watcher path exercised here through `index_for_watch`)
    /// never set it for directory-hierarchy albums, even though the file
    /// was physically inside the album's directory and correctly counted
    /// by path-based album membership (`generate_filter.rs`) — causing the
    /// sidepane to always show "No album" until a photo was manually
    /// (re-)assigned. Fixed by resolving the file's parent directory's
    /// dir-album (`workflow::ensure_dir_albums` / `get_album_id_for_dir`)
    /// and passing it through as the indexed item's album.
    // Note: these scenarios use a plain `#[test]` plus a throwaway Tokio
    // runtime for just the `index_for_watch` call, rather than
    // `#[tokio::test]`. `TEST_ENV`'s lazy initialiser drives a
    // `rocket::local::blocking::Client`, which internally does its own
    // `block_on`; if that first run happened inside a `#[tokio::test]`'s
    // own runtime, it would panic with "Cannot start a runtime from within
    // a runtime". Touching `TEST_ENV` here, before entering our own
    // runtime, keeps that blocking-client init outside of any async
    // context.
    #[test]
    fn scenario_l_dir_album_membership_not_set_at_index_time() {
        let data = {
            let _ = &*TEST_ENV;
            DATA_PATH.get().expect("DATA_PATH initialised")
        };

        let album_dir = data.join("e2e_l_album");
        std::fs::create_dir_all(&album_dir).expect("create album dir");
        let album_id = make_dir_album(&album_dir);

        let photo_path = album_dir.join("e2e_l_photo.jpg");
        write_real_jpeg(&photo_path, [120, 130, 140]);

        let rt = tokio::runtime::Runtime::new().expect("build runtime");
        rt.block_on(async {
            index_for_watch(photo_path.clone(), None)
                .await
                .expect("index_for_watch must succeed");
            wait_for_flush().await;
        });

        let hash = find_hash_by_alias_path(&photo_path);
        let txn = TREE.in_disk.begin_read().expect("begin read");
        let table = txn.open_table(DATA_TABLE).expect("open table");
        let guard = table
            .get(hash.as_str())
            .expect("redb get")
            .expect("indexed record must be in redb");
        let abstract_data = guard.value();

        assert_eq!(
            abstract_data.album(),
            Some(album_id),
            "a photo indexed inside a dir-album's directory must have its \
             explicit `album` field set to that album (currently never set \
             by indexing — this is the sidepane \"No album\" bug)"
        );
    }

    // ─── Scenario M: watcher re-indexing after assign_album does not
    // duplicate the alias entry ─────────────────────────────────────────────

    /// Regression test for a fixed bug: `assign_album` renames the file on
    /// disk, which the filesystem watcher (`start_watcher.rs`) observes as
    /// a `Create` event at the destination path and re-indexes via
    /// `index_for_watch(path, None)`. Since the hash already exists,
    /// `DeduplicateTask` (`deduplicate.rs`) used to push another alias
    /// entry for the *same* path instead of recognising it already
    /// matched the current alias, so the alias list grew by one entry on
    /// every reassignment. Fixed by pruning dead aliases and skipping the
    /// push when the path is already present.
    #[test]
    fn scenario_m_watcher_reindex_after_assign_duplicates_alias() {
        let data = {
            let _ = &*TEST_ENV;
            DATA_PATH.get().expect("DATA_PATH initialised")
        };

        let import_dir = data.join("e2e_m_import");
        std::fs::create_dir_all(&import_dir).expect("create import dir");
        let src = import_dir.join("e2e_m_photo.jpg");
        write_real_jpeg(&src, [10, 20, 30]);

        let album_dir = data.join("e2e_m_album");
        std::fs::create_dir_all(&album_dir).expect("create album dir");
        let album_id = make_dir_album(&album_dir);

        let rt = tokio::runtime::Runtime::new().expect("build runtime");
        rt.block_on(async {
            index_for_watch(src.clone(), None)
                .await
                .expect("initial index_for_watch must succeed");
            wait_for_flush().await;
        });
        let hash = find_hash_by_alias_path(&src);

        let client = make_client();
        let resp = client
            .put("/put/assign_album")
            .cookie(auth_cookie(&client))
            .header(ContentType::JSON)
            .body(format!(r#"{{"hash":"{hash}","albumId":"{album_id}"}}"#))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok, "assign_album must return 200");

        let dest_path = album_dir.join("e2e_m_photo.jpg");
        assert!(dest_path.exists(), "file must have been moved to album dir");

        // Simulate the filesystem watcher observing the Create event at the
        // new path and re-indexing it, as start_watcher.rs does.
        rt.block_on(async {
            index_for_watch(dest_path.clone(), None)
                .await
                .expect("watcher-triggered reindex must succeed");
            wait_for_flush().await;
        });

        let txn = TREE.in_disk.begin_read().expect("begin read");
        let table = txn.open_table(DATA_TABLE).expect("open table");
        let guard = table
            .get(hash.as_str())
            .expect("redb get")
            .expect("still in redb");
        let AbstractData::Image(img) = guard.value() else {
            panic!("not an image")
        };
        assert_eq!(
            img.metadata.alias.len(),
            1,
            "re-discovering the same file at its already-recorded path must \
             not duplicate the alias entry (got {:?})",
            img.metadata.alias
        );
    }

    // ─── Scenario N: tags are discovered from embedded XMP keywords at
    // index time ───────────────────────────────────────────────────────────

    /// Regression test for a fixed gap (see TODO.md "tags discovered at
    /// index time"): tags used to be settable only via `PUT /put/edit_tag`
    /// — nothing in the indexing pipeline extracted keyword metadata
    /// (IPTC/XMP `dc:subject`) embedded by photo tools like
    /// Lightroom/digiKam. `extract_keywords_from_xmp`
    /// (extract_keywords.rs) now scans for an embedded `<dc:subject>` XMP
    /// element and adds its `<rdf:li>` entries as tags. Limitations (PNG
    /// `zTXt` compression, MP4/MOV `uuid` boxes, binary IPTC IIM) are
    /// noted on that function and tracked separately for per-format
    /// coverage.
    #[test]
    fn scenario_n_tags_not_discovered_from_xmp_keywords_at_index_time() {
        let data = {
            let _ = &*TEST_ENV;
            DATA_PATH.get().expect("DATA_PATH initialised")
        };

        let import_dir = data.join("e2e_n_import");
        std::fs::create_dir_all(&import_dir).expect("create import dir");
        let photo_path = import_dir.join("e2e_n_photo.jpg");
        write_real_jpeg_with_xmp_keywords(
            &photo_path,
            [50, 60, 70],
            &["e2e_n_family", "e2e_n_vacation"],
        );

        let rt = tokio::runtime::Runtime::new().expect("build runtime");
        rt.block_on(async {
            index_for_watch(photo_path.clone(), None)
                .await
                .expect("index_for_watch must succeed");
            wait_for_flush().await;
        });

        let hash = find_hash_by_alias_path(&photo_path);
        let txn = TREE.in_disk.begin_read().expect("begin read");
        let table = txn.open_table(DATA_TABLE).expect("open table");
        let guard = table
            .get(hash.as_str())
            .expect("redb get")
            .expect("indexed record must be in redb");
        let abstract_data = guard.value();
        let tags = abstract_data.tag();

        assert!(
            tags.contains("e2e_n_family") && tags.contains("e2e_n_vacation"),
            "keywords embedded in the file's XMP packet must be discovered \
             and added to tags at index time (got {tags:?})"
        );
    }

    // ─── Scenario O: tags set during indexing are visible via GET
    // /get/get-data (the same field the sidebar reads) ─────────────────────

    /// Regression lock: unlike the per-photo `album` field (Scenario L),
    /// tags ARE already populated through a single, consistently-read
    /// field, so this is expected to pass today. Exercises the real
    /// prefetch -> get-data flow the frontend uses, not just a direct redb
    /// read.
    #[test]
    fn scenario_o_tags_visible_via_get_data_sidebar_path() {
        let _serial = PREFETCH_SERIAL_GUARD.lock().unwrap();
        insert_photos(&[PhotoSpec {
            path: "/e2e_o/tagged.jpg",
            tags: &["e2e_o_family", "e2e_o_vacation"],
            exif_date: None,
        }]);
        let hash = find_hash_by_alias_path(Path::new("/e2e_o/tagged.jpg"));

        let client = make_client();
        let abstract_data = read_current_abstract_data(&client, hash.as_str());

        let tags: Vec<String> = abstract_data["tags"]
            .as_array()
            .expect("tags array")
            .iter()
            .map(|t| t.as_str().expect("tag string").to_owned())
            .collect();

        assert!(tags.contains(&"e2e_o_family".to_string()), "got {tags:?}");
        assert!(tags.contains(&"e2e_o_vacation".to_string()), "got {tags:?}");
    }

    // ─── Scenario P: tags are modifiable via PUT /put/edit_tag ─────────────

    /// Regression lock for the full prefetch -> edit_tag -> get-data round
    /// trip a real client performs when editing tags from the sidepane.
    #[test]
    fn scenario_p_tags_modifiable_via_edit_tag_api() {
        let _serial = PREFETCH_SERIAL_GUARD.lock().unwrap();
        insert_photos(&[PhotoSpec {
            path: "/e2e_p/photo.jpg",
            tags: &[],
            exif_date: None,
        }]);
        let hash = find_hash_by_alias_path(Path::new("/e2e_p/photo.jpg"));

        let client = make_client();
        let (timestamp, index, _token) = prefetch_locate(&client, hash.as_str());

        let resp = client
            .put("/put/edit_tag")
            .cookie(auth_cookie(&client))
            .header(ContentType::JSON)
            .body(format!(
                r#"{{"indexArray":[{index}],"addTagsArray":["e2e_p_added"],"removeTagsArray":[],"timestamp":{timestamp}}}"#
            ))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok, "edit_tag must succeed");

        let abstract_data = read_current_abstract_data(&client, hash.as_str());
        let tags: Vec<String> = abstract_data["tags"]
            .as_array()
            .expect("tags array")
            .iter()
            .map(|t| t.as_str().expect("tag string").to_owned())
            .collect();
        assert!(
            tags.contains(&"e2e_p_added".to_string()),
            "tag added via PUT /put/edit_tag must be visible afterwards: {tags:?}"
        );
    }

    // ─── Scenario Q: the album assigned via the API is visible via GET
    // /get/get-data (the same field the sidebar reads) ─────────────────────

    /// Regression lock: once `assign_album` has set the explicit field,
    /// the sidebar's actual data path (prefetch -> get-data) reflects it
    /// immediately. Complements Scenario L, which locks in that indexing
    /// itself also sets this field.
    #[test]
    fn scenario_q_album_visible_via_get_data_after_assign() {
        let _serial = PREFETCH_SERIAL_GUARD.lock().unwrap();
        let data = {
            let _ = &*TEST_ENV;
            DATA_PATH.get().expect("DATA_PATH initialised")
        };

        let import_dir = data.join("e2e_q_import");
        std::fs::create_dir_all(&import_dir).expect("create import dir");
        let src = import_dir.join("e2e_q_photo.jpg");
        std::fs::write(&src, b"\xff\xd8\xff fake jpeg").expect("write source file");

        let album_dir = data.join("e2e_q_album");
        std::fs::create_dir_all(&album_dir).expect("create album dir");
        let album_id = make_dir_album(&album_dir);

        let hash = insert_photo_with_real_file(&src);

        let client = make_client();
        let resp = client
            .put("/put/assign_album")
            .cookie(auth_cookie(&client))
            .header(ContentType::JSON)
            .body(format!(r#"{{"hash":"{hash}","albumId":"{album_id}"}}"#))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok, "assign_album must return 200");

        let abstract_data = read_current_abstract_data(&client, hash.as_str());

        assert_eq!(
            abstract_data["album"].as_str(),
            Some(album_id.as_str()),
            "GET /get/get-data (the sidebar's actual data source) must \
             reflect the album assigned via PUT /put/assign_album"
        );
    }

    // ─── Scenario R: a file moved externally (no assign_album involved)
    // does not keep a dead alias entry ──────────────────────────────────────

    /// Regression test for a fixed bug, generalising Scenario M:
    /// `DeduplicateTask` (deduplicate.rs) used to only ever *push* a new
    /// alias entry when it re-discovered a known hash at a different
    /// path — it never pruned entries whose `file` no longer existed on
    /// disk. If a user moved a tracked file with a file manager (not
    /// through `assign_album`), the watcher would re-index it at the new
    /// path, and the old, now-nonexistent path would stay in `alias`
    /// forever. Fixed by the same dead-alias pruning as Scenario M.
    #[test]
    fn scenario_r_externally_moved_file_keeps_dead_alias_entry() {
        let data = {
            let _ = &*TEST_ENV;
            DATA_PATH.get().expect("DATA_PATH initialised")
        };

        let import_dir = data.join("e2e_r_import");
        std::fs::create_dir_all(&import_dir).expect("create import dir");
        let src = import_dir.join("e2e_r_photo.jpg");
        write_real_jpeg(&src, [200, 90, 40]);

        let other_dir = data.join("e2e_r_moved_to");
        std::fs::create_dir_all(&other_dir).expect("create destination dir");

        let rt = tokio::runtime::Runtime::new().expect("build runtime");
        rt.block_on(async {
            index_for_watch(src.clone(), None)
                .await
                .expect("initial index_for_watch must succeed");
            wait_for_flush().await;
        });
        let hash = find_hash_by_alias_path(&src);

        // Move the file outside the app (e.g. via a file manager), then
        // simulate the watcher observing the Create event at the new path.
        let dest = other_dir.join("e2e_r_photo.jpg");
        std::fs::rename(&src, &dest).expect("move file externally");
        assert!(!src.exists(), "source path must be vacated by the move");

        rt.block_on(async {
            index_for_watch(dest.clone(), None)
                .await
                .expect("watcher-triggered reindex must succeed");
            wait_for_flush().await;
        });

        let txn = TREE.in_disk.begin_read().expect("begin read");
        let table = txn.open_table(DATA_TABLE).expect("open table");
        let guard = table
            .get(hash.as_str())
            .expect("redb get")
            .expect("still in redb");
        let AbstractData::Image(img) = guard.value() else {
            panic!("not an image")
        };

        let live: Vec<&FileModify> = img
            .metadata
            .alias
            .iter()
            .filter(|a| Path::new(&a.file).exists())
            .collect();
        assert_eq!(
            img.metadata.alias.len(),
            live.len(),
            "alias must not retain entries whose file no longer exists on \
             disk after an external move (got {:?})",
            img.metadata.alias
        );
    }

    // ─── Scenario S: PUT /put/reindex preserves the existing album and tags
    // ────────────────────────────────────────────────────────────────────

    /// Regression lock: `regenerate_metadata_for_image` mutates the record
    /// fetched from redb in place (only `exif_vec`, width/height,
    /// thumbnails and hashes are recomputed; tags are only ever *extended*,
    /// never cleared, and `album` is never touched) — so a full reindex
    /// must NOT lose a previously assigned album or previously added tags.
    #[test]
    fn scenario_s_reindex_preserves_album_and_tags() {
        let _serial = PREFETCH_SERIAL_GUARD.lock().unwrap();
        let data = {
            let _ = &*TEST_ENV;
            DATA_PATH.get().expect("DATA_PATH initialised")
        };

        let import_dir = data.join("e2e_s_import");
        std::fs::create_dir_all(&import_dir).expect("create import dir");
        let src = import_dir.join("e2e_s_photo.jpg");
        write_real_jpeg(&src, [5, 100, 200]);

        let album_dir = data.join("e2e_s_album");
        std::fs::create_dir_all(&album_dir).expect("create album dir");
        let album_id = make_dir_album(&album_dir);

        let rt = tokio::runtime::Runtime::new().expect("build runtime");
        rt.block_on(async {
            index_for_watch(src.clone(), None)
                .await
                .expect("initial index_for_watch must succeed");
            wait_for_flush().await;
        });
        let hash = find_hash_by_alias_path(&src);

        let client = make_client();

        // Assign an album and a tag via the real APIs, like a user would.
        let resp = client
            .put("/put/assign_album")
            .cookie(auth_cookie(&client))
            .header(ContentType::JSON)
            .body(format!(r#"{{"hash":"{hash}","albumId":"{album_id}"}}"#))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok, "assign_album must return 200");

        let (timestamp, index, _token) = prefetch_locate(&client, hash.as_str());
        let resp = client
            .put("/put/edit_tag")
            .cookie(auth_cookie(&client))
            .header(ContentType::JSON)
            .body(format!(
                r#"{{"indexArray":[{index}],"addTagsArray":["e2e_s_keep_me"],"removeTagsArray":[],"timestamp":{timestamp}}}"#
            ))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok, "edit_tag must return 200");

        // Full reindex of the same item.
        let (timestamp, index, _token) = prefetch_locate(&client, hash.as_str());
        let resp = client
            .post("/put/reindex")
            .cookie(auth_cookie(&client))
            .header(ContentType::JSON)
            .body(format!(
                r#"{{"indexArray":[{index}],"timestamp":{timestamp}}}"#
            ))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok, "reindex must return 200");
        rt.block_on(wait_for_flush());

        let abstract_data = read_current_abstract_data(&client, hash.as_str());
        assert_eq!(
            abstract_data["album"].as_str(),
            Some(album_id.as_str()),
            "reindex must not lose the previously assigned album"
        );
        let tags: Vec<String> = abstract_data["tags"]
            .as_array()
            .expect("tags array")
            .iter()
            .map(|t| t.as_str().expect("tag string").to_owned())
            .collect();
        assert!(
            tags.contains(&"e2e_s_keep_me".to_string()),
            "reindex must not lose previously added tags: {tags:?}"
        );
    }

    // ─── Scenario T: path-completion always returns absolute paths ──────────

    /// Regression test: `/get/path-completion` used to list the server's
    /// cwd via a literal `"."`, so its default (no-query) view and the
    /// bare-name search branch returned *relative* paths (e.g. `./photos`).
    /// If a caller saves one of those as `imagePath` (the "Image Path"
    /// settings field, or "One-Time Import"'s folder picker — both use this
    /// endpoint), it gets resolved against `UROCISSA_IMAGE_HOME` later, not
    /// the cwd it was actually picked from — a silent path mismatch that
    /// made `ensure_dir_albums` never match, so imported photos showed up
    /// with no album assigned. The picker must only ever return absolute
    /// paths so a saved `imagePath` always means what it appeared to mean
    /// when picked.
    #[test]
    fn scenario_t_path_completion_returns_absolute_paths() {
        let client = make_client();

        // Default view (empty query) lists cwd contents under `children`.
        let default_view = json_get(&client, "/get/path-completion");
        let children = default_view["children"].as_array().expect("children array");
        assert!(!children.is_empty(), "expected at least one cwd entry");
        for child in children {
            let path = child.as_str().expect("child is a string");
            assert!(
                Path::new(path).is_absolute(),
                "default view must return absolute paths, got {path:?}"
            );
        }

        // Bare-name query (no path separator) hits the "search roots +
        // current directory" branch.
        let bare_name_view = json_get(&client, "/get/path-completion?path=src");
        let bare_name_children = bare_name_view["children"]
            .as_array()
            .expect("children array");
        assert!(
            !bare_name_children.is_empty(),
            "expected at least one match for 'src'"
        );
        for child in bare_name_children {
            let path = child.as_str().expect("child is a string");
            assert!(
                Path::new(path).is_absolute(),
                "bare-name search must return absolute paths, got {path:?}"
            );
        }
    }

    // ─── Scenario U: scanning the configured imagePath discovers both
    // album hierarchy and XMP keyword tags ─────────────────────────────────

    /// `start_image_home_scan` always targets the configured `imagePath`
    /// (unlike `start_folder_import`, which accepts any path), so unlike
    /// importing an arbitrary external folder, scanning must reliably
    /// build the album from the directory structure. Also exercises that
    /// XMP keyword discovery runs the same way through this entry point as
    /// it does for the watcher/one-time-import paths (Scenario N).
    #[test]
    fn scenario_u_image_home_scan_discovers_albums_and_tags() {
        let data = {
            let _ = &*TEST_ENV;
            DATA_PATH.get().expect("DATA_PATH initialised")
        };

        let image_home = data.join("e2e_u_image_home");
        let album_dir = image_home.join("vacation");
        std::fs::create_dir_all(&album_dir).expect("create album dir");

        let photo_path = album_dir.join("e2e_u_photo.jpg");
        write_real_jpeg_with_xmp_keywords(&photo_path, [10, 20, 30], &["e2e_u_sunset"]);

        // Point the configured imagePath at our fixture root, scoped to
        // this test (reset at the end so later tests aren't affected).
        {
            let mut config = APP_CONFIG.get().unwrap().write().unwrap();
            config.public.image_path = Some(image_home.clone());
        }

        start_image_home_scan(false).expect("start_image_home_scan must accept the job");

        let rt = tokio::runtime::Runtime::new().expect("build runtime");
        rt.block_on(async {
            for _ in 0..100 {
                if folder_import_status().state != FolderImportState::Running {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            wait_for_flush().await;
        });

        {
            let mut config = APP_CONFIG.get().unwrap().write().unwrap();
            config.public.image_path = None;
        }

        let status = folder_import_status();
        assert_eq!(
            status.state,
            FolderImportState::Completed,
            "scan must complete: {status:?}"
        );
        assert_eq!(
            status.processed, 1,
            "must process exactly the one fixture photo"
        );

        let hash = find_hash_by_alias_path(&photo_path);
        let txn = TREE.in_disk.begin_read().expect("begin read");
        let table = txn.open_table(DATA_TABLE).expect("open table");
        let guard = table
            .get(hash.as_str())
            .expect("redb get")
            .expect("indexed record must be in redb");
        let abstract_data = guard.value();

        assert!(
            abstract_data.album().is_some(),
            "scanning the configured imagePath must assign an album from \
             the discovered directory hierarchy"
        );

        let tags = abstract_data.tag();
        assert!(
            tags.contains("e2e_u_sunset"),
            "keywords embedded in the file's XMP packet must be discovered \
             during the scan, same as any other indexing path (got {tags:?})"
        );
    }

    // ─── Scenario V: GET /object/imported serves the live source file,
    // including after assign_album moves it ───────────────────────────────

    /// Regression test for the storage architecture fix (see `TODO.md`):
    /// `/object/imported/<hash>.<ext>` used to serve a content-addressed
    /// *copy* of the original under `DATA_HOME`. It now looks up the
    /// record's current `source_path()` and serves that directly, so it
    /// must keep working after the file moves (e.g. via `assign_album`)
    /// without any copy to keep in sync.
    #[test]
    fn scenario_v_imported_route_serves_live_source_after_move() {
        let _serial = PREFETCH_SERIAL_GUARD.lock().unwrap();
        let data = {
            let _ = &*TEST_ENV;
            DATA_PATH.get().expect("DATA_PATH initialised")
        };

        let import_dir = data.join("e2e_v_import");
        std::fs::create_dir_all(&import_dir).expect("create import dir");
        let src = import_dir.join("e2e_v_photo.jpg");
        let original_bytes = b"e2e_v original bytes";
        std::fs::write(&src, original_bytes).expect("write source file");

        let hash = insert_photo_with_real_file(&src);
        let client = make_client();

        let served = fetch_original_bytes(&client, hash.as_str(), "jpg");
        assert_eq!(
            served, original_bytes,
            "must serve the live source file's bytes, not a stale copy"
        );

        let album_dir = data.join("e2e_v_album");
        std::fs::create_dir_all(&album_dir).expect("create album dir");
        let album_id = make_dir_album(&album_dir);

        let resp = client
            .put("/put/assign_album")
            .cookie(auth_cookie(&client))
            .header(ContentType::JSON)
            .body(format!(r#"{{"hash":"{hash}","albumId":"{album_id}"}}"#))
            .dispatch();
        assert_eq!(resp.status(), Status::Ok, "assign_album must return 200");

        let served_after_move = fetch_original_bytes(&client, hash.as_str(), "jpg");
        assert_eq!(
            served_after_move, original_bytes,
            "must still serve the original after assign_album moved it to a new path"
        );
    }

    /// Fetch the original-file bytes via the real client flow: prefetch +
    /// get-data (to obtain the hash-scoped, `allow_original` token embedded
    /// in the response) + `GET /object/imported/<hash-prefix>/<hash>.<ext>`.
    fn fetch_original_bytes(client: &Client, hash: &str, ext: &str) -> Vec<u8> {
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

    // ─── Scenario W: identical content at two simultaneously-existing
    // paths is tracked as two live aliases, not silently merged ───────────

    /// Unlike Scenario M/R (the old path is gone by the time the duplicate
    /// is discovered), this is the genuinely-new-alias path through
    /// `deduplicate_task`: both copies exist on disk at once. Regression
    /// test for the storage-architecture-fix follow-up (`TODO.md`): this
    /// branch used to fire a generic warning on *every* re-index
    /// (including routine same-path ones); it's now a distinct
    /// duplicate-content warning that only fires here. Covering the
    /// behavioral side (both aliases recorded and live) since asserting on
    /// log output isn't practical with this project's current test setup.
    #[test]
    fn scenario_w_duplicate_content_at_two_live_paths_tracked_as_two_aliases() {
        let data = {
            let _ = &*TEST_ENV;
            DATA_PATH.get().expect("DATA_PATH initialised")
        };

        let dir_a = data.join("e2e_w_a");
        let dir_b = data.join("e2e_w_b");
        std::fs::create_dir_all(&dir_a).expect("create dir a");
        std::fs::create_dir_all(&dir_b).expect("create dir b");

        let path_a = dir_a.join("e2e_w_photo.jpg");
        write_real_jpeg(&path_a, [77, 88, 99]);
        let path_b = dir_b.join("e2e_w_photo.jpg");
        std::fs::copy(&path_a, &path_b).expect("duplicate fixture file");

        let rt = tokio::runtime::Runtime::new().expect("build runtime");
        rt.block_on(async {
            index_for_watch(path_a.clone(), None)
                .await
                .expect("index path_a must succeed");
            wait_for_flush().await;
            index_for_watch(path_b.clone(), None)
                .await
                .expect("index path_b (duplicate content) must succeed");
            wait_for_flush().await;
        });

        let hash = find_hash_by_alias_path(&path_a);
        let txn = TREE.in_disk.begin_read().expect("begin read");
        let table = txn.open_table(DATA_TABLE).expect("open table");
        let guard = table
            .get(hash.as_str())
            .expect("redb get")
            .expect("still in redb");
        let AbstractData::Image(img) = guard.value() else {
            panic!("not an image")
        };

        let aliased_files: Vec<&str> = img.metadata.alias.iter().map(|a| a.file.as_str()).collect();
        assert_eq!(
            img.metadata.alias.len(),
            2,
            "identical content at two still-existing paths must be tracked \
             as two live aliases (got {aliased_files:?})"
        );
        assert!(
            aliased_files.contains(&path_a.to_string_lossy().as_ref()),
            "must keep the first path: {aliased_files:?}"
        );
        assert!(
            aliased_files.contains(&path_b.to_string_lossy().as_ref()),
            "must record the duplicate's path: {aliased_files:?}"
        );
    }

    // ─── Scenario X: upload with no target album lands in the ingress
    // folder under imagePath, and is then reachable via assign_album ──────

    /// Regression test for the bug found while planning the storage
    /// architecture fix (`TODO.md`): uploads used to write only to a
    /// `DATA_HOME`-resident staging dir, which got deleted after indexing,
    /// leaving no file under `imagePath` for `assign_album` to move. Now
    /// uploads write directly into their final location.
    #[test]
    fn scenario_x_upload_with_no_album_lands_in_ingress_folder() {
        let data = {
            let _ = &*TEST_ENV;
            DATA_PATH.get().expect("DATA_PATH initialised")
        };

        let image_home = data.join("e2e_x_image_home");
        std::fs::create_dir_all(&image_home).expect("create image home");

        {
            let mut config = APP_CONFIG.get().unwrap().write().unwrap();
            config.public.image_path = Some(image_home.clone());
        }

        let fixture_path = data.join("e2e_x_fixture.jpg");
        write_real_jpeg(&fixture_path, [11, 22, 33]);
        let file_bytes = std::fs::read(&fixture_path).expect("read fixture bytes");

        let client = make_client();
        let (boundary, body) =
            build_upload_multipart_body("e2e_x_photo.jpg", &file_bytes, "image/jpeg", 0);
        let resp = client
            .post("/upload")
            .cookie(auth_cookie(&client))
            .header(
                ContentType::parse_flexible(&format!("multipart/form-data; boundary={boundary}"))
                    .expect("valid content type"),
            )
            .body(body)
            .dispatch();
        assert_eq!(resp.status(), Status::Ok, "upload must return 200");

        // index_for_watch enqueues its DB write via execute_batch_detached
        // (see wait_for_flush's doc comment) -- wait for it before querying.
        let rt = tokio::runtime::Runtime::new().expect("build runtime");
        rt.block_on(wait_for_flush());

        {
            let mut config = APP_CONFIG.get().unwrap().write().unwrap();
            config.public.image_path = None;
        }

        let uploads_dir = image_home.join("uploads");
        let uploaded_files: Vec<_> = std::fs::read_dir(&uploads_dir)
            .expect("read uploads dir")
            .filter_map(Result::ok)
            .collect();
        assert_eq!(
            uploaded_files.len(),
            1,
            "exactly one file must land in the default ingress folder under imagePath"
        );
        let uploaded_path = uploaded_files[0].path();
        assert_eq!(
            std::fs::read(&uploaded_path).expect("read uploaded file"),
            file_bytes,
            "uploaded file content must match what was sent"
        );

        let hash = find_hash_by_alias_path(&uploaded_path);

        // The ingress folder itself became a top-level dir-album; confirm
        // assign_album (which requires the recorded alias path to exist on
        // disk) now works for an uploaded photo.
        let album_dir = data.join("e2e_x_album");
        std::fs::create_dir_all(&album_dir).expect("create album dir");
        let album_id = make_dir_album(&album_dir);

        let resp = client
            .put("/put/assign_album")
            .cookie(auth_cookie(&client))
            .header(ContentType::JSON)
            .body(format!(r#"{{"hash":"{hash}","albumId":"{album_id}"}}"#))
            .dispatch();
        assert_eq!(
            resp.status(),
            Status::Ok,
            "assign_album must succeed for an uploaded photo now that it \
             has a real file under imagePath"
        );
    }

    // ─── Scenario Y: upload with a target album writes directly into that
    // album's real directory ────────────────────────────────────────────

    #[test]
    fn scenario_y_upload_with_album_writes_into_album_directory() {
        let data = {
            let _ = &*TEST_ENV;
            DATA_PATH.get().expect("DATA_PATH initialised")
        };

        let album_dir = data.join("e2e_y_album");
        std::fs::create_dir_all(&album_dir).expect("create album dir");
        let album_id = make_dir_album(&album_dir);

        let fixture_path = data.join("e2e_y_fixture.jpg");
        write_real_jpeg(&fixture_path, [44, 55, 66]);
        let file_bytes = std::fs::read(&fixture_path).expect("read fixture bytes");

        let client = make_client();
        let (boundary, body) =
            build_upload_multipart_body("e2e_y_photo.jpg", &file_bytes, "image/jpeg", 0);
        let resp = client
            .post(format!("/upload?presigned_album_id_opt={album_id}"))
            .cookie(auth_cookie(&client))
            .header(
                ContentType::parse_flexible(&format!("multipart/form-data; boundary={boundary}"))
                    .expect("valid content type"),
            )
            .body(body)
            .dispatch();
        assert_eq!(resp.status(), Status::Ok, "upload must return 200");

        let rt = tokio::runtime::Runtime::new().expect("build runtime");
        rt.block_on(wait_for_flush());

        let uploaded_files: Vec<_> = std::fs::read_dir(&album_dir)
            .expect("read album dir")
            .filter_map(Result::ok)
            .collect();
        assert_eq!(
            uploaded_files.len(),
            1,
            "the uploaded file must land directly in the target album's \
             real directory, not a staging dir"
        );
        assert_eq!(
            std::fs::read(uploaded_files[0].path()).expect("read uploaded file"),
            file_bytes,
            "uploaded file content must match what was sent"
        );

        let hash = find_hash_by_alias_path(&uploaded_files[0].path());
        let txn = TREE.in_disk.begin_read().expect("begin read");
        let table = txn.open_table(DATA_TABLE).expect("open table");
        let guard = table
            .get(hash.as_str())
            .expect("redb get")
            .expect("indexed record must be in redb");
        assert_eq!(
            guard.value().album(),
            Some(album_id),
            "uploaded photo must have the presigned album set"
        );
    }

    /// Build a `multipart/form-data` body for `POST /upload`: one `file`
    /// part (binary, with the given filename/content-type) and one
    /// `lastModified` text part, matching what the frontend sends.
    fn build_upload_multipart_body(
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

    // ─── Scenario Z: force-reindexing a Scan Image Path run refreshes
    // already-known files, not just newly-discovered ones ──────────────────

    /// `start_image_home_scan(force: false)` (the default) only processes
    /// brand-new hashes — it can't fix an already-indexed record with
    /// stale/incomplete metadata (e.g. one written before a metadata
    /// extraction feature existed). `force: true` re-runs full metadata
    /// extraction for every matched file regardless of whether its hash is
    /// already known, so this is the action exposed for "fix
    /// inconsistencies" / "launching with an existing file repo".
    #[test]
    fn scenario_z_force_reindex_refreshes_already_known_files() {
        let data = {
            let _ = &*TEST_ENV;
            DATA_PATH.get().expect("DATA_PATH initialised")
        };

        let image_home = data.join("e2e_z_image_home");
        let album_dir = image_home.join("vacation");
        std::fs::create_dir_all(&album_dir).expect("create album dir");

        let photo_path = album_dir.join("e2e_z_photo.jpg");
        write_real_jpeg_with_xmp_keywords(&photo_path, [80, 90, 100], &["e2e_z_sunset"]);

        let real_hash =
            blake3_hasher(std::fs::File::open(&photo_path).expect("open fixture for hashing"))
                .expect("compute real content hash");
        insert_stale_photo_record(&photo_path, real_hash);

        {
            let mut config = APP_CONFIG.get().unwrap().write().unwrap();
            config.public.image_path = Some(image_home.clone());
        }

        let rt = tokio::runtime::Runtime::new().expect("build runtime");

        // Non-forced scan: the hash is already known, so it must be left
        // untouched (still the stale 1x1 placeholder, no tags).
        start_image_home_scan(false).expect("start_image_home_scan (no force) must accept the job");
        rt.block_on(async {
            for _ in 0..100 {
                if folder_import_status().state != FolderImportState::Running {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            wait_for_flush().await;
        });

        {
            let txn = TREE.in_disk.begin_read().expect("begin read");
            let table = txn.open_table(DATA_TABLE).expect("open table");
            let abstract_data = table
                .get(real_hash.as_str())
                .expect("redb get")
                .expect("still in redb")
                .value();
            assert_eq!(
                abstract_data.width(),
                1,
                "non-forced scan must not touch an already-known record's metadata"
            );
            assert!(
                abstract_data.tag().is_empty(),
                "non-forced scan must not discover tags for an already-known record"
            );
        }

        // Forced scan: must refresh the existing record's metadata.
        start_image_home_scan(true).expect("start_image_home_scan (force) must accept the job");
        rt.block_on(async {
            for _ in 0..100 {
                if folder_import_status().state != FolderImportState::Running {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            wait_for_flush().await;
        });

        {
            let mut config = APP_CONFIG.get().unwrap().write().unwrap();
            config.public.image_path = None;
        }

        let status = folder_import_status();
        assert_eq!(
            status.state,
            FolderImportState::Completed,
            "forced scan must complete: {status:?}"
        );

        let txn = TREE.in_disk.begin_read().expect("begin read");
        let table = txn.open_table(DATA_TABLE).expect("open table");
        let abstract_data = table
            .get(real_hash.as_str())
            .expect("redb get")
            .expect("still in redb")
            .value();

        assert_eq!(
            abstract_data.width(),
            4,
            "forced scan must refresh dimensions from the real file (got {:?})",
            abstract_data.width()
        );
        assert!(
            abstract_data.tag().contains("e2e_z_sunset"),
            "forced scan must (re-)discover tags from the file's XMP packet (got {:?})",
            abstract_data.tag()
        );
        assert!(
            abstract_data.album().is_some(),
            "forced scan must also (re-)assign the album from the directory hierarchy"
        );
    }
}
