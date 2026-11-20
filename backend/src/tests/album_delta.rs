#![cfg(test)]

//! Cross-route album-delta coverage: membership-mutating writes outside the
//! flush batch (trash toggle, album detach) and a drift guard that walks the
//! mutation sites and checks `ASSET_BY_ALBUM` + album aggregates after each.

use std::path::{Path, PathBuf};

use arrayvec::ArrayString;
use snapfab::{PhotoSpec, generate_batch};

use crate::model::abstract_data::AbstractData;
use crate::storage::asset_store;
use crate::tasks::batcher::flush_tree::{flush_tables, sweep_remove_batch};
use crate::tests::bootstrap::{
    TEST_ENV, album_member_ids, assert_album_aggregates_exact, lock, read_album_payload,
    reset_backend_state, test_image_home,
};
use proptest::prelude::*;

fn make_jpeg(dir: &Path, name: &str, side: u32) -> PathBuf {
    generate_batch(&[PhotoSpec {
        output: Some(dir.join(name).to_string_lossy().into_owned()),
        format: Some("jpeg".into()),
        width: Some(side),
        height: Some(side),
        tags: None,
        exif_date: None,
        minimal: false,
    }])
    .expect("generate jpeg");
    dir.join(name)
}

fn hash_file(path: &Path) -> ArrayString<64> {
    let file = std::fs::File::open(path).expect("open generated jpeg");
    crate::process::hash::blake3_hasher(file).expect("hash generated jpeg")
}

fn create_album(dir_name: &str) -> (ArrayString<64>, PathBuf) {
    let dir = test_image_home().join("album_delta_flow").join(dir_name);
    std::fs::create_dir_all(&dir).expect("create album dir");
    let album_id =
        crate::process::dir_album::get_or_create_dir_album(dir.clone()).expect("create dir album");
    (album_id, dir)
}

fn flush_member(dir: &Path, name: &str, side: u32, album_id: ArrayString<64>) -> PathBuf {
    let path = make_jpeg(dir, name, side);
    let hash = hash_file(&path);
    let mut data = AbstractData::new(&path, hash).expect("build member data");
    data.set_album(Some(album_id));
    flush_tables(&[data], &[]).expect("flush album member");
    path
}

fn member_id_at(path: &Path) -> ArrayString<64> {
    asset_store::get_asset_id_by_path(&path.to_string_lossy())
        .expect("read member path")
        .expect("member path exists")
}

#[test]
fn trash_toggle_adds_and_removes_member() {
    let _guard = lock();
    let _ = &*TEST_ENV;
    reset_backend_state();
    let (album_id, dir) = create_album("trash");
    let path = flush_member(&dir, "photo.jpg", 4, album_id);
    let asset_id = member_id_at(&path);
    assert_eq!(
        album_member_ids(album_id.as_str()),
        vec![asset_id.to_string()]
    );

    let view = crate::process::transitor::compose_by_asset_id(asset_id.as_str())
        .expect("compose member")
        .expect("member view exists");
    crate::process::transitor::store_metadata_record(asset_id.as_str(), &view, Some(true))
        .expect("trash member");

    assert_eq!(
        album_member_ids(album_id.as_str()),
        Vec::<String>::new(),
        "trashing must remove the member row"
    );
    let payload = read_album_payload(album_id.as_str());
    assert_eq!(payload.item_count, 0);
    assert_eq!(payload.item_size, 0);
    assert_album_aggregates_exact(album_id.as_str());
    assert!(
        asset_store::get_asset_by_id(asset_id.as_str())
            .expect("read trashed record")
            .expect("record exists")
            .is_trashed
    );

    crate::process::transitor::store_metadata_record(asset_id.as_str(), &view, Some(false))
        .expect("untrash member");

    assert_eq!(
        album_member_ids(album_id.as_str()),
        vec![asset_id.to_string()],
        "untrashing must restore the member row"
    );
    let payload = read_album_payload(album_id.as_str());
    assert_eq!(payload.item_count, 1);
    assert_eq!(payload.cover, Some(asset_id));
    assert_album_aggregates_exact(album_id.as_str());

    std::fs::remove_dir_all(&dir).ok();
    reset_backend_state();
}

