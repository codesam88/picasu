use anyhow::Context;
use arrayvec::ArrayString;
use mini_executor::BatchTask;

use crate::{
    model::abstract_data::AbstractData,
    model::asset::{AssetKind, AssetRecord},
    storage::db::{ASSET_BY_ID, ASSET_BY_PATH, DUPE_INDEX, METADATA_TABLE, TREE},
};

#[cfg(test)]
pub static FLUSH_DELAY_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Test-only count of write transactions opened by the remove path (single or
/// batched). Bulk delete relies on this staying at one for the metadata phase.
#[cfg(test)]
pub static FLUSH_REMOVE_WRITE_TXN_COUNT: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
pub struct FlushTreeTask {
    pub insert_list: Vec<AbstractData>,
    pub remove_list: Vec<AbstractData>,
}

impl FlushTreeTask {
    pub fn insert(data_list: Vec<AbstractData>) -> Self {
        Self {
            insert_list: data_list,
            remove_list: Vec::new(),
        }
    }

    pub fn remove(abstract_data_list: Vec<AbstractData>) -> Self {
        Self {
            insert_list: Vec::new(),
            remove_list: abstract_data_list,
        }
    }
}

impl BatchTask for FlushTreeTask {
    async fn batch_run(list: Vec<Self>) {
        let mut all_insert_data = Vec::new();
        let mut all_remove_abstract_data = Vec::new();
        for task in list {
            all_insert_data.extend(task.insert_list);
            all_remove_abstract_data.extend(task.remove_list);
        }
        flush_tree_task(&all_insert_data, &all_remove_abstract_data);
    }
}

fn flush_tree_task(insert_list: &[AbstractData], remove_list: &[AbstractData]) {
    if let Err(err) = flush_tables(insert_list, remove_list) {
        log::error!("flush_tree_task: asset flush failed: {err:#}");
    }
    crate::storage::tree_view::tree_changed();
}
/// Write the insert/remove lists to the four asset tables (`ASSET_BY_ID`,
/// `ASSET_BY_PATH`, `DUPE_INDEX`, `METADATA_TABLE`).
///
/// This is the index/file-mutation flush: each inserted `AbstractData` is
/// filesystem-fresh (scan output or a file-mutation result), so it is the
/// one place allowed to derive and write a full [`AssetRecord`] identity.
/// `METADATA_TABLE` receives only the metadata-only payload extracted from
/// the input — never its identity fields.
///
/// Does not invalidate the tree view; `flush_tree_task` does that after the
/// tables are written. The serial writer (`tasks::queue::writer`) calls this
/// directly and advances its own epoch. Tests call this directly so no
/// background rebuild races their assertions.
#[allow(clippy::too_many_lines)]
#[allow(warnings)]
pub(crate) fn flush_tables(
    insert_list: &[AbstractData],
    remove_list: &[AbstractData],
) -> anyhow::Result<()> {
    use crate::model::metadata_record::to_metadata_record;
    use crate::process::hash::generate_random_hash;
    use crate::storage::asset_store;
    use redb::ReadableTable;

    // Test-only hook: slow down a flush so a reset can race it deterministically.
    #[cfg(test)]
    {
        let delay = crate::tasks::batcher::flush_tree::FLUSH_DELAY_MS
            .load(std::sync::atomic::Ordering::SeqCst);
        if delay > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }
    }

    log::debug!(
        "flush_tree_task: {} inserts, {} removes",
        insert_list.len(),
        remove_list.len()
    );

    // Process inserts: each AbstractData gets its own asset record.
    for abstract_data in insert_list {
        let content_hash = abstract_data.hash();
        let path = abstract_data
            .path()
            .map(|a| a.file.clone())
            .unwrap_or_default();

        // Check if this path already has an asset (idempotent re-index).
        let existing_id = if let Ok(Some(existing_id)) = asset_store::get_asset_id_by_path(&path) {
            Some(existing_id)
        } else {
            None
        };
        // Read the previous content hash before `begin_write`: store helpers
        // open their own transactions, which cannot run inside the flush
        // write transaction (redb is single-writer).
        let old_record = existing_id
            .as_ref()
            .and_then(|id| asset_store::get_asset_by_id(id).ok().flatten());
        let old_hash = old_record.as_ref().and_then(|record| record.content_hash);
        let old_album_id = old_record.as_ref().and_then(|r| r.album_id);
        let old_size = old_record.as_ref().map(|r| r.file_size).unwrap_or(0);
        let old_trashed = old_record.as_ref().map(|r| r.is_trashed).unwrap_or(false);
        let asset_id = existing_id.unwrap_or_else(generate_random_hash);

        let kind = match abstract_data {
            AbstractData::Image(_) => AssetKind::Image,
            AbstractData::Video(_) => AssetKind::Video,
            AbstractData::Album(_) => AssetKind::Album,
        };

        let modified = abstract_data.path().map_or(0, |a| a.modified);
        // Trash ownership lives on the record: carry the flag from the
        // freshly scanned / file-mutation input so lean list rows derived
        // from this record see the same trashed state that composition
        // projects back onto the wire.
        let is_trashed = match abstract_data {
            AbstractData::Album(alb) => alb.metadata.is_trashed,
            _ => abstract_data.path().is_some_and(|a| a.is_trashed),
        };
        // The input is filesystem-fresh: its scan_time is the index time.
        // Metadata edits no longer flow through this path, so a re-flush
        // only happens on an actual re-scan or file mutation.
        let scan_time = abstract_data
            .path()
            .map_or_else(|| chrono::Utc::now().timestamp_millis(), |a| a.scan_time);
        let ext = match abstract_data {
            AbstractData::Image(img) => img.metadata.ext.clone(),
            AbstractData::Video(vid) => vid.metadata.ext.clone(),
            AbstractData::Album(_) => String::new(),
        };
        let file_size = match abstract_data {
            AbstractData::Image(img) => img.metadata.size,
            AbstractData::Video(vid) => vid.metadata.size,
            AbstractData::Album(_) => 0,
        };

        let record = AssetRecord {
            asset_id,
            kind,
            path: path.clone(),
            content_hash: Some(content_hash),
            file_size,
            ext,
            modified,
            scan_time,
            is_trashed,
            album_id: abstract_data.album(),
        };

        // Write to ASSET_BY_ID, ASSET_BY_PATH, DUPE_INDEX, and METADATA_TABLE.
        let json =
            serde_json::to_string(&record).context("Failed to serialize AssetRecord for flush")?;

        {
            let write_txn = TREE
                .in_disk
                .begin_write()
                .expect("failed to begin write transaction for asset flush");
            {
                let mut id_table = write_txn
                    .open_table(ASSET_BY_ID)
                    .expect("failed to open ASSET_BY_ID");
                let mut path_table = write_txn
                    .open_table(ASSET_BY_PATH)
                    .expect("failed to open ASSET_BY_PATH");
                let mut dupe_table = write_txn
                    .open_table(DUPE_INDEX)
                    .expect("failed to open DUPE_INDEX");
                let mut metadata_table = write_txn
                    .open_table(METADATA_TABLE)
                    .expect("failed to open METADATA_TABLE");

                // Write to ASSET_BY_ID and ASSET_BY_PATH.
                id_table
                    .insert(&*asset_id, json.as_str())
                    .expect("failed to insert into ASSET_BY_ID");
                path_table
                    .insert(path.as_str(), &*asset_id)
                    .expect("failed to insert into ASSET_BY_PATH");

                // Write the metadata-only payload to METADATA_TABLE keyed by
                // asset_id; identity stays on the AssetRecord written above.
                let payload = to_metadata_record(abstract_data);
                metadata_table
                    .insert(&*asset_id, payload)
                    .expect("failed to insert into METADATA_TABLE");

                log::debug!(
                    "flush_tree: wrote to METADATA_TABLE key={asset_id}, path={path}, content_hash={content_hash}"
                );

                // When the file's bytes changed, first drop this asset from its
                // previous hash group so the old group does not keep stale
                // membership.
                if let Some(old) = old_hash.as_ref()
                    && old != &content_hash
                {
                    remove_from_old_group(&mut dupe_table, old, &asset_id);
                }

                // Update DUPE_INDEX: add this asset_id to the content hash group.
                let dupe_key = &*content_hash;
                let existing: Vec<String> = dupe_table
                    .get(dupe_key)
                    .ok()
                    .flatten()
                    .map(|g| serde_json::from_str(g.value()).unwrap_or_default())
                    .unwrap_or_default();
                let mut ids = existing;
                if !ids.iter().any(|id| id == &*asset_id) {
                    ids.push(asset_id.to_string());
                    if let Ok(ids_json) = serde_json::to_string(&ids) {
                        dupe_table
                            .insert(dupe_key, ids_json.as_str())
                            .expect("failed to update DUPE_INDEX");
                    }
                }

                // Drop table handles so apply_album_delta can open them in the
                // same transaction (redb allows one open per table per txn).
                drop(id_table);
                drop(path_table);
                drop(dupe_table);
                drop(metadata_table);

                let is_placeholder = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    == Some(".__picasu_ph__.jpg");

                // Album aggregate deltas (same transaction as the member row).
                if !is_placeholder {
                    {
                        use crate::storage::asset_store;
                        let new_album = record.album_id;
                        // Removal from a previous album when the album assignment changed.
                        if let Some(old) = old_album_id {
                            if Some(old) != new_album && !old_trashed {
                                asset_store::apply_album_delta(
                                    &write_txn,
                                    old.as_str(),
                                    asset_store::AlbumDelta {
                                        count_delta: -1,
                                        size_delta: -(old_size as i64),
                                        member: Some(asset_id),
                                        is_add: false,
                                        member_timestamp: None,
                                        member_size_override: Some(old_size),
                                        is_update: false,
                                    },
                                )
                                .with_context(|| {
                                    format!(
                                        "Failed to remove asset {asset_id} from album {old} during flush"
                                    )
                                })?;
                            }
                        }
                        if old_album_id == new_album {
                            if let Some(alb) = new_album {
                                if old_trashed && !is_trashed {
                                    asset_store::apply_album_delta(
                                        &write_txn,
                                        alb.as_str(),
                                        asset_store::AlbumDelta {
                                            count_delta: 1,
                                            size_delta: 0,
                                            member: Some(asset_id),
                                            is_add: true,
                                            member_timestamp: None,
                                            member_size_override: None,
                                            is_update: false,
                                        },
                                    )
                                    .with_context(|| {
                                        format!(
                                            "Failed to restore asset {asset_id} into album {alb} during flush"
                                        )
                                    })?;
                                } else if !old_trashed && is_trashed {
                                    asset_store::apply_album_delta(
                                        &write_txn,
                                        alb.as_str(),
                                        asset_store::AlbumDelta {
                                            count_delta: -1,
                                            size_delta: -(old_size as i64),
                                            member: Some(asset_id),
                                            is_add: false,
                                            member_timestamp: None,
                                            member_size_override: Some(old_size),
                                            is_update: false,
                                        },
                                    )
                                    .with_context(|| {
                                        format!(
                                            "Failed to remove trashed asset {asset_id} from album {alb} during flush"
                                        )
                                    })?;
                                } else if !is_trashed && !old_trashed && existing_id.is_some() {
                                    let new_size = file_size;
                                    if new_size != old_size {
                                        let diff = new_size as i64 - old_size as i64;
                                        asset_store::apply_album_delta(
                                            &write_txn,
                                            alb.as_str(),
                                            asset_store::AlbumDelta {
                                                count_delta: 0,
                                                size_delta: diff,
                                                member: Some(asset_id),
                                                is_add: false,
                                                member_timestamp: None,
                                                member_size_override: Some(new_size),
                                                is_update: true,
                                            },
                                        )
                                        .with_context(|| {
                                            format!(
                                                "Failed to apply size update for asset {asset_id} in album {alb} during flush"
                                            )
                                        })?;
                                    }
                                }
                            }
                        } else if let Some(alb) = new_album {
                            if !is_trashed {
                                asset_store::apply_album_delta(
                                    &write_txn,
                                    alb.as_str(),
                                    asset_store::AlbumDelta {
                                        count_delta: 1,
                                        size_delta: 0,
                                        member: Some(asset_id),
                                        is_add: true,
                                        member_timestamp: None,
                                        member_size_override: None,
                                        is_update: false,
                                    },
                                )
                                .with_context(|| {
                                    format!(
                                        "Failed to add asset {asset_id} to album {alb} during flush"
                                    )
                                })?;
                            }
                        }
                    }
                }
            }
            write_txn.commit().context("Failed to commit asset flush")?;
        }
    }

    // Process removes: delete from asset tables, all in ONE write transaction
    // so a bulk delete commits atomically instead of one txn per asset.
    use crate::model::asset::AssetKind;

    // Pre-phase (outside any txn): resolve identities, drop thumbnails, snapshot
    // membership. Filesystem thumbnail deletion stays here, before the txn.
    struct RemoveItem {
        asset_id: ArrayString<64>,
        path: String,
        content_hash: ArrayString<64>,
        album_id: Option<ArrayString<64>>,
        is_trashed: bool,
        file_size: u64,
    }

    let mut items: Vec<RemoveItem> = Vec::new();
    for abstract_data in remove_list {
        let content_hash = abstract_data.hash();
        let path = abstract_data
            .path()
            .map(|a| a.file.clone())
            .unwrap_or_default();

        if path.is_empty() {
            for id in asset_store::get_dupe_ids(&content_hash)? {
                if let Some(record) = asset_store::get_asset_by_id(id.as_ref())?
                    && !std::path::Path::new(&record.path).exists()
                {
                    // Thumbnail deletion (best-effort, outside txn).
                    let other_refs = asset_store::get_dupe_ids(&content_hash)
                        .map(|v| v.len())
                        .unwrap_or(0);
                    if other_refs <= 1 {
                        delete_thumbnail(&content_hash, record.kind);
                    }
                    items.push(RemoveItem {
                        asset_id: id,
                        path: record.path,
                        content_hash,
                        album_id: record.album_id,
                        is_trashed: record.is_trashed,
                        file_size: record.file_size,
                    });
                }
            }
        } else if let Some(asset_id) = asset_store::get_asset_id_by_path(&path)? {
            let record = asset_store::get_asset_by_id(asset_id.as_str())?;
            let (album_id, is_trashed, file_size, kind) = match &record {
                Some(r) => (r.album_id, r.is_trashed, r.file_size, r.kind),
                None => (None, false, 0, AssetKind::Image),
            };
            let other_refs = asset_store::get_dupe_ids(&content_hash)
                .map(|v| v.len())
                .unwrap_or(0);
            if other_refs <= 1 {
                delete_thumbnail(&content_hash, kind);
            }
            items.push(RemoveItem {
                asset_id,
                path,
                content_hash,
                album_id,
                is_trashed,
                file_size,
            });
        }
    }

    if !items.is_empty() {
        #[cfg(test)]
        crate::tasks::batcher::flush_tree::FLUSH_REMOVE_WRITE_TXN_COUNT
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let write_txn = TREE
            .in_disk
            .begin_write()
            .context("Failed to begin write transaction for asset remove")?;
        for item in &items {
            remove_asset_record_in_txn(
                &write_txn,
                &item.asset_id,
                &item.path,
                item.content_hash,
                item.album_id,
                item.is_trashed,
                item.file_size,
            )?;
        }
        write_txn
            .commit()
            .context("Failed to commit asset remove transaction")?;
    }

    Ok(())
}

