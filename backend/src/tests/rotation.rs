#![cfg(test)]

//! Rotation persistence: the rotate endpoint records the accumulated display
//! rotation in the asset's XMP sidecar, and a full reindex from the original
//! file reproduces the same displayed orientation without changing the
//! content hash or the asset identity.

use std::fs::File;
use std::path::{Path, PathBuf};

use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::json;
use snapfab::{PhotoSpec, generate_batch};

use crate::model::abstract_data::AbstractData;
use crate::process::hash::blake3_hasher;
use crate::process::misc::generate_thumbhash;
use crate::process::xmp::extract_xmp_data_from_file;
use crate::storage::asset_store;
use crate::tests::bootstrap::{cookie, lock, seed, setup_image_home, test_image_home};
use crate::tests::fixtures::wait::index_timeout_ms;
use crate::tests::fixtures::{auth_cookie, discover_asset_id, test_password, wait_for_album_index};

const PHOTO_REL: &str = "rotation/album/photo.jpg";
const ALBUM_REL: &str = "/rotation/album";
const PHOTO_WIDTH: u32 = 48;
const PHOTO_HEIGHT: u32 = 32;

#[cfg(unix)]
struct RestoreMode<'a> {
    path: &'a Path,
    mode: u32,
}

#[cfg(unix)]
impl Drop for RestoreMode<'_> {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(self.path, std::fs::Permissions::from_mode(self.mode));
    }
}

fn generate_photo() -> PathBuf {
    let photo = test_image_home().join(PHOTO_REL);
    std::fs::create_dir_all(photo.parent().expect("photo parent")).expect("create photo dir");
    generate_batch(&[PhotoSpec {
        output: Some(photo.to_string_lossy().into_owned()),
        format: Some("jpeg".into()),
        width: Some(PHOTO_WIDTH),
        height: Some(PHOTO_HEIGHT),
        tags: None,
        exif_date: None,
        minimal: false,
    }])
    .expect("generate photo fixture");
    photo
}

