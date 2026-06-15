/// Scenario-based end-to-end API tests.
///
/// Five scenarios exercise realistic usage configurations.  Each scenario is
/// self-contained: it injects its own fixtures with unique namespaced paths /
/// tags and asserts only on what it created.  Scenarios that mutate global
/// config (album_paths_from_filesystem) serialise via CONFIG_MUTEX.
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
    use redb::ReadableDatabase;
    use rocket::http::{ContentType, Cookie, Status};
    use rocket::local::blocking::Client;
    use serde_json::Value;
    use tempfile::TempDir;

    use crate::operations::dir_album::get_or_create_dir_album;
    use crate::operations::hash::generate_random_hash;
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
    use crate::tasks::actor::album::album_task;

    // ─── Mutex for tests that change app-wide config ──────────────────────────

    static CONFIG_MUTEX: Mutex<()> = Mutex::new(());

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
        // album_paths_from_filesystem starts as false (manual-album mode).
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
        use redb::ReadableTable;
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

    fn set_dir_album_mode(enabled: bool) {
        let _ = &*TEST_ENV;
        APP_CONFIG
            .get()
            .unwrap()
            .write()
            .unwrap()
            .public
            .album_paths_from_filesystem = enabled;
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

    // ─── Scenario C: API-created albums with titles ───────────────────────────

    /// Create two albums via the API, rename one, and verify the listing returns
    /// the correct fields — including that dirPath and parentAlbumId are null
    /// for user-created albums.
    #[test]
    fn scenario_c_api_albums_have_correct_structure() {
        let _guard = CONFIG_MUTEX.lock().unwrap();
        let client = make_client();
        let cookie = auth_cookie(&client);

        let id_a = client
            .post("/post/create_empty_album")
            .cookie(cookie.clone())
            .dispatch()
            .into_string()
            .expect("album id");

        let id_b = client
            .post("/post/create_empty_album")
            .cookie(cookie.clone())
            .dispatch()
            .into_string()
            .expect("album id");

        // Rename album A
        let payload = serde_json::json!({ "albumId": id_a, "title": "Summer Highlights" });
        let r = client
            .put("/put/set_album_title")
            .header(ContentType::JSON)
            .cookie(cookie.clone())
            .body(payload.to_string())
            .dispatch();
        assert_eq!(r.status(), Status::Ok, "set_album_title failed");

        let albums: Vec<Value> =
            serde_json::from_value(json_get(&client, "/get/get-albums")).expect("array");
        let find = |id: &str| {
            albums
                .iter()
                .find(|a| a["albumId"].as_str().unwrap_or("") == id)
                .unwrap_or_else(|| panic!("album {id} not in listing"))
                .clone()
        };

        let a = find(&id_a);
        assert_eq!(a["albumName"], "Summer Highlights");
        assert_eq!(
            a["dirPath"],
            Value::Null,
            "user album: dirPath must be null"
        );
        assert_eq!(
            a["parentAlbumId"],
            Value::Null,
            "user album: parentAlbumId must be null"
        );

        let b = find(&id_b);
        assert_eq!(
            b["albumName"],
            Value::Null,
            "untitled album: albumName must be null"
        );
        assert_eq!(b["dirPath"], Value::Null);
        assert_eq!(b["parentAlbumId"], Value::Null);
    }

    // ─── Scenario D: directory-based photo hierarchy ──────────────────────────

    /// Switch to filesystem-album mode and verify that the parent→child
    /// relationship is correctly exposed via parentAlbumId and dirPath.
    ///
    /// Uses a unique base path (/e2e_d/) so this test does not conflict with
    /// Scenario E's generated tree.
    #[test]
    fn scenario_d_dir_album_parent_child_relationship() {
        let _guard = CONFIG_MUTEX.lock().unwrap();

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

        set_dir_album_mode(true);

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

        // Parent sits at the root of this sub-tree
        assert_eq!(
            parent["parentAlbumId"],
            Value::Null,
            "vacation: parentAlbumId must be null"
        );
        assert_eq!(parent["albumId"].as_str().unwrap(), parent_id.as_str());

        // Child points back to parent
        assert_eq!(
            child["parentAlbumId"].as_str().unwrap(),
            parent_id.as_str(),
            "day1.parentAlbumId must equal vacation.albumId"
        );
        assert_eq!(child["albumId"].as_str().unwrap(), child_id.as_str());

        // The parent album counts only its direct photo (img1.jpg), not day1/img2.jpg.
        // Read from redb directly — background UpdateTreeTask only writes to
        // TREE.in_memory, not to the on-disk DB, so redb is race-free here.
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

        set_dir_album_mode(false);
    }

    // ─── Scenario E: generated multi-level dir tree ───────────────────────────

    /// Build a known three-level directory tree and verify that the hierarchy
    /// properties hold for all nodes:
    ///   - root albums have parentAlbumId == null,
    ///   - each non-root album's parentAlbumId matches its parent's albumId,
    ///   - no album is its own parent.
    #[test]
    fn scenario_e_generated_dir_tree_hierarchy_properties() {
        let _guard = CONFIG_MUTEX.lock().unwrap();

        // (dir_path, parent_dir_path)
        let dirs: &[(&str, Option<&str>)] = &[
            ("/e2e_e/root", None),
            ("/e2e_e/root/alpha", Some("/e2e_e/root")),
            ("/e2e_e/root/beta", Some("/e2e_e/root")),
            ("/e2e_e/root/alpha/deep", Some("/e2e_e/root/alpha")),
        ];

        // One photo per directory to make them non-empty
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

        set_dir_album_mode(true);

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

        set_dir_album_mode(false);
    }

    // ─── Scenario F: empty manual album properties ────────────────────────────

    /// A freshly created album must have zero items and no cover.
    /// Verified in in_memory after the create handler flushes and syncs.
    #[test]
    fn scenario_f_new_album_starts_empty() {
        let client = make_client();
        let cookie = auth_cookie(&client);

        let album_id = client
            .post("/post/create_empty_album")
            .cookie(cookie)
            .dispatch()
            .into_string()
            .expect("album id");

        let in_memory = TREE.in_memory.read().unwrap();
        let album = in_memory
            .iter()
            .find_map(|ts| {
                if let AbstractData::Album(a) = &ts.abstract_data {
                    if a.object.id.as_str() == album_id.as_str() {
                        return Some(a.clone());
                    }
                }
                None
            })
            .expect("new album not found in in_memory");

        assert_eq!(album.metadata.item_count, 0, "new album must have 0 items");
        assert!(
            album.metadata.cover.is_none(),
            "new album must have no cover"
        );
        assert!(
            album.metadata.dir_path.is_none(),
            "API album must have no dir_path"
        );
    }
}