/// Best-effort thumbnail deletion helper for the remove path.
fn delete_thumbnail(content_hash: &ArrayString<64>, kind: AssetKind) {
    let thumbnail = match kind {
        AssetKind::Image => crate::storage::files::get_data_path().join(format!(
            "object/compressed/{}/{}.jpg",
            &content_hash.as_str()[..2],
            content_hash
        )),
        AssetKind::Video => crate::storage::files::get_data_path().join(format!(
            "object/compressed/{}/{}.mp4",
            &content_hash.as_str()[..2],
            content_hash
        )),
        AssetKind::Album => std::path::PathBuf::new(),
    };
    if !thumbnail.as_os_str().is_empty()
        && thumbnail.exists()
        && let Err(err) = std::fs::remove_file(&thumbnail)
    {
        log::warn!("Failed to delete thumbnail {}: {err}", thumbnail.display());
    }
}

/// Drop `asset_id` from its previous content-hash group (`old_hash`) using
/// the flush transaction's own `DUPE_INDEX` handle.
///
/// Duplicates [`crate::storage::asset_store::remove_from_dupe_group`], but that
/// function opens its own write transaction — calling it here would deadlock,
/// because redb permits only one open write transaction at a time and the
/// flush transaction is already active.
fn remove_from_old_group(
    dupe_table: &mut redb::Table<'_, &str, &str>,
    old_hash: &ArrayString<64>,
    asset_id: &ArrayString<64>,
) {
    use redb::ReadableTable;

    let prior: Vec<String> = dupe_table
        .get(old_hash.as_str())
        .ok()
        .flatten()
        .map_or_default(|g| serde_json::from_str(g.value()).unwrap_or_default());
    let retained: Vec<String> = prior
        .into_iter()
        .filter(|id| id.as_str() != asset_id.as_str())
        .collect();
    if retained.is_empty() {
        dupe_table
            .remove(old_hash.as_str())
            .expect("failed to remove old DUPE_INDEX group");
    } else {
        let ids_json = serde_json::to_string(&retained).expect("failed to serialize dupe IDs");
        dupe_table
            .insert(old_hash.as_str(), ids_json.as_str())
            .expect("failed to update old DUPE_INDEX group");
    }
}