fn post_index_album(client: &Client) {
    let resp = client
        .post("/post/index/album")
        .cookie(auth_cookie(client))
        .header(ContentType::JSON)
        .body(json!({ "album": ALBUM_REL }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Accepted, "index {ALBUM_REL}");
    wait_for_album_index(client, index_timeout_ms());
}

fn record(asset_id: &str) -> AbstractData {
    asset_store::lookup_abstract_data_by_asset_id(asset_id)
        .expect("read asset record")
        .expect("asset record present")
}

fn compressed_dimensions(data: &AbstractData) -> (u32, u32) {
    let image = image::open(data.compressed_path()).expect("decode compressed JPEG");
    (image.width(), image.height())
}

/// The thumbhash a reindex must produce: the original rotated one quarter turn
/// counter-clockwise, hashed through the same production helper.
fn rotated_thumbhash(photo: &Path) -> Vec<u8> {
    let image = image::open(photo).expect("decode original");
    generate_thumbhash(&image.rotate270())
}

#[test]
fn missing_orientation_indexes_unrotated() {
    let _guard = lock();
    let client = setup_image_home();
    let photo = generate_photo();
    post_index_album(&client);
    let asset_id = discover_asset_id(&client, PHOTO_REL);

    let data = record(&asset_id);
    assert_eq!(
        (data.width(), data.height()),
        (PHOTO_WIDTH, PHOTO_HEIGHT),
        "a file with no rotation marker keeps its original display dimensions"
    );
    assert_eq!(compressed_dimensions(&data), (PHOTO_WIDTH, PHOTO_HEIGHT));
    assert_eq!(extract_xmp_data_from_file(&photo).rotation, 0);
    assert!(
        !photo.with_extension("xmp").exists(),
        "indexing must not invent a sidecar rotation marker"
    );
}

#[test]
fn rotate_persists_to_xmp_and_reindex_preserves_display() {
    let _guard = lock();
    let client = setup_image_home();
    let photo = generate_photo();
    seed(
        "rotation-writer",
        false,
        Some(test_password("rotation-writer").as_str()),
    );
    let viewer = cookie("rotation-writer", false);

    post_index_album(&client);
    let asset_id = discover_asset_id(&client, PHOTO_REL);
    let baseline_hash = blake3_hasher(File::open(&photo).expect("open original")).expect("hash");
    let baseline_thumbhash = record(&asset_id)
        .thumbhash()
        .cloned()
        .expect("baseline thumbhash");

    let resp = client
        .put("/put/rotate-image")
        .cookie(viewer.clone())
        .header(ContentType::JSON)
        .body(json!({ "assetId": asset_id }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "rotate-image: {resp:?}");

    let rotated = record(&asset_id);
    assert_eq!(
        (rotated.width(), rotated.height()),
        (PHOTO_HEIGHT, PHOTO_WIDTH),
        "display dimensions are swapped after a quarter turn"
    );
    assert_eq!(
        compressed_dimensions(&rotated),
        (PHOTO_HEIGHT, PHOTO_WIDTH),
        "the compressed JPEG is re-rendered rotated"
    );
    assert_eq!(extract_xmp_data_from_file(&photo).rotation, 90);
    assert_eq!(
        rotated.hash(),
        baseline_hash,
        "original bytes are untouched, so the content hash must not change"
    );
    assert_eq!(
        blake3_hasher(File::open(&photo).expect("open original")).expect("hash"),
        baseline_hash
    );
    assert_ne!(
        rotated.thumbhash().expect("rotated thumbhash"),
        &baseline_thumbhash,
        "the thumbhash must describe the rotated image"
    );
    assert_eq!(
        asset_store::get_asset_id_by_path(&photo.to_string_lossy())
            .expect("read path mapping")
            .expect("path mapping")
            .as_str(),
        asset_id.as_str(),
        "rotation must not change the asset identity"
    );

    // The regression: reindex from the original bytes must recreate the same
    // display state from the sidecar.
    post_index_album(&client);
    let reindexed = record(&asset_id);
    assert_eq!(
        (reindexed.width(), reindexed.height()),
        (PHOTO_HEIGHT, PHOTO_WIDTH),
        "reindex must keep the swapped display dimensions"
    );
    assert_eq!(
        compressed_dimensions(&reindexed),
        (PHOTO_HEIGHT, PHOTO_WIDTH),
        "reindex must rebuild the compressed JPEG rotated"
    );
    assert_eq!(extract_xmp_data_from_file(&photo).rotation, 90);
    assert_eq!(reindexed.hash(), baseline_hash);
    let expected_thumbhash = rotated_thumbhash(&photo);
    assert_eq!(
        reindexed.thumbhash(),
        Some(&expected_thumbhash),
        "reindex must hash the rotated display image"
    );
    assert_ne!(
        reindexed.thumbhash().expect("reindexed thumbhash"),
        &baseline_thumbhash
    );
    assert_eq!(
        asset_store::get_asset_id_by_path(&photo.to_string_lossy())
            .expect("read path mapping")
            .expect("path mapping")
            .as_str(),
        asset_id.as_str(),
        "reindex must reuse the path's asset id"
    );
}

#[cfg(unix)]
#[test]
fn rotate_sidecar_write_failure_leaves_thumbnail_and_db_unchanged() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock();
    let client = setup_image_home();
    let photo = generate_photo();
    seed(
        "rotation-writer",
        false,
        Some(test_password("rotation-writer").as_str()),
    );
    let viewer = cookie("rotation-writer", false);

    post_index_album(&client);
    let asset_id = discover_asset_id(&client, PHOTO_REL);

    let baseline = record(&asset_id);
    let compressed_path = baseline.compressed_path();
    let baseline_thumb_bytes =
        std::fs::read(&compressed_path).expect("read baseline thumbnail bytes");
    let baseline_width = baseline.width();
    let baseline_height = baseline.height();
    let baseline_thumbhash = baseline.thumbhash().cloned();
    let baseline_phash = match &baseline {
        AbstractData::Image(img) => img.metadata.phash.clone(),
        _ => None,
    };
    let baseline_sidecar_rotation = extract_xmp_data_from_file(&photo).rotation;

    // Make the album directory read-only so the sidecar write fails.
    let album_dir = test_image_home().join(ALBUM_REL.trim_start_matches('/'));
    let original_mode = std::fs::metadata(&album_dir)
        .expect("stat album dir")
        .permissions()
        .mode();
    std::fs::set_permissions(&album_dir, std::fs::Permissions::from_mode(0o555))
        .expect("chmod album dir read-only");

    let restore = RestoreMode {
        path: &album_dir,
        mode: original_mode,
    };

    let resp = client
        .put("/put/rotate-image")
        .cookie(viewer.clone())
        .header(ContentType::JSON)
        .body(json!({ "assetId": asset_id }).to_string())
        .dispatch();
    assert_ne!(
        resp.status(),
        Status::Ok,
        "rotate must fail when the sidecar cannot be written"
    );

    // Restore permissions before reading back.
    drop(restore);

    let after = record(&asset_id);
    let after_thumb_bytes = std::fs::read(&compressed_path).expect("read thumbnail bytes after");
    assert_eq!(
        after_thumb_bytes, baseline_thumb_bytes,
        "thumbnail bytes must be unchanged when the sidecar write fails"
    );
    assert_eq!(after.width(), baseline_width, "DB width must be unchanged");
    assert_eq!(
        after.height(),
        baseline_height,
        "DB height must be unchanged"
    );
    assert_eq!(
        after.thumbhash(),
        baseline_thumbhash.as_ref(),
        "DB thumbhash must be unchanged"
    );
    let after_phash = match &after {
        AbstractData::Image(img) => img.metadata.phash.clone(),
        _ => None,
    };
    assert_eq!(after_phash, baseline_phash, "DB phash must be unchanged");
    assert_eq!(
        extract_xmp_data_from_file(&photo).rotation,
        baseline_sidecar_rotation,
        "sidecar rotation must be unchanged"
    );
}