#[test]
fn deleted_album_detach_drops_member_rows() {
    let _guard = lock();
    let _ = &*TEST_ENV;
    reset_backend_state();
    let (album_id, dir) = create_album("detach");
    let path = flush_member(&dir, "photo.jpg", 4, album_id);
    let asset_id = member_id_at(&path);

    let mut album_data = crate::process::transitor::compose_by_asset_id(album_id.as_str())
        .expect("compose album")
        .expect("album view exists");
    // Keep the filesystem cleanup out of this test: its root path is emptied,
    // so only the membership detach runs.
    let AbstractData::Album(album) = &mut album_data else {
        panic!("album view expected");
    };
    album.metadata.dir_path.clear();

    crate::router::delete::cleanup_album_descendants(&[album_data]).expect("detach deleted album");

    assert_eq!(
        album_member_ids(album_id.as_str()),
        Vec::<String>::new(),
        "deleting an album must drop its member rows"
    );
    let record = asset_store::get_asset_by_id(asset_id.as_str())
        .expect("read detached record")
        .expect("detached record exists");
    assert_eq!(
        record.album_id, None,
        "remaining members must be detached from the deleted album"
    );

    std::fs::remove_dir_all(&dir).ok();
    reset_backend_state();
}

/// Drift guard over the membership-mutation sites: every site must leave
/// `ASSET_BY_ALBUM` and the album aggregates exact. A mutation that updates
/// one without the other fails here.
#[test]
fn membership_mutations_leave_members_and_aggregates_exact() {
    let _guard = lock();
    let _ = &*TEST_ENV;
    reset_backend_state();
    let (album_a, dir_a) = create_album("drift_a");
    let (album_b, dir_b) = create_album("drift_b");

    // Site: flush insert.
    let path = flush_member(&dir_a, "photo.jpg", 4, album_a);
    let asset_id = member_id_at(&path);
    assert_eq!(
        album_member_ids(album_a.as_str()),
        vec![asset_id.to_string()]
    );
    assert_album_aggregates_exact(album_a.as_str());

    // Site: flush size change.
    let hash = {
        make_jpeg(&dir_a, "photo.jpg", 16);
        hash_file(&path)
    };
    let mut resized = AbstractData::new(&path, hash).expect("build resized data");
    resized.set_album(Some(album_a));
    flush_tables(&[resized], &[]).expect("flush size change");
    assert_eq!(
        album_member_ids(album_a.as_str()),
        vec![asset_id.to_string()]
    );
    assert_album_aggregates_exact(album_a.as_str());

    // Site: trash toggle (both directions).
    let view = crate::process::transitor::compose_by_asset_id(asset_id.as_str())
        .expect("compose member")
        .expect("member view exists");
    crate::process::transitor::store_metadata_record(asset_id.as_str(), &view, Some(true))
        .expect("trash member");
    assert_eq!(album_member_ids(album_a.as_str()), Vec::<String>::new());
    assert_album_aggregates_exact(album_a.as_str());
    crate::process::transitor::store_metadata_record(asset_id.as_str(), &view, Some(false))
        .expect("untrash member");
    assert_eq!(
        album_member_ids(album_a.as_str()),
        vec![asset_id.to_string()]
    );
    assert_album_aggregates_exact(album_a.as_str());

    // Site: move between albums.
    let mut moved = asset_store::get_asset_by_id(asset_id.as_str())
        .expect("read member")
        .expect("member exists");
    let old_path = moved.path.clone();
    let new_path = dir_b.join("photo.jpg").to_string_lossy().into_owned();
    moved.path.clone_from(&new_path);
    moved.album_id = Some(album_b);
    asset_store::move_asset_mapping(&old_path, &new_path, &moved).expect("move member");
    assert_eq!(
        album_member_ids(album_a.as_str()),
        Vec::<String>::new(),
        "source album must lose the member row"
    );
    assert_eq!(
        album_member_ids(album_b.as_str()),
        vec![asset_id.to_string()]
    );
    assert_album_aggregates_exact(album_a.as_str());
    assert_album_aggregates_exact(album_b.as_str());

    // Site: sweep removal.
    let moved_path = PathBuf::from(&new_path);
    std::fs::rename(&old_path, &moved_path).expect("move file on disk");
    std::fs::remove_file(&moved_path).expect("remove swept file");
    sweep_remove_batch(std::slice::from_ref(&moved_path)).expect("sweep member");
    assert_eq!(album_member_ids(album_b.as_str()), Vec::<String>::new());
    assert_album_aggregates_exact(album_b.as_str());

    std::fs::remove_dir_all(&dir_a).ok();
    std::fs::remove_dir_all(&dir_b).ok();
    reset_backend_state();
}