/// Remove one asset record from all tables by its exact identity. Thumbnail
/// deletion stays with the confirmed removal (other refs <= 1) and the
/// metadata row is dropped in the same short transaction. Used by both the
/// normal flush remove path and the batched sweep removal op.
#[allow(clippy::too_many_lines)]
pub(crate) fn remove_asset_record_from_tables(
    asset_id: &ArrayString<64>,
    path: &str,
    content_hash: ArrayString<64>,
    kind: AssetKind,
) -> anyhow::Result<()> {
    use crate::storage::asset_store;
    use crate::storage::db::TREE;
    // Thumbnail deletion belongs to the confirmed record removal, not to the
    // sweep/verify pre-check: a path can transiently vanish while a mutation
    // is moving it, and the lease keeps the record alive in that window.
    let other_refs = asset_store::get_dupe_ids(&content_hash)
        .with_context(|| format!("Failed to read DUPE_INDEX for {content_hash}"))?
        .len();
    if other_refs <= 1 {
        let thumbnail = match kind {
            AssetKind::Image => crate::storage::files::get_data_path().join(format!(
                "object/compressed/{}/{}.jpg",
                &content_hash.as_str()[..2],
                content_hash
            )),
            AssetKind::Video => crate::storage::files::get_data_path().join(format!(
                "object/compressed/{}/{}.mp4",
                &content_hash.as_str()[..2],
                content_hash
            )),
            AssetKind::Album => std::path::PathBuf::new(),
        };
        if !thumbnail.as_os_str().is_empty()
            && thumbnail.exists()
            && let Err(err) = std::fs::remove_file(&thumbnail)
        {
            log::warn!("Failed to delete thumbnail {}: {err}", thumbnail.display());
        }
    }

    // Capture album membership before the record is deleted (read outside txn).
    let (album_id, is_trashed, file_size) =
        match crate::storage::asset_store::get_asset_by_id(asset_id.as_str())? {
            Some(r) => (r.album_id, r.is_trashed, r.file_size),
            None => (None, false, 0),
        };

    #[cfg(test)]
    crate::tasks::batcher::flush_tree::FLUSH_REMOVE_WRITE_TXN_COUNT
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let write_txn = TREE
        .in_disk
        .begin_write()
        .context("Failed to begin write transaction for asset remove")?;
    remove_asset_record_in_txn(
        &write_txn,
        asset_id,
        path,
        content_hash,
        album_id,
        is_trashed,
        file_size,
    )?;
    write_txn
        .commit()
        .context("Failed to commit asset remove transaction")
}

/// Table writes for one asset removal inside an already-open write transaction.
/// Owns no transaction lifecycle and no thumbnail deletion.
#[allow(clippy::too_many_arguments)]
fn remove_asset_record_in_txn(
    write_txn: &redb::WriteTransaction,
    asset_id: &ArrayString<64>,
    path: &str,
    content_hash: ArrayString<64>,
    album_id: Option<ArrayString<64>>,
    is_trashed: bool,
    file_size: u64,
) -> anyhow::Result<()> {
    use crate::storage::db::{ASSET_BY_ID, ASSET_BY_PATH, DUPE_INDEX, METADATA_TABLE};
    use redb::ReadableTable;

    // Record deletes.
    let mut t = write_txn
        .open_table(ASSET_BY_ID)
        .context("Failed to open ASSET_BY_ID for remove")?;
    t.remove(&**asset_id)
        .context("Failed to remove ASSET_BY_ID row")?;
    drop(t);

    let mut t = write_txn
        .open_table(ASSET_BY_PATH)
        .context("Failed to open ASSET_BY_PATH for remove")?;
    t.remove(path)
        .context("Failed to remove ASSET_BY_PATH row")?;
    drop(t);

    // Dupe index: drop this asset from its group.
    let mut t = write_txn
        .open_table(DUPE_INDEX)
        .context("Failed to open DUPE_INDEX for remove")?;
    let prior: Vec<String> = t
        .get(content_hash.as_str())
        .context("Failed to read DUPE_INDEX group")?
        .map_or_default(|g| serde_json::from_str(g.value()).unwrap_or_default());
    let retained: Vec<String> = prior
        .into_iter()
        .filter(|id| id.as_str() != asset_id.as_str())
        .collect();
    if retained.is_empty() {
        t.remove(content_hash.as_str())
            .context("Failed to remove DUPE_INDEX group")?;
    } else {
        let json =
            serde_json::to_string(&retained).context("Failed to serialize retained dupe group")?;
        t.insert(content_hash.as_str(), json.as_str())
            .context("Failed to update DUPE_INDEX group")?;
    }
    drop(t);

    // Metadata row.
    let mut t = write_txn
        .open_table(METADATA_TABLE)
        .context("Failed to open METADATA_TABLE for remove")?;
    t.remove(&**asset_id)
        .context("Failed to remove METADATA_TABLE row")?;
    drop(t);

    // Member row + album aggregate delta.
    if let Some(alb) = album_id
        && !is_trashed
    {
        crate::storage::asset_store::apply_album_delta(
            write_txn,
            alb.as_str(),
            crate::storage::asset_store::AlbumDelta {
                count_delta: -1,
                size_delta: -file_size.cast_signed(),
                member: Some(*asset_id),
                is_add: false,
                member_timestamp: None,
                member_size_override: Some(file_size),
                is_update: false,
            },
        )
        .with_context(|| format!("Failed to apply removal delta for album {alb}"))?;
    }

    Ok(())
}