#[test]
fn deleted_album_delta_does_not_resurrect_member_row() {
    let _guard = lock();
    let _ = &*TEST_ENV;
    reset_backend_state();
    let (album_id, dir) = create_album("resurrect");
    let path = flush_member(&dir, "photo.jpg", 4, album_id);
    let asset_id = member_id_at(&path);

    let mut album_data = crate::process::transitor::compose_by_asset_id(album_id.as_str())
        .expect("compose album")
        .expect("album view exists");
    let AbstractData::Album(album) = &mut album_data else {
        panic!("album view expected");
    };
    album.metadata.dir_path.clear();

    crate::router::delete::cleanup_album_descendants(&[album_data]).expect("detach deleted album");

    {
        use crate::storage::db::METADATA_TABLE;
        let txn = crate::storage::db::TREE
            .in_disk
            .begin_write()
            .expect("begin album payload removal");
        {
            let mut table = txn.open_table(METADATA_TABLE).expect("open metadata");
            let _ = table.remove(album_id.as_str());
        }
        txn.commit().expect("commit album payload removal");
    }

    let delta = crate::storage::asset_store::AlbumDelta {
        count_delta: 1,
        member: Some(asset_id),
        is_add: true,
        ..Default::default()
    };
    let txn = crate::storage::db::TREE
        .in_disk
        .begin_write()
        .expect("begin stale delta");
    crate::storage::asset_store::apply_album_delta(&txn, album_id.as_str(), delta)
        .expect("apply stale delta");
    txn.commit().expect("commit stale delta");

    assert_eq!(
        album_member_ids(album_id.as_str()),
        Vec::<String>::new(),
        "a deleted album must not resurrect a member row"
    );

    std::fs::remove_dir_all(&dir).ok();
    reset_backend_state();
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 20, ..Default::default() })]

    #[test]
    fn prop_trash_toggle_interleave_store_level(
        ops in prop::collection::vec(any::<bool>(), 0..=16)
    ) {
        let _guard = lock();
        let _ = &*TEST_ENV;
        reset_backend_state();
        let (album_id, dir) = create_album("trash_prop");
        let path = flush_member(&dir, "photo.jpg", 4, album_id);
        let asset_id = member_id_at(&path);

        let view = crate::process::transitor::compose_by_asset_id(asset_id.as_str())
            .expect("compose member")
            .expect("member view exists");

        let mut trashed = false;
        for want in ops {
            if want != trashed {
                crate::process::transitor::store_metadata_record(asset_id.as_str(), &view, Some(want))
                    .expect("toggle trash");
                trashed = want;
            }
            if trashed {
                assert_eq!(album_member_ids(album_id.as_str()), Vec::<String>::new());
            } else {
                assert_eq!(
                    album_member_ids(album_id.as_str()),
                    vec![asset_id.to_string()]
                );
            }
            let payload = read_album_payload(album_id.as_str());
            if trashed {
                assert_eq!(payload.item_count, 0);
            } else {
                assert_eq!(payload.item_count, 1);
            }
            assert_album_aggregates_exact(album_id.as_str());
        }

        std::fs::remove_dir_all(&dir).ok();
        reset_backend_state();
    }
}