/// Batched sweep removal: delete each listed path's record by exact path/id
/// only. A path that still exists on disk is skipped (the file reappeared or
/// the candidate was stale), so a batch can never reap a live row. No
/// hash-group reap is performed.
#[quint::operation("OP-9")]
#[allow(warnings)]
pub(crate) fn sweep_remove_batch(paths: &[std::path::PathBuf]) -> anyhow::Result<()> {
    use crate::storage::asset_store;

    for path in paths {
        if path.exists() {
            continue;
        }
        let path_str = path.to_string_lossy().to_string();
        let Some(asset_id) = asset_store::get_asset_id_by_path(&path_str)? else {
            continue;
        };
        match asset_store::get_asset_by_id(asset_id.as_str())? {
            Some(record) => {
                let content_hash = record.content_hash.unwrap_or(record.asset_id);
                // Unified remover handles record deletes + member row + album delta
                // in one txn; the separate delta txn is no longer needed.
                remove_asset_record_from_tables(&asset_id, &path_str, content_hash, record.kind)?;
            }
            None => {
                // A remove that tore between the id-table delete and the path
                // rewrite leaves a mapping whose record is gone. The file is
                // missing too, so the exact-path mapping is the remaining
                // residue; drop it so the next sweep converges instead of
                // re-reading the dangling key forever. Also heal any orphan
                // member row in ASSET_BY_ALBUM pointing at this asset id.
                if let Err(err) = asset_store::remove_asset_by_path(&path_str) {
                    log::warn!("sweep: failed to drop dangling path mapping {path_str}: {err:#}");
                }
                // Heal orphan member rows.
                use crate::storage::db::{ASSET_BY_ALBUM, TREE};
                use redb::ReadableTable;
                let txn = TREE
                    .in_disk
                    .begin_write()
                    .context("Failed to begin write txn for orphan member heal")?;
                let keys_to_remove: Vec<String> = {
                    let table = txn
                        .open_table(ASSET_BY_ALBUM)
                        .context("Failed to open ASSET_BY_ALBUM for orphan heal")?;
                    let mut keys = Vec::new();
                    for row in table
                        .iter()
                        .context("Failed to scan ASSET_BY_ALBUM for orphan heal")?
                    {
                        let (key, value) = row.context("Failed to read ASSET_BY_ALBUM row")?;
                        if value.value() == asset_id.as_str() {
                            keys.push(key.value().to_string());
                        }
                    }
                    keys
                };
                let mut table = txn
                    .open_table(ASSET_BY_ALBUM)
                    .context("Failed to reopen ASSET_BY_ALBUM for orphan heal")?;
                for key in &keys_to_remove {
                    table
                        .remove(key.as_str())
                        .context("Failed to remove orphan ASSET_BY_ALBUM row")?;
                }
                drop(table);
                txn.commit()
                    .context("Failed to commit orphan member heal")?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::hash::blake3_hasher;
    use crate::storage::asset_store;
    use crate::tests::bootstrap::*;
    use redb::{ReadableTable, TableDefinition};
    use std::path::{Path, PathBuf};

    fn clear_table<V: redb::Value>(table_def: TableDefinition<'static, &'static str, V>) {
        let txn = TREE.in_disk.begin_write().expect("begin write for clear");
        {
            let table = txn.open_table(table_def).expect("open table for clear");
            let keys: Vec<String> = table
                .iter()
                .expect("iterate")
                .filter_map(|r| r.ok().map(|(k, _)| k.value().to_string()))
                .collect();
            drop(table);
            let mut table = txn.open_table(table_def).expect("reopen table for clear");
            for key in &keys {
                table.remove(key.as_str()).expect("remove key");
            }
        }
        txn.commit().expect("commit clear");
    }

    /// Flush writes all four tables, so tests must clear all four before
    /// asserting — not just the three asset tables.
    fn clear_all_tables() {
        clear_table(ASSET_BY_PATH);
        clear_table(ASSET_BY_ID);
        clear_table(crate::storage::db::ASSET_BY_ALBUM);
        clear_table(DUPE_INDEX);
        clear_table(METADATA_TABLE);
    }

    fn metadata_table_key_count() -> usize {
        let txn = TREE
            .in_disk
            .begin_read()
            .expect("begin read METADATA_TABLE");
        let table = txn.open_table(METADATA_TABLE).expect("open METADATA_TABLE");
        table.iter().expect("iterate METADATA_TABLE").count()
    }

    fn make_jpeg(dir: &Path, name: &str, side: u32) -> PathBuf {
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(dir.join(name).to_string_lossy().into()),
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
        let file = std::fs::File::open(path).expect("open file for hashing");
        blake3_hasher(file).expect("hash file")
    }

    fn asset_id_at(path: &Path) -> ArrayString<64> {
        asset_store::get_asset_id_by_path(&path.to_string_lossy())
            .expect("lookup ASSET_BY_PATH")
            .expect("asset must exist at path")
    }

    /// Recursively collect every JSON object key of a stored metadata row.
    fn json_keys(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, val) in map {
                    out.push(key.clone());
                    json_keys(val, out);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    json_keys(item, out);
                }
            }
            _ => {}
        }
    }

    /// Serialize the raw `METADATA_TABLE` row for `asset_id` as JSON.
    fn stored_metadata_json(asset_id: &ArrayString<64>) -> serde_json::Value {
        let txn = TREE
            .in_disk
            .begin_read()
            .expect("begin read for stored metadata");
        let table = txn.open_table(METADATA_TABLE).expect("open METADATA_TABLE");
        let row = table
            .get(&**asset_id)
            .expect("read stored metadata row")
            .expect("stored metadata row must exist");
        serde_json::to_value(row.value()).expect("stored row must serialize to JSON")
    }

    /// Identity keys that must never appear in the stored metadata payload —
    /// they live exclusively on `AssetRecord` (`ASSET_BY_ID`) and are
    /// re-projected onto the wire by composition.
    const FORBIDDEN_IDENTITY_KEYS: [&str; 11] = [
        "path",
        "file",
        "modified",
        "scanTime",
        "isTrashed",
        "size",
        "ext",
        "album",
        "dirPath",
        "id",
        "objType",
    ];

    fn assert_no_identity_keys(json: &serde_json::Value, context: &str) {
        let mut keys = Vec::new();
        json_keys(json, &mut keys);
        for forbidden in FORBIDDEN_IDENTITY_KEYS {
            assert!(
                !keys.iter().any(|k| k == forbidden),
                "{context}: stored metadata payload must not contain identity key \
                 `{forbidden}` (identity lives on AssetRecord); keys: {keys:?}"
            );
        }
    }

    /// The stored media metadata row is a metadata-only payload: no `path`,
    /// no top-level `id`, no size/ext/album — all identity lives on
    /// `AssetRecord`.
    #[test]
    fn stored_media_metadata_payload_excludes_identity() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_all_tables();

        let dir = test_image_home().join("flush_payload_slim");
        std::fs::create_dir_all(&dir).expect("create flush test dir");

        let path = make_jpeg(&dir, "photo.jpg", 4);
        let hash = hash_file(&path);
        let data = AbstractData::new(&path, hash).expect("build AbstractData");

        flush_tables(&[data], &[]).expect("flush tables");

        let id = asset_id_at(&path);
        let json = stored_metadata_json(&id);
        assert_no_identity_keys(&json, "image metadata row");

        clear_all_tables();
        std::fs::remove_dir_all(&dir).expect("remove flush test dir");
    }

    /// The stored album metadata row is a metadata-only payload: no `dirPath`,
    /// no top-level `id`, no `isTrashed` — identity lives on `AssetRecord`.
    #[test]
    fn stored_album_metadata_payload_excludes_identity() {
        use crate::model::album::{AlbumCombined, AlbumMetadata};
        use crate::model::object::{ObjectSchema, ObjectType};

        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_all_tables();

        let album_id = ArrayString::from("album_payload_slim_test_id").expect("fits");
        let data = AbstractData::Album(AlbumCombined {
            object: ObjectSchema::new(album_id, ObjectType::Album),
            metadata: AlbumMetadata {
                id: album_id,
                title: Some("Slim".to_string()),
                created_time: 1,
                dir_path: "/tmp/picasu_slim_album".to_string(),
                ..Default::default()
            },
        });

        flush_tables(&[data], &[]).expect("flush tables");

        let stored_id = asset_store::get_asset_id_by_path("")
            .expect("lookup ASSET_BY_PATH")
            .expect("album flush must register under its empty path");
        let json = stored_metadata_json(&stored_id);
        assert_no_identity_keys(&json, "album metadata row");

        clear_all_tables();
    }

    #[test]
    fn flush_identical_bytes_two_paths_two_assets() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_all_tables();

        let dir = test_image_home().join("flush_dup");
        std::fs::create_dir_all(&dir).expect("create flush test dir");

        let path_a = make_jpeg(&dir, "a.jpg", 4);
        std::fs::copy(&path_a, dir.join("b.jpg")).expect("copy test photo");
        let path_b = dir.join("b.jpg");

        let hash_a = hash_file(&path_a);
        let hash_b = hash_file(&path_b);
        assert_eq!(hash_a, hash_b, "identical bytes must hash the same");

        let data_a = AbstractData::new(&path_a, hash_a).expect("build AbstractData for a.jpg");
        let data_b = AbstractData::new(&path_b, hash_b).expect("build AbstractData for b.jpg");

        flush_tables(&[data_a, data_b], &[]).expect("flush tables");

        let id_a = asset_id_at(&path_a);
        let id_b = asset_id_at(&path_b);
        assert_ne!(id_a, id_b, "two paths must get distinct asset IDs");

        let ids = asset_store::get_dupe_ids(&hash_a).expect("read DUPE_INDEX");
        assert_eq!(ids.len(), 2, "same-hash group must hold both assets");
        assert!(ids.contains(&id_a));
        assert!(ids.contains(&id_b));

        clear_all_tables();
        std::fs::remove_dir_all(&dir).expect("remove flush test dir");
    }

    #[test]
    fn flush_same_path_idempotent() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_all_tables();

        let dir = test_image_home().join("flush_idem");
        std::fs::create_dir_all(&dir).expect("create flush test dir");

        let path = make_jpeg(&dir, "photo.jpg", 4);
        let hash = hash_file(&path);
        let data = AbstractData::new(&path, hash).expect("build AbstractData");

        flush_tables(std::slice::from_ref(&data), &[]).expect("flush tables");
        let id_first = asset_id_at(&path);

        flush_tables(&[data], &[]).expect("flush tables");
        let id_second = asset_id_at(&path);

        assert_eq!(id_first, id_second, "same path must keep the same asset ID");

        let ids = asset_store::get_dupe_ids(&hash).expect("read DUPE_INDEX");
        assert_eq!(ids.len(), 1, "re-flush must not grow the dupe group");
        assert_eq!(ids[0], id_first);
        assert_eq!(
            metadata_table_key_count(),
            1,
            "re-flush must not leave duplicate METADATA_TABLE keys"
        );

        clear_all_tables();
        std::fs::remove_dir_all(&dir).expect("remove flush test dir");
    }

    #[test]
    fn flush_changed_hash_moves_dupe_group() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_all_tables();

        let dir = test_image_home().join("flush_changed");
        std::fs::create_dir_all(&dir).expect("create flush test dir");

        let path = make_jpeg(&dir, "photo.jpg", 4);
        let hash_a = hash_file(&path);
        let hash_b = ArrayString::from("changed_bytes_hash_b").expect("fits in 64 chars");
        assert_ne!(hash_a, hash_b, "test needs two distinct hashes");

        let data_a = AbstractData::new(&path, hash_a).expect("build AbstractData, hash A");
        flush_tables(&[data_a], &[]).expect("flush tables");
        let id_first = asset_id_at(&path);

        let data_b = AbstractData::new(&path, hash_b).expect("build AbstractData, hash B");
        flush_tables(&[data_b], &[]).expect("flush tables");
        let id_second = asset_id_at(&path);

        assert_eq!(id_first, id_second, "same path must keep the same asset ID");

        let group_a = asset_store::get_dupe_ids(&hash_a).expect("read group A");
        assert!(
            !group_a.contains(&id_first),
            "old-hash group must no longer contain the asset"
        );
        let group_b = asset_store::get_dupe_ids(&hash_b).expect("read group B");
        assert_eq!(
            group_b,
            vec![id_second],
            "new-hash group must contain exactly the asset"
        );

        clear_all_tables();
        std::fs::remove_dir_all(&dir).expect("remove flush test dir");
    }

    #[test]
    fn flush_remove_one_same_hash_leaves_other() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_all_tables();

        let dir = test_image_home().join("flush_remove");
        std::fs::create_dir_all(&dir).expect("create flush test dir");

        let path_a = make_jpeg(&dir, "a.jpg", 4);
        std::fs::copy(&path_a, dir.join("b.jpg")).expect("copy test photo");
        let path_b = dir.join("b.jpg");

        let hash = hash_file(&path_a);
        let data_a = AbstractData::new(&path_a, hash).expect("build AbstractData for a.jpg");
        let data_b = AbstractData::new(&path_b, hash).expect("build AbstractData for b.jpg");

        flush_tables(&[data_a.clone(), data_b], &[]).expect("flush tables");
        let id_a = asset_id_at(&path_a);
        let id_b = asset_id_at(&path_b);
        assert_ne!(id_a, id_b);

        // The normal remove branch resolves by path via
        // ASSET_BY_PATH and does not consult the filesystem, so the
        // remove input is the same path AbstractData used for the insert.
        flush_tables(&[], &[data_a]).expect("flush tables");

        let found_b = asset_store::get_asset_by_id(id_b.as_str())
            .expect("read ASSET_BY_ID")
            .expect("asset B must survive removal of asset A");
        assert_eq!(found_b.asset_id, id_b);

        let ids = asset_store::get_dupe_ids(&hash).expect("read DUPE_INDEX");
        assert_eq!(ids.len(), 1, "group must keep only the surviving asset");
        assert!(ids.contains(&id_b));
        assert!(!ids.contains(&id_a));

        assert!(
            asset_store::get_asset_by_id(id_a.as_str())
                .expect("read ASSET_BY_ID")
                .is_none(),
            "asset A must be gone from ASSET_BY_ID"
        );
        assert!(
            asset_store::get_asset_id_by_path(&path_a.to_string_lossy())
                .expect("lookup ASSET_BY_PATH")
                .is_none(),
            "asset A path mapping must be gone"
        );
        assert_eq!(
            metadata_table_key_count(),
            1,
            "only B's METADATA_TABLE row remains"
        );

        clear_all_tables();
        std::fs::remove_dir_all(&dir).expect("remove flush test dir");
    }

    /// A remove that committed the `ASSET_BY_ID` delete but not the
    /// `ASSET_BY_PATH` rewrite (the torn window in `asset_store::remove_asset`,
    /// whose three table writes are separate transactions) leaves a path
    /// mapping whose id record is gone. The next sweep sees the file missing
    /// and must drop the residue rather than leave the dangling mapping.
    /// Pins OP-3's `aspectsRemoveTornWindowTest` residue-heal half; it does
    /// not exercise an interruptible remove (none exists) — the residue state
    /// is constructed directly.
    #[test]
    fn remove_torn_window_sweep_removes_dangling_path_mapping() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_all_tables();

        let dir = test_image_home().join("remove_torn_window");
        std::fs::create_dir_all(&dir).expect("create torn-window dir");
        let path = make_jpeg(&dir, "torn.jpg", 4);
        let hash = hash_file(&path);
        let data = AbstractData::new(&path, hash).expect("build torn asset");
        flush_tables(&[data], &[]).expect("flush tables");
        let asset_id = asset_id_at(&path);

        // Torn remove: the id-table delete landed, the path rewrite did not.
        std::fs::remove_file(&path).expect("remove torn file");
        asset_store::remove_asset_by_id(asset_id.as_str()).expect("delete id row only");
        assert!(
            asset_store::get_asset_by_id(asset_id.as_str())
                .expect("read id row")
                .is_none(),
            "residue premise: the id record is gone"
        );
        assert_eq!(
            asset_store::get_asset_id_by_path(&path.to_string_lossy())
                .expect("read dangling path mapping"),
            Some(asset_id),
            "residue premise: the path mapping still points at the deleted id"
        );

        // The sweep enumerates the exact path for a missing file.
        sweep_remove_batch(std::slice::from_ref(&path)).expect("sweep remove batch");

        assert!(
            asset_store::get_asset_id_by_path(&path.to_string_lossy())
                .expect("read path mapping after sweep")
                .is_none(),
            "the next sweep must clear a dangling path mapping"
        );

        clear_all_tables();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sweep_remove_batch_existing_path_is_noop_and_missing_path_removes_row() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_all_tables();

        let dir = test_image_home().join("sweep_remove_batch");
        std::fs::create_dir_all(&dir).expect("create sweep test dir");

        let path_keep = make_jpeg(&dir, "keep.jpg", 4);
        let path_drop = make_jpeg(&dir, "drop.jpg", 4);
        let hash_keep = hash_file(&path_keep);
        let hash_drop = hash_file(&path_drop);

        let data_keep = AbstractData::new(&path_keep, hash_keep).expect("build keep");
        let data_drop = AbstractData::new(&path_drop, hash_drop).expect("build drop");
        flush_tables(&[data_keep.clone(), data_drop.clone()], &[]).expect("flush tables");

        let id_keep = asset_id_at(&path_keep);
        let id_drop = asset_id_at(&path_drop);

        // Existing path: sweep must change nothing.
        sweep_remove_batch(std::slice::from_ref(&path_keep)).expect("sweep remove batch");
        assert!(
            asset_store::get_asset_by_id(id_keep.as_str())
                .expect("read keep")
                .is_some(),
            "existing-path record must survive sweep_remove_batch"
        );
        assert!(
            asset_store::get_asset_id_by_path(&path_keep.to_string_lossy())
                .expect("lookup keep")
                .is_some(),
            "existing-path mapping must survive sweep_remove_batch"
        );

        // Missing path: only that exact row is removed.
        std::fs::remove_file(&path_drop).expect("remove drop file");
        sweep_remove_batch(std::slice::from_ref(&path_drop)).expect("sweep remove batch");
        assert!(
            asset_store::get_asset_by_id(id_drop.as_str())
                .expect("read drop")
                .is_none(),
            "missing-path record must be removed"
        );
        assert!(
            asset_store::get_asset_id_by_path(&path_drop.to_string_lossy())
                .expect("lookup drop")
                .is_none(),
            "missing-path mapping must be removed"
        );
        assert!(
            asset_store::get_asset_by_id(id_keep.as_str())
                .expect("read keep after drop")
                .is_some(),
            "sibling record must survive exact-path removal"
        );

        clear_all_tables();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Absorbed sweep pruning: a sweep may prune a row whose path is missing
    /// even while a move is in flight; the move's own row rewrite then
    /// re-establishes the destination and both tables stay coherent (no lost
    /// update, no dangling mapping). Covers OP-9 `aspectsAbsorbedSweepTest` at
    /// the observable contract level (the exact lease/generation interleave is
    /// not parked here; convergence of the end state is what is asserted).
    #[test]
    fn absorbed_sweep_prune_does_not_break_move() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_all_tables();

        let dir = test_image_home().join("absorbed_sweep");
        std::fs::create_dir_all(&dir).expect("create absorbed-sweep dir");
        let src_dir = dir.join("src");
        let dst_dir = dir.join("dst");
        std::fs::create_dir_all(&src_dir).expect("create src dir");
        std::fs::create_dir_all(&dst_dir).expect("create dst dir");
        let source = make_jpeg(&src_dir, "photo.jpg", 4);
        let stale_dest = make_jpeg(&dst_dir, "stale.jpg", 4);

        let src_hash = hash_file(&source);
        let stale_hash = hash_file(&stale_dest);
        flush_tables(
            &[
                AbstractData::new(&source, src_hash).expect("build source data"),
                AbstractData::new(&stale_dest, stale_hash).expect("build stale dest data"),
            ],
            &[],
        )
        .expect("flush tables");

        let src_id = asset_id_at(&source);
        let stale_id = asset_id_at(&stale_dest);

        // The destination file is externally deleted; the sweep prunes its row.
        std::fs::remove_file(&stale_dest).expect("remove stale dest file");
        sweep_remove_batch(std::slice::from_ref(&stale_dest)).expect("sweep remove batch");
        assert!(
            asset_store::get_asset_id_by_path(&stale_dest.to_string_lossy())
                .expect("read pruned dest mapping")
                .is_none(),
            "sweep must prune the stale destination mapping"
        );
        assert!(
            asset_store::get_asset_by_id(stale_id.as_str())
                .expect("read pruned dest record")
                .is_none(),
            "sweep must prune the stale destination record"
        );

        // The in-flight move then renames source -> destination and rewrites
        // the row, re-establishing the destination despite the prior prune.
        let dest = dst_dir.join("photo.jpg");
        std::fs::rename(&source, &dest).expect("move file");
        let mut moved = asset_store::get_asset_by_id(src_id.as_str())
            .expect("read moved record")
            .expect("moved record exists");
        moved.path = dest.to_string_lossy().into_owned();
        asset_store::put_asset_by_id(&moved).expect("id-table rewrite");
        asset_store::remove_asset_by_path(&source.to_string_lossy())
            .expect("remove source mapping");
        asset_store::put_asset_by_path(&dest.to_string_lossy(), src_id).expect("put dest mapping");

        // End state coherent.
        let mapped = asset_store::get_asset_id_by_path(&dest.to_string_lossy())
            .expect("read dest mapping")
            .expect("dest mapping exists");
        let record = asset_store::get_asset_by_id(mapped.as_str())
            .expect("read dest record")
            .expect("dest record exists");
        assert_eq!(
            record.path,
            dest.to_string_lossy(),
            "dest mapping must name the dest"
        );
        assert!(
            asset_store::get_asset_id_by_path(&source.to_string_lossy())
                .expect("read source mapping")
                .is_none(),
            "source mapping must be gone"
        );
        // No dangling path mappings under the test dir.
        let rows =
            asset_store::list_asset_paths_under(&dir.to_string_lossy()).expect("list path rows");
        for (path, id) in rows {
            let rec = asset_store::get_asset_by_id(id.as_str())
                .expect("read mapped record")
                .unwrap_or_else(|| panic!("dangling mapping {path}"));
            assert_eq!(rec.path, path, "path mapping and record must agree");
        }

        clear_all_tables();
        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Album deltas applied by flush/sweep ───────────────────────────────

    fn album_member_data(
        dir: &Path,
        name: &str,
        side: u32,
        album_id: ArrayString<64>,
    ) -> (PathBuf, AbstractData) {
        let path = make_jpeg(dir, name, side);
        let hash = hash_file(&path);
        let mut data = AbstractData::new(&path, hash).expect("build album member data");
        data.set_album(Some(album_id));
        (path, data)
    }

    fn prepare_album_dir(name: &str) -> (ArrayString<64>, PathBuf) {
        reset_backend_state();
        let dir = test_image_home().join("flush_album_delta").join(name);
        std::fs::create_dir_all(&dir).expect("create album dir");
        let album_id =
            crate::process::dir_album::get_or_create_dir_album(dir.clone()).expect("create album");
        (album_id, dir)
    }

    #[test]
    fn flush_album_insert_records_member_and_aggregates() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        let (album_id, dir) = prepare_album_dir("insert");
        let (path, data) = album_member_data(&dir, "photo.jpg", 4, album_id);

        flush_tables(&[data], &[]).expect("flush album member");

        let asset_id = asset_id_at(&path);
        assert_eq!(
            album_member_ids(album_id.as_str()),
            vec![asset_id.to_string()],
            "flush must add the member row in the same transaction"
        );
        let payload = read_album_payload(album_id.as_str());
        assert_eq!(payload.item_count, 1);
        // Slice B: cover is no longer recomputed on flush; album is dirty.
        assert_eq!(payload.cover, None);
        assert!(payload.aggregates_dirty);

        clear_all_tables();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn flush_album_size_change_updates_aggregate() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        let (album_id, dir) = prepare_album_dir("size");
        let (path, first) = album_member_data(&dir, "photo.jpg", 4, album_id);
        flush_tables(&[first], &[]).expect("flush first bytes");
        let first_size = read_album_payload(album_id.as_str()).item_size;

        // Re-scan the same path with a larger payload: the flush takes the
        // size-change delta branch (`is_update`).
        let hash = {
            make_jpeg(&dir, "photo.jpg", 16);
            hash_file(&path)
        };
        let mut second = AbstractData::new(&path, hash).expect("build larger data");
        second.set_album(Some(album_id));
        flush_tables(&[second], &[]).expect("flush size change");

        let payload = read_album_payload(album_id.as_str());
        assert_eq!(payload.item_count, 1, "a re-flush keeps one member");
        assert_ne!(payload.item_size, first_size, "size delta must apply");
        // Slice B: cover is no longer recomputed on flush; album is dirty.
        assert_eq!(payload.cover, None);
        assert!(payload.aggregates_dirty);

        clear_all_tables();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn flush_remove_drops_member_and_aggregates() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        let (album_id, dir) = prepare_album_dir("remove");
        let (path, data) = album_member_data(&dir, "photo.jpg", 4, album_id);
        flush_tables(std::slice::from_ref(&data), &[]).expect("flush album member");
        assert_eq!(read_album_payload(album_id.as_str()).item_count, 1);

        flush_tables(&[], &[data]).expect("remove album member");

        assert!(
            album_member_ids(album_id.as_str()).is_empty(),
            "remove must drop the member row"
        );
        let payload = read_album_payload(album_id.as_str());
        assert_eq!(payload.item_count, 0);
        assert_eq!(payload.item_size, 0);
        assert_eq!(payload.cover, None);
        assert_album_aggregates_exact(album_id.as_str());
        assert!(
            asset_store::get_asset_id_by_path(&path.to_string_lossy())
                .expect("read removed path")
                .is_none()
        );

        clear_all_tables();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bulk_remove_uses_single_write_txn() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;

        let (album_a, dir_a) = prepare_album_dir("bulk_a");
        let dir_b = test_image_home().join("flush_album_delta").join("bulk_b");
        std::fs::create_dir_all(&dir_b).expect("create album dir b");
        let album_b = crate::process::dir_album::get_or_create_dir_album(dir_b.clone())
            .expect("create album b");
        let (_pa, data_a) = album_member_data(&dir_a, "a.jpg", 4, album_a);
        let (_pb, data_b) = album_member_data(&dir_b, "b.jpg", 4, album_b);

        flush_tables(&[data_a.clone(), data_b.clone()], &[]).expect("flush members");
        assert_eq!(read_album_payload(album_a.as_str()).item_count, 1);
        assert_eq!(read_album_payload(album_b.as_str()).item_count, 1);

        crate::tasks::batcher::flush_tree::FLUSH_REMOVE_WRITE_TXN_COUNT
            .store(0, std::sync::atomic::Ordering::SeqCst);

        flush_tables(&[], &[data_a, data_b]).expect("bulk remove");

        assert_eq!(
            crate::tasks::batcher::flush_tree::FLUSH_REMOVE_WRITE_TXN_COUNT
                .load(std::sync::atomic::Ordering::SeqCst),
            1,
            "bulk remove metadata phase must use a single write transaction"
        );
        assert_eq!(album_member_ids(album_a.as_str()), Vec::<String>::new());
        assert_eq!(album_member_ids(album_b.as_str()), Vec::<String>::new());
        assert_eq!(read_album_payload(album_a.as_str()).item_count, 0);
        assert_eq!(read_album_payload(album_b.as_str()).item_count, 0);

        clear_all_tables();
        std::fs::remove_dir_all(&dir_a).ok();
        std::fs::remove_dir_all(&dir_b).ok();
    }

    #[test]
    fn sweep_remove_batch_drops_member_and_aggregates() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        let (album_id, dir) = prepare_album_dir("sweep");
        let (path, data) = album_member_data(&dir, "photo.jpg", 4, album_id);
        flush_tables(&[data], &[]).expect("flush album member");
        let asset_id = asset_id_at(&path);
        assert_eq!(
            album_member_ids(album_id.as_str()),
            vec![asset_id.to_string()]
        );

        std::fs::remove_file(&path).expect("remove file before sweep");
        sweep_remove_batch(std::slice::from_ref(&path)).expect("sweep member");

        assert!(
            album_member_ids(album_id.as_str()).is_empty(),
            "sweep must drop the member row"
        );
        let payload = read_album_payload(album_id.as_str());
        assert_eq!(payload.item_count, 0);
        assert_eq!(payload.item_size, 0);
        assert_album_aggregates_exact(album_id.as_str());
        assert!(
            asset_store::get_asset_by_id(asset_id.as_str())
                .expect("read swept record")
                .is_none()
        );

        clear_all_tables();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Lock-order guard for the flush site: `flush_tables` must not hold the
    /// `TREE.in_disk` write transaction while locking `DIR_ALBUM_CACHE`.
    /// Pre-fix the flush thread takes the txn first and then blocks on the
    /// mutex, so the test thread's `begin_write` times out. Post-fix the
    /// flush thread blocks on the mutex before taking the txn, so the test
    /// thread's `begin_write` succeeds.
    #[test]
    fn flush_site_does_not_hold_write_txn_across_dir_album_cache_lock() {
        use std::sync::mpsc;
        use std::time::Duration;

        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_all_tables();

        let dir = test_image_home().join("lock_order_flush");
        std::fs::create_dir_all(&dir).expect("create lock-order test dir");
        let path = make_jpeg(&dir, "photo.jpg", 4);
        let hash = hash_file(&path);
        let data = AbstractData::new(&path, hash).expect("build AbstractData");

        // Hold the dir-album cache mutex for the whole critical section.
        let cache_guard = crate::process::dir_album::lock_dir_album_cache_for_test();

        let (done_tx, done_rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let res = flush_tables(std::slice::from_ref(&data), &[]);
            let _ = done_tx.send(res);
        });

        // Give the flush thread time to reach (and block on) the mutex.
        std::thread::sleep(Duration::from_millis(300));

        // Attempt to begin + commit a write transaction from this thread.
        // Pre-fix: the flush thread holds it -> timeout -> panic naming inversion.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let txn = TREE
                .in_disk
                .begin_write()
                .expect("begin write for lock-order probe");
            txn.commit().expect("commit lock-order probe");
            let _ = tx.send(());
        });

        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(()) => {
                // Post-fix path: flush is blocked on the mutex, txn free.
            }
            Err(err) => {
                panic!(
                    "lock-order inversion: flush thread held the write txn while \
                     waiting for DIR_ALBUM_CACHE: {err}"
                );
            }
        }

        // Release the mutex so the (blocked) flush thread can finish.
        drop(cache_guard);

        let res = done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("flush thread must finish after mutex release");
        res.expect("flush must succeed");

        handle.join().expect("join flush thread");

        clear_all_tables();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Deterministic concurrent barrier seed for R1: a flush-remove runs
    /// concurrently with a `get_or_create_dir_album` call (which may take the
    /// `DIR_ALBUM_CACHE` mutex) and both must complete within a bounded
    /// window. After quiesce the album aggregates and member rows must be
    /// exact (no orphans). Exercises the lock-order fix under a real
    /// interleave rather than the static ordering probe above.
    #[test]
    fn concurrent_flush_remove_and_dir_album_get_or_create_converges() {
        use std::sync::mpsc;
        use std::time::Duration;

        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_all_tables();

        let (album_id, dir) = prepare_album_dir("concurrent_barrier");
        let (path, data) = album_member_data(&dir, "photo.jpg", 4, album_id);
        flush_tables(std::slice::from_ref(&data), &[]).expect("flush album member");
        assert_eq!(read_album_payload(album_id.as_str()).item_count, 1);

        // Slow the flush remove so the get_or_create call can interleave.
        crate::tasks::batcher::flush_tree::FLUSH_DELAY_MS
            .store(100, std::sync::atomic::Ordering::SeqCst);

        let (done_tx, done_rx) = mpsc::channel();
        let data_clone = data.clone();
        let handle = std::thread::spawn(move || {
            let res = flush_tables(&[], std::slice::from_ref(&data_clone));
            let _ = done_tx.send(res);
        });

        // Give the flush thread time to enter its delay (before any lock/txn).
        std::thread::sleep(Duration::from_millis(30));

        // Concurrent call that takes DIR_ALBUM_CACHE while the flush is in
        // flight. Must not deadlock and must return the same album id.
        let concurrent_id = crate::process::dir_album::get_or_create_dir_album(dir.clone())
            .expect("concurrent get_or_create must succeed");
        assert_eq!(concurrent_id, album_id, "album id must be stable");

        let res = done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("flush remove must complete within bound");
        res.expect("flush remove must succeed");
        handle.join().expect("join flush remove thread");

        crate::tasks::batcher::flush_tree::FLUSH_DELAY_MS
            .store(0, std::sync::atomic::Ordering::SeqCst);

        // Exactness after quiesce.
        assert!(
            album_member_ids(album_id.as_str()).is_empty(),
            "remove must drop the member row"
        );
        let payload = read_album_payload(album_id.as_str());
        assert_eq!(payload.item_count, 0);
        assert_eq!(payload.item_size, 0);
        assert_eq!(payload.cover, None);
        assert_album_aggregates_exact(album_id.as_str());
        assert!(
            asset_store::get_asset_id_by_path(&path.to_string_lossy())
                .expect("read removed path")
                .is_none(),
            "removed path mapping must be gone"
        );

        clear_all_tables();
        std::fs::remove_dir_all(&dir).ok();
    }
}
