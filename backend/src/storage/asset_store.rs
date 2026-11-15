use anyhow::{Context, Result};
use arrayvec::ArrayString;
use redb::ReadableDatabase;

use crate::model::asset::AssetKind;
use crate::model::asset::AssetRecord;
use crate::storage::db::{ASSET_BY_ID, ASSET_BY_PATH, DUPE_INDEX, TREE};

// ── Asset by path ────────────────────────────────────────────────────────────

/// Look up an asset ID by its path.
pub fn get_asset_id_by_path(path: &str) -> Result<Option<ArrayString<64>>> {
    let txn = TREE
        .in_disk
        .begin_read()
        .context("Failed to begin read transaction for ASSET_BY_PATH")?;
    let table = txn
        .open_table(ASSET_BY_PATH)
        .context("Failed to open ASSET_BY_PATH")?;

    match table.get(path)? {
        Some(guard) => {
            let id_str = guard.value();
            let id = ArrayString::from(id_str)
                .map_err(|_| anyhow::anyhow!("Asset ID exceeds 64 bytes: {id_str}"))?;
            Ok(Some(id))
        }
        None => Ok(None),
    }
}

/// List `(path, asset_id)` pairs in `ASSET_BY_PATH` whose path is `root` or
/// a descendant of it. Uses a redb range scan so only the relevant key range
/// is read. `root` may be a directory (trailing slash optional).
#[allow(dead_code)]
pub fn list_asset_paths_under(root: &str) -> Result<Vec<(String, ArrayString<64>)>> {
    let base = root.trim_end_matches('/');
    let txn = TREE
        .in_disk
        .begin_read()
        .context("Failed to begin read transaction for ASSET_BY_PATH")?;
    let table = txn
        .open_table(ASSET_BY_PATH)
        .context("Failed to open ASSET_BY_PATH")?;

    let mut out = Vec::new();
    for row in table
        .range(base..)
        .context("Failed to range scan ASSET_BY_PATH")?
    {
        let (key_guard, value_guard) = row.context("Failed to read ASSET_BY_PATH row")?;
        let key = key_guard.value();
        let is_under = key == base || key.starts_with(&format!("{base}/"));
        if !is_under {
            // Either a sibling sharing the textual prefix or a key past the
            // whole subtree: the range is ordered, so stop scanning.
            break;
        }
        let id_str = value_guard.value();
        let id = ArrayString::from(id_str)
            .map_err(|_| anyhow::anyhow!("Asset ID exceeds 64 bytes: {id_str}"))?;
        out.push((key.to_string(), id));
    }
    Ok(out)
}

/// List `(dir_path, asset_id)` pairs for album assets (`AssetKind::Album`)
/// whose directory path is `root` or a descendant of it. Scans `ASSET_BY_ID`
/// (one table scan) and filters by kind and path prefix, so the in-memory
/// tree view is not consulted. `root` may be a directory (trailing slash
/// optional).
pub fn list_album_assets_under(root: &str) -> Result<Vec<(String, ArrayString<64>)>> {
    use redb::ReadableTable;
    let base = root.trim_end_matches('/');
    let base_path = std::path::Path::new(base);
    let txn = TREE
        .in_disk
        .begin_read()
        .context("Failed to begin read transaction for ASSET_BY_ID")?;
    let table = txn
        .open_table(ASSET_BY_ID)
        .context("Failed to open ASSET_BY_ID")?;

    let mut out = Vec::new();
    for row in table.iter().context("Failed to iterate ASSET_BY_ID")? {
        let (_, value) = row.context("Failed to read row from ASSET_BY_ID")?;
        let record: AssetRecord = match serde_json::from_str(value.value()) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if record.kind != AssetKind::Album {
            continue;
        }
        if record.path.is_empty() {
            continue;
        }
        let dir_path = std::path::Path::new(&record.path);
        if !dir_path.starts_with(base_path) {
            continue;
        }
        out.push((record.path.clone(), record.asset_id));
    }
    Ok(out)
}

/// Insert or update a `path → asset_id` mapping.
pub fn put_asset_by_path(path: &str, asset_id: ArrayString<64>) -> Result<()> {
    let txn = TREE
        .in_disk
        .begin_write()
        .context("Failed to begin write transaction for ASSET_BY_PATH")?;
    {
        let mut table = txn
            .open_table(ASSET_BY_PATH)
            .context("Failed to open ASSET_BY_PATH for write")?;
        table
            .insert(path, &*asset_id)
            .context("Failed to insert into ASSET_BY_PATH")?;
    }
    txn.commit()
        .context("Failed to commit ASSET_BY_PATH insert")
}

/// Remove a `path → asset_id` mapping.
pub fn remove_asset_by_path(path: &str) -> Result<()> {
    let txn = TREE
        .in_disk
        .begin_write()
        .context("Failed to begin write transaction for ASSET_BY_PATH")?;
    {
        let mut table = txn
            .open_table(ASSET_BY_PATH)
            .context("Failed to open ASSET_BY_PATH for write")?;
        table
            .remove(path)
            .context("Failed to remove from ASSET_BY_PATH")?;
    }
    txn.commit()
        .context("Failed to commit ASSET_BY_PATH remove")
}

/// Rewrite an asset's three move-related tables in one write transaction:
/// `ASSET_BY_ID` (updated record), `ASSET_BY_PATH` (old mapping removed, new
/// mapping added). All three land together or not at all, mirroring
/// `apply_dir_move_plans`' single-transaction pattern. The `DUPE_INDEX` is not
/// touched: the content hash is unchanged by a move.
pub fn move_asset_mapping(old_path: &str, new_path: &str, updated: &AssetRecord) -> Result<()> {
    use redb::ReadableTable;
    let txn = TREE
        .in_disk
        .begin_write()
        .context("Failed to begin write transaction for asset move")?;
    {
        let mut id_table = txn
            .open_table(ASSET_BY_ID)
            .context("Failed to open ASSET_BY_ID for write")?;
        let mut path_table = txn
            .open_table(ASSET_BY_PATH)
            .context("Failed to open ASSET_BY_PATH for write")?;

        // Capture the previous album assignment before overwriting the record.
        let old_album = id_table
            .get(&*updated.asset_id)
            .context("Failed to read ASSET_BY_ID during move")?
            .map(|g| {
                serde_json::from_str::<AssetRecord>(g.value())
                    .context("Failed to deserialize AssetRecord during move")
            })
            .transpose()?
            .and_then(|r| r.album_id);

        let json = serde_json::to_string(updated).context("Failed to serialize AssetRecord")?;
        id_table
            .insert(&*updated.asset_id, json.as_str())
            .context("Failed to update ASSET_BY_ID")?;

        path_table
            .remove(old_path)
            .context("Failed to remove old path mapping")?;
        path_table
            .insert(new_path, &*updated.asset_id)
            .context("Failed to add new path mapping")?;

        drop(id_table);
        drop(path_table);

        let new_album = updated.album_id;
        let size = updated.file_size;
        let is_trashed = updated.is_trashed;

        // Member-row re-key + album aggregate deltas, all in this same txn.
        if old_album != new_album {
            if let Some(old_alb) = old_album
                && !is_trashed
            {
                apply_album_delta(
                    &txn,
                    old_alb.as_str(),
                    AlbumDelta {
                        count_delta: -1,
                        size_delta: -size.cast_signed(),
                        member: Some(updated.asset_id),
                        is_add: false,
                        member_timestamp: None,
                        member_size_override: Some(size),
                        is_update: false,
                    },
                )
                .with_context(|| format!("Failed to remove member from album {old_alb}"))?;
            }
            if let Some(new_alb) = new_album
                && !is_trashed
            {
                apply_album_delta(
                    &txn,
                    new_alb.as_str(),
                    AlbumDelta {
                        count_delta: 1,
                        size_delta: 0,
                        member: Some(updated.asset_id),
                        is_add: true,
                        member_timestamp: None,
                        member_size_override: Some(size),
                        is_update: false,
                    },
                )
                .with_context(|| format!("Failed to add member to album {new_alb}"))?;
            }
        }
    }
    txn.commit().context("Failed to commit asset move mapping")
}

// ── Asset by ID ──────────────────────────────────────────────────────────────

/// Look up an asset record by its asset ID.
pub fn get_asset_by_id(asset_id: &str) -> Result<Option<AssetRecord>> {
    let txn = TREE
        .in_disk
        .begin_read()
        .context("Failed to begin read transaction for ASSET_BY_ID")?;
    let table = txn
        .open_table(ASSET_BY_ID)
        .context("Failed to open ASSET_BY_ID")?;

    match table.get(asset_id)? {
        Some(guard) => {
            let record: AssetRecord =
                serde_json::from_str(guard.value()).context("Failed to deserialize AssetRecord")?;
            Ok(Some(record))
        }
        None => Ok(None),
    }
}

/// Get all asset records from `ASSET_BY_ID`.
pub fn get_all_assets() -> Result<Vec<AssetRecord>> {
    use redb::ReadableTable;
    let txn = TREE
        .in_disk
        .begin_read()
        .context("Failed to begin read transaction for ASSET_BY_ID")?;
    let table = txn
        .open_table(ASSET_BY_ID)
        .context("Failed to open ASSET_BY_ID")?;

    let mut records = Vec::new();
    for row in table.iter().context("Failed to iterate ASSET_BY_ID")? {
        let (_, value) = row.context("Failed to read row from ASSET_BY_ID")?;
        if let Ok(record) = serde_json::from_str::<AssetRecord>(value.value()) {
            records.push(record);
        }
    }
    Ok(records)
}

/// Find all asset records whose `path` is a descendant of `dir_path`.
/// The directory itself is NOT included — only children and deeper descendants.
pub fn get_assets_under_path(dir_path: &str) -> Result<Vec<AssetRecord>> {
    let prefix = if dir_path.ends_with('/') {
        dir_path.to_string()
    } else {
        format!("{dir_path}/")
    };

    let all = get_all_assets()?;
    Ok(all
        .into_iter()
        .filter(|r| r.path.starts_with(&prefix))
        .collect())
}

/// Compose the wire `AbstractData` view for `asset_id` from its identity
/// `AssetRecord` plus optional stored metadata payload.
pub fn lookup_abstract_data_by_asset_id(
    asset_id: &str,
) -> Result<Option<crate::model::abstract_data::AbstractData>> {
    let Some(record) = get_asset_by_id(asset_id)? else {
        return Ok(None);
    };
    let txn = TREE
        .in_disk
        .begin_read()
        .context("Failed to begin read transaction")?;
    let table = txn
        .open_table(crate::storage::db::METADATA_TABLE)
        .context("Failed to open METADATA_TABLE")?;
    let payload = table.get(asset_id)?.map(|guard| guard.value());

    Ok(Some(crate::model::metadata_record::compose_abstract_data(
        &record,
        payload.as_ref(),
    )))
}

/// Insert or update an asset record.
pub fn put_asset_by_id(record: &AssetRecord) -> Result<()> {
    let json = serde_json::to_string(record).context("Failed to serialize AssetRecord")?;
    let txn = TREE
        .in_disk
        .begin_write()
        .context("Failed to begin write transaction for ASSET_BY_ID")?;
    {
        let mut table = txn
            .open_table(ASSET_BY_ID)
            .context("Failed to open ASSET_BY_ID for write")?;
        table
            .insert(&*record.asset_id, json.as_str())
            .context("Failed to insert into ASSET_BY_ID")?;
    }
    txn.commit().context("Failed to commit ASSET_BY_ID insert")
}

/// Remove an asset record by ID.
pub fn remove_asset_by_id(asset_id: &str) -> Result<()> {
    let txn = TREE
        .in_disk
        .begin_write()
        .context("Failed to begin write transaction for ASSET_BY_ID")?;
    {
        let mut table = txn
            .open_table(ASSET_BY_ID)
            .context("Failed to open ASSET_BY_ID for write")?;
        table
            .remove(asset_id)
            .context("Failed to remove from ASSET_BY_ID")?;
    }
    txn.commit().context("Failed to commit ASSET_BY_ID remove")
}

// ── Dupe index ───────────────────────────────────────────────────────────────

/// Get the list of asset IDs sharing a content hash.
pub fn get_dupe_ids(content_hash: &str) -> Result<Vec<ArrayString<64>>> {
    let txn = TREE
        .in_disk
        .begin_read()
        .context("Failed to begin read transaction for DUPE_INDEX")?;
    let table = txn
        .open_table(DUPE_INDEX)
        .context("Failed to open DUPE_INDEX")?;

    match table.get(content_hash)? {
        Some(guard) => {
            let ids: Vec<String> =
                serde_json::from_str(guard.value()).context("Failed to deserialize dupe IDs")?;
            ids.iter()
                .map(|s| {
                    ArrayString::from(s)
                        .map_err(|_| anyhow::anyhow!("Asset ID exceeds 64 bytes: {s}"))
                })
                .collect()
        }
        None => Ok(vec![]),
    }
}

/// Add an asset ID to a content hash's duplicate group. Creates the group
/// if it doesn't exist.
#[allow(dead_code)]
pub fn add_to_dupe_group(content_hash: &str, asset_id: ArrayString<64>) -> Result<()> {
    let mut ids = get_dupe_ids(content_hash)?;
    if ids.contains(&asset_id) {
        return Ok(());
    }
    ids.push(asset_id);
    let json = serde_json::to_string(&ids).context("Failed to serialize dupe IDs")?;
    let txn = TREE
        .in_disk
        .begin_write()
        .context("Failed to begin write transaction for DUPE_INDEX")?;
    {
        let mut table = txn
            .open_table(DUPE_INDEX)
            .context("Failed to open DUPE_INDEX for write")?;
        table
            .insert(content_hash, json.as_str())
            .context("Failed to insert into DUPE_INDEX")?;
    }
    txn.commit().context("Failed to commit DUPE_INDEX insert")
}

/// Remove an asset ID from a content hash's duplicate group. Removes the
/// entire group if it becomes empty.
pub fn remove_from_dupe_group(content_hash: &str, asset_id: ArrayString<64>) -> Result<()> {
    let mut ids = get_dupe_ids(content_hash)?;
    ids.retain(|id| *id != asset_id);

    let txn = TREE
        .in_disk
        .begin_write()
        .context("Failed to begin write transaction for DUPE_INDEX")?;
    {
        let mut table = txn
            .open_table(DUPE_INDEX)
            .context("Failed to open DUPE_INDEX for write")?;
        if ids.is_empty() {
            table
                .remove(content_hash)
                .context("Failed to remove from DUPE_INDEX")?;
        } else {
            let json = serde_json::to_string(&ids).context("Failed to serialize dupe IDs")?;
            table
                .insert(content_hash, json.as_str())
                .context("Failed to insert into DUPE_INDEX")?;
        }
    }
    txn.commit().context("Failed to commit DUPE_INDEX update")
}

/// Insert a complete asset (record + path mapping + optional dupe group).
/// This is the primary entry point for adding a new asset to the stores.
#[allow(dead_code)]
pub fn insert_asset(record: &AssetRecord) -> Result<()> {
    put_asset_by_id(record)?;
    put_asset_by_path(&record.path, record.asset_id)?;
    if let Some(hash) = &record.content_hash {
        add_to_dupe_group(hash, record.asset_id)?;
    }
    Ok(())
}

/// Remove a complete asset (record + path mapping + dupe group entry).
/// This is the primary entry point for removing an asset from the stores.
pub fn remove_asset(record: &AssetRecord) -> Result<()> {
    remove_asset_by_id(&record.asset_id)?;
    remove_asset_by_path(&record.path)?;
    if let Some(hash) = &record.content_hash {
        remove_from_dupe_group(hash, record.asset_id)?;
    }
    Ok(())
}

// ── Album membership index (txn-scoped) ──────────────────────────────────────

use crate::storage::db::ASSET_BY_ALBUM;

/// Add one member row to `ASSET_BY_ALBUM` (idempotent).
pub fn add_album_member(
    txn: &redb::WriteTransaction,
    album_id: &str,
    asset_id: &str,
) -> anyhow::Result<()> {
    debug_assert!(
        !album_id.contains('/') && !asset_id.contains('/'),
        "album/asset ids must not contain the key separator"
    );
    let mut table = txn
        .open_table(ASSET_BY_ALBUM)
        .context("Failed to open ASSET_BY_ALBUM for write")?;
    let key = format!("{album_id}/{asset_id}");
    table
        .insert(key.as_str(), asset_id)
        .context("Failed to insert ASSET_BY_ALBUM row")?;
    Ok(())
}

/// Remove one member row from `ASSET_BY_ALBUM`.
pub fn remove_album_member(
    txn: &redb::WriteTransaction,
    album_id: &str,
    asset_id: &str,
) -> anyhow::Result<()> {
    debug_assert!(
        !album_id.contains('/') && !asset_id.contains('/'),
        "album/asset ids must not contain the key separator"
    );
    let mut table = txn
        .open_table(ASSET_BY_ALBUM)
        .context("Failed to open ASSET_BY_ALBUM for write")?;
    let key = format!("{album_id}/{asset_id}");
    let _ = table.remove(key.as_str());
    Ok(())
}

/// List the asset IDs that are members of `album_id` (prefix range scan).
pub fn list_album_members(
    txn: &redb::WriteTransaction,
    album_id: &str,
) -> anyhow::Result<Vec<String>> {
    use redb::ReadableTable;
    let table = txn
        .open_table(ASSET_BY_ALBUM)
        .context("Failed to open ASSET_BY_ALBUM for read")?;
    let prefix = format!("{album_id}/");
    let mut out = Vec::new();
    for row in table
        .range(prefix.as_str()..)
        .context("Failed to range scan ASSET_BY_ALBUM")?
    {
        let (key_guard, value_guard) = row.context("Failed to read ASSET_BY_ALBUM row")?;
        let key = key_guard.value();
        if !key.starts_with(&prefix) {
            break;
        }
        out.push(value_guard.value().to_string());
    }
    Ok(out)
}

/// Drop every member row for `album_id`.
pub fn drop_album_members(txn: &redb::WriteTransaction, album_id: &str) -> anyhow::Result<()> {
    let members = list_album_members(txn, album_id)?;
    let mut table = txn
        .open_table(ASSET_BY_ALBUM)
        .context("Failed to open ASSET_BY_ALBUM for write")?;
    for asset_id in members {
        let key = format!("{album_id}/{asset_id}");
        let _ = table.remove(key.as_str());
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
#[allow(warnings)]
pub struct AlbumDelta {
    pub count_delta: i64,
    pub size_delta: i64,
    pub member: Option<ArrayString<64>>,
    pub is_add: bool,
    /// Override for the member's timestamp (used on removal when the record
    /// has already been overwritten/deleted). Falls back to reading the
    /// member's current record.
    pub member_timestamp: Option<i64>,
    /// Override for the member's size (used on removal). Falls back to
    /// reading the member's current record.
    pub member_size_override: Option<u64>,
    /// When true, the member row is left in place (no add/remove) but the
    /// album's size delta is applied and its time range + cover are recomputed
    /// from the member table (uniform rule for any size/timestamp change).
    pub is_update: bool,
}

const ALBUM_PRIORITY_LIST: &[&str] = &["DateTimeOriginal", "filename", "modified", "scan_time"];

/// Read one member's sort timestamp, size, and thumbhash. A missing record
/// is a legitimate zero fact (the removal paths delete the record before
/// applying the delta); a table or decode failure is not.
#[allow(warnings)]
fn member_info(
    txn: &redb::WriteTransaction,
    asset_id: &str,
) -> anyhow::Result<(i64, u64, Option<Vec<u8>>)> {
    use crate::model::metadata_record::compose_abstract_data;
    use crate::storage::db::{ASSET_BY_ID, METADATA_TABLE};
    use redb::ReadableTable;
    let id_table = txn
        .open_table(ASSET_BY_ID)
        .context("Failed to open ASSET_BY_ID for member read")?;
    let meta_table = txn
        .open_table(METADATA_TABLE)
        .context("Failed to open METADATA_TABLE for member read")?;
    let Some(record_json) = id_table
        .get(asset_id)
        .with_context(|| format!("Failed to read ASSET_BY_ID row for {asset_id}"))?
    else {
        return Ok((0, 0, None));
    };
    let record: crate::model::asset::AssetRecord = serde_json::from_str(record_json.value())
        .with_context(|| format!("Failed to deserialize AssetRecord for {asset_id}"))?;
    let payload = meta_table
        .get(asset_id)
        .with_context(|| format!("Failed to read METADATA_TABLE row for {asset_id}"))?
        .map(|g| g.value());
    let ad = compose_abstract_data(&record, payload.as_ref());
    let ts = ad.compute_timestamp(ALBUM_PRIORITY_LIST);
    let size = match &ad {
        crate::model::abstract_data::AbstractData::Image(img) => img.metadata.size,
        crate::model::abstract_data::AbstractData::Video(vid) => vid.metadata.size,
        _ => 0,
    };
    let thumb = ad.thumbhash().cloned();
    Ok((ts, size, thumb))
}

#[allow(warnings)]
fn recompute_time_range(
    txn: &redb::WriteTransaction,
    album_id: &str,
) -> anyhow::Result<(Option<i64>, Option<i64>)> {
    let members = list_album_members(txn, album_id)?;
    if members.is_empty() {
        return Ok((None, None));
    }
    let mut min = i64::MAX;
    let mut max = i64::MIN;
    for mid in members {
        let (ts, _, _) = member_info(txn, &mid)?;
        min = min.min(ts);
        max = max.max(ts);
    }
    Ok((Some(min), Some(max)))
}

#[allow(warnings)]
fn recompute_cover_from_members(
    txn: &redb::WriteTransaction,
    album_id: &str,
) -> anyhow::Result<(Option<ArrayString<64>>, Option<Vec<u8>>)> {
    let members = list_album_members(txn, album_id)?;
    if members.is_empty() {
        return Ok((None, None));
    }
    let mut best: Option<(i64, ArrayString<64>, Option<Vec<u8>>)> = None;
    for mid in members {
        let (ts, _, th) = member_info(txn, &mid)?;
        let id: ArrayString<64> = mid
            .parse()
            .map_err(|_| anyhow::anyhow!("album member id exceeds 64 bytes: {mid}"))?;
        match &best {
            None => best = Some((ts, id, th)),
            Some((best_ts, _, _)) if ts > *best_ts => best = Some((ts, id, th)),
            _ => {}
        }
    }
    Ok(best
        .map(|(_, id, th)| (Some(id), th))
        .unwrap_or((None, None)))
}

#[allow(warnings)]
fn descendant_cover(
    txn: &redb::WriteTransaction,
    dir_path: &str,
) -> anyhow::Result<(Option<ArrayString<64>>, Option<Vec<u8>>)> {
    use crate::model::metadata_record::compose_abstract_data;
    use crate::storage::db::{ASSET_BY_ID, METADATA_TABLE};
    use redb::ReadableTable;
    let id_table = txn
        .open_table(ASSET_BY_ID)
        .context("Failed to open ASSET_BY_ID for descendant cover")?;
    let meta_table = txn
        .open_table(METADATA_TABLE)
        .context("Failed to open METADATA_TABLE for descendant cover")?;
    let mut best: Option<(i64, ArrayString<64>, Option<Vec<u8>>)> = None;
    for row in id_table.iter().context("Failed to scan ASSET_BY_ID")? {
        let (_, value) = row.context("Failed to read ASSET_BY_ID row")?;
        let record: crate::model::asset::AssetRecord = serde_json::from_str(value.value())
            .context("Failed to deserialize AssetRecord during descendant cover")?;
        if record.kind == crate::model::asset::AssetKind::Album {
            continue;
        }
        if record.is_trashed {
            continue;
        }
        if std::path::Path::new(&record.path)
            .file_name()
            .and_then(|n| n.to_str())
            == Some(".__picasu_ph__.jpg")
        {
            continue;
        }
        if !std::path::Path::new(&record.path).starts_with(dir_path) {
            continue;
        }
        let payload = meta_table
            .get(&*record.asset_id)
            .context("Failed to read descendant metadata row")?
            .map(|g| g.value());
        let ad = compose_abstract_data(&record, payload.as_ref());
        let ts = ad.compute_timestamp(ALBUM_PRIORITY_LIST);
        match &best {
            None => best = Some((ts, record.asset_id, ad.thumbhash().cloned())),
            Some((best_ts, _, _)) if ts > *best_ts => {
                best = Some((ts, record.asset_id, ad.thumbhash().cloned()))
            }
            _ => {}
        }
    }
    Ok(best
        .map(|(_, id, th)| (Some(id), th))
        .unwrap_or((None, None)))
}

#[allow(warnings)]
#[quint::operation("OP-8")]
pub fn apply_album_delta(
    txn: &redb::WriteTransaction,
    album_id: &str,
    delta: AlbumDelta,
) -> anyhow::Result<()> {
    use crate::model::abstract_data::AbstractData;
    use crate::model::metadata_record::{
        MetadataRecord, compose_abstract_data, to_metadata_record,
    };
    use crate::storage::db::{ASSET_BY_ID, METADATA_TABLE};
    use redb::ReadableTable;

    if let Some(member) = &delta.member {
        if delta.is_update {
            // Member row stays in place; nothing to re-key.
        } else if delta.is_add {
            add_album_member(txn, album_id, member.as_str())?;
        } else {
            remove_album_member(txn, album_id, member.as_str())?;
        }
    }

    let id_table = txn
        .open_table(ASSET_BY_ID)
        .context("Failed to open ASSET_BY_ID")?;
    let mut meta_table = txn
        .open_table(METADATA_TABLE)
        .context("Failed to open METADATA_TABLE")?;

    let record_json = id_table
        .get(album_id)
        .with_context(|| format!("Failed to read album record for {album_id}"))?;
    let record: Option<crate::model::asset::AssetRecord> = record_json
        .map(|g| {
            serde_json::from_str(g.value())
                .with_context(|| format!("Failed to deserialize album record for {album_id}"))
        })
        .transpose()?;
    let payload = meta_table
        .get(album_id)
        .with_context(|| format!("Failed to read album metadata for {album_id}"))?
        .map(|g| g.value());

    // Release the read handles before the helper functions reopen these tables
    // in the same transaction (redb allows one open handle per table per txn).
    drop(id_table);
    drop(meta_table);

    let (record, mut album) = match (record, payload.clone()) {
        (Some(r), Some(MetadataRecord::Album(_))) => {
            let ad = compose_abstract_data(&r, payload.as_ref());
            match ad {
                AbstractData::Album(a) => (r, a),
                _ => return Ok(()),
            }
        }
        _ => return Ok(()),
    };
    let _ = record;

    let mut size_delta = delta.size_delta;
    let member_ts = match delta.member_timestamp {
        Some(ts) => ts,
        None => match &delta.member {
            Some(member) => member_info(txn, member.as_str())?.0,
            None => 0,
        },
    };
    let member_size = match delta.member_size_override {
        Some(size) => size,
        None => match &delta.member {
            Some(member) => member_info(txn, member.as_str())?.1,
            None => 0,
        },
    };
    if size_delta == 0 && delta.member.is_some() {
        size_delta = if delta.is_add {
            member_size as i64
        } else {
            -(member_size as i64)
        };
    }

    if delta.count_delta != 0 {
        let new_count = (album.metadata.item_count as i64 + delta.count_delta).max(0) as usize;
        album.metadata.item_count = new_count;
    }
    if size_delta != 0 {
        if size_delta >= 0 {
            album.metadata.item_size = album.metadata.item_size.saturating_add(size_delta as u64);
        } else {
            album.metadata.item_size = album
                .metadata
                .item_size
                .saturating_sub((-size_delta) as u64);
        }
    }

    album.metadata.last_modified_time = chrono::Utc::now().timestamp_millis();

    if delta.is_update && delta.member.is_some() {
        // Uniform rule: any size/timestamp change recomputes time range + cover
        // from the member table (O(M)).
        let (s, e) = recompute_time_range(txn, album_id)?;
        album.metadata.start_time = s;
        album.metadata.end_time = e;
        let (cover, th) = recompute_cover_from_members(txn, album_id)?;
        album.metadata.cover = cover;
        album.object.thumbhash = th;
    } else if delta.is_add && delta.member.is_some() {
        match album.metadata.start_time {
            None => album.metadata.start_time = Some(member_ts),
            Some(s) if member_ts < s => album.metadata.start_time = Some(member_ts),
            _ => {}
        }
        match album.metadata.end_time {
            None => album.metadata.end_time = Some(member_ts),
            Some(e) if member_ts > e => album.metadata.end_time = Some(member_ts),
            _ => {}
        }
        // Cover = newest member by timestamp (unified rule).
        let m = delta.member.as_ref().unwrap();
        let current_cover_ts = match album.metadata.cover.as_ref() {
            Some(cover) => member_info(txn, cover.as_str())?.0,
            None => i64::MIN,
        };
        if album.metadata.cover.is_none() || member_ts > current_cover_ts {
            album.metadata.cover = Some(m.clone());
            let (_, _, th) = member_info(txn, m.as_str())?;
            album.object.thumbhash = th;
        }
    } else if !delta.is_add && delta.member.is_some() {
        let m = delta.member.as_ref().unwrap();
        let _ = member_ts;
        let _ = m;

        if album.metadata.item_count == 0 {
            album.metadata.start_time = None;
            album.metadata.end_time = None;
            album.metadata.item_size = 0;
            let (cover, th) = descendant_cover(txn, &album.metadata.dir_path)?;
            album.metadata.cover = cover;
            album.object.thumbhash = th;
        } else {
            let (s, e) = recompute_time_range(txn, album_id)?;
            album.metadata.start_time = s;
            album.metadata.end_time = e;
            let (cover, th) = recompute_cover_from_members(txn, album_id)?;
            album.metadata.cover = cover;
            album.object.thumbhash = th;
        }
    }

    let new_payload = to_metadata_record(&AbstractData::Album(album));
    let mut meta_table = txn
        .open_table(METADATA_TABLE)
        .context("Failed to reopen METADATA_TABLE for album write")?;
    meta_table
        .insert(album_id, new_payload)
        .context("Failed to write album metadata")?;

    Ok(())
}

/// Refresh an empty album's cover from its descendants (no-op when the album
/// has direct members). Used for ancestor albums after a membership change.
#[allow(warnings)]
pub fn refresh_empty_album_cover(
    txn: &redb::WriteTransaction,
    album_id: &str,
) -> anyhow::Result<()> {
    use crate::model::abstract_data::AbstractData;
    use crate::model::metadata_record::{
        MetadataRecord, compose_abstract_data, to_metadata_record,
    };
    use crate::storage::db::{ASSET_BY_ID, METADATA_TABLE};
    use redb::ReadableTable;

    let id_table = txn
        .open_table(ASSET_BY_ID)
        .context("Failed to open ASSET_BY_ID")?;
    let meta_table = txn
        .open_table(METADATA_TABLE)
        .context("Failed to open METADATA_TABLE")?;

    let record_json = id_table
        .get(album_id)
        .with_context(|| format!("Failed to read album record for {album_id}"))?;
    let record: Option<crate::model::asset::AssetRecord> = record_json
        .map(|g| {
            serde_json::from_str(g.value())
                .with_context(|| format!("Failed to deserialize album record for {album_id}"))
        })
        .transpose()?;
    let payload = meta_table
        .get(album_id)
        .with_context(|| format!("Failed to read album metadata for {album_id}"))?
        .map(|g| g.value());
    let (record, mut album) = match (record, payload.clone()) {
        (Some(r), Some(MetadataRecord::Album(_))) => {
            let ad = compose_abstract_data(&r, payload.as_ref());
            match ad {
                AbstractData::Album(a) => (r, a),
                _ => return Ok(()),
            }
        }
        _ => return Ok(()),
    };
    let _ = record;
    if album.metadata.item_count != 0 {
        return Ok(());
    }
    drop(id_table);
    drop(meta_table);
    let (cover, th) = descendant_cover(txn, &album.metadata.dir_path)?;
    if cover != album.metadata.cover {
        album.metadata.cover = cover;
        album.object.thumbhash = th;
        let mut meta_table = txn
            .open_table(METADATA_TABLE)
            .context("Failed to reopen METADATA_TABLE")?;
        let new_payload = to_metadata_record(&AbstractData::Album(album));
        meta_table
            .insert(album_id, new_payload)
            .context("Failed to write album metadata")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::asset::{AssetKind, AssetRecord};
    use crate::tests::bootstrap::*;
    use redb::{ReadableTable, TableDefinition};

    fn ensure_tables() {
        let _ = &*TEST_ENV;
        let txn = TREE
            .in_disk
            .begin_write()
            .expect("begin write txn for table creation");
        txn.open_table(ASSET_BY_PATH).expect("create ASSET_BY_PATH");
        txn.open_table(ASSET_BY_ID).expect("create ASSET_BY_ID");
        txn.open_table(DUPE_INDEX).expect("create DUPE_INDEX");
        txn.commit().expect("commit table creation");
    }

    fn clear_tables() {
        clear_one_table(ASSET_BY_PATH);
        clear_one_table(ASSET_BY_ID);
        clear_one_table(DUPE_INDEX);
    }

    fn clear_one_table(table_def: TableDefinition<'static, &'static str, &'static str>) {
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

    #[test]
    fn roundtrip_asset_by_path() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ensure_tables();
        clear_tables();

        let record = AssetRecord::new_media(
            AssetKind::Image,
            "/test/path.jpg".into(),
            Some("abc123".parse().expect("test content hash parses")),
            1024,
            "jpg".into(),
            0,
        );

        put_asset_by_path(&record.path, record.asset_id).expect("put asset path mapping");
        let found = get_asset_id_by_path(&record.path).expect("read asset id by path");
        assert_eq!(found, Some(record.asset_id));

        clear_tables();
    }

    #[test]
    fn move_asset_mapping_rewrites_all_tables_in_one_txn() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ensure_tables();
        clear_tables();

        let record = AssetRecord::new_media(
            AssetKind::Image,
            "/old/path.jpg".into(),
            Some("abc".parse().expect("test content hash parses")),
            100,
            "jpg".into(),
            0,
        );
        let asset_id = record.asset_id;
        insert_asset(&record).expect("insert test asset");

        let mut updated = record.clone();
        updated.path = "/new/path.jpg".into();

        move_asset_mapping("/old/path.jpg", "/new/path.jpg", &updated).expect("move asset mapping");

        // Old mapping gone.
        assert!(
            get_asset_id_by_path("/old/path.jpg")
                .expect("read old path")
                .is_none(),
            "old path mapping must be removed"
        );
        // New mapping present and points at the asset id.
        assert_eq!(
            get_asset_id_by_path("/new/path.jpg").expect("read new path"),
            Some(asset_id),
            "new path mapping must carry the asset id"
        );
        // Record updated.
        let found = get_asset_by_id(asset_id.as_ref())
            .expect("read asset by id")
            .expect("asset present");
        assert_eq!(found.path, "/new/path.jpg", "record path must be rewritten");

        clear_tables();
    }

    #[test]
    fn roundtrip_asset_by_id() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ensure_tables();
        clear_tables();

        let record = AssetRecord::new_media(
            AssetKind::Image,
            "/test.jpg".into(),
            Some("hash123".parse().expect("test content hash parses")),
            500,
            "jpg".into(),
            0,
        );

        put_asset_by_id(&record).expect("put asset id mapping");
        let found = get_asset_by_id(record.asset_id.as_ref()).expect("read asset by id");
        assert!(found.is_some());
        let found = found.expect("asset present right after put");
        assert_eq!(found.asset_id, record.asset_id);
        assert_eq!(found.path, "/test.jpg");
        assert_eq!(found.kind, AssetKind::Image);

        clear_tables();
    }

    #[test]
    fn dupe_group_insert_and_query() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ensure_tables();
        clear_tables();

        let id1: ArrayString<64> = "asset001".parse().expect("test asset id parses");
        let id2: ArrayString<64> = "asset002".parse().expect("test asset id parses");
        let hash = "deadbeef";

        add_to_dupe_group(hash, id1).expect("add asset to dupe group");
        add_to_dupe_group(hash, id2).expect("add asset to dupe group");

        let ids = get_dupe_ids(hash).expect("read dupe group");
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));

        clear_tables();
    }

    #[test]
    fn dupe_group_remove_one() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ensure_tables();
        clear_tables();

        let id1: ArrayString<64> = "asset001".parse().expect("test asset id parses");
        let id2: ArrayString<64> = "asset002".parse().expect("test asset id parses");
        let hash = "deadbeef";

        add_to_dupe_group(hash, id1).expect("add asset to dupe group");
        add_to_dupe_group(hash, id2).expect("add asset to dupe group");
        remove_from_dupe_group(hash, id1).expect("remove asset from dupe group");

        let ids = get_dupe_ids(hash).expect("read dupe group");
        assert_eq!(ids.len(), 1);
        assert!(ids.contains(&id2));
        assert!(!ids.contains(&id1));

        clear_tables();
    }

    #[test]
    fn dupe_group_remove_last_deletes_group() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ensure_tables();
        clear_tables();

        let id: ArrayString<64> = "asset001".parse().expect("test asset id parses");
        let hash = "deadbeef";

        add_to_dupe_group(hash, id).expect("add asset to dupe group");
        remove_from_dupe_group(hash, id).expect("remove asset from dupe group");

        let ids = get_dupe_ids(hash).expect("read dupe group");
        assert!(ids.is_empty(), "removed group reads empty");

        // Verify the key is actually removed from the table.
        let txn = TREE.in_disk.begin_read().expect("begin read txn in test");
        let table = txn.open_table(DUPE_INDEX).expect("open dupe index in test");
        assert!(table.get(hash).expect("read dupe entry").is_none());

        clear_tables();
    }

    #[test]
    fn insert_asset_creates_all_mappings() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ensure_tables();
        clear_tables();

        let record = AssetRecord::new_media(
            AssetKind::Image,
            "/photo.jpg".into(),
            Some("abc".parse().expect("test content hash parses")),
            100,
            "jpg".into(),
            0,
        );
        let asset_id = record.asset_id;

        insert_asset(&record).expect("insert test asset");

        // Path mapping exists.
        assert_eq!(
            get_asset_id_by_path("/photo.jpg").expect("read asset id by photo path"),
            Some(asset_id)
        );

        // ID mapping exists.
        let found = get_asset_by_id(asset_id.as_ref()).expect("read asset by id");
        assert!(found.is_some());

        // Dupe group contains this asset.
        let ids = get_dupe_ids("abc").expect("read dupe group for test hash");
        assert!(ids.contains(&asset_id));

        clear_tables();
    }

    #[test]
    fn remove_asset_cleans_up_all_mappings() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ensure_tables();
        clear_tables();

        let record = AssetRecord::new_media(
            AssetKind::Image,
            "/photo.jpg".into(),
            Some("abc".parse().expect("test content hash parses")),
            100,
            "jpg".into(),
            0,
        );
        let asset_id = record.asset_id;

        insert_asset(&record).expect("insert test asset");
        remove_asset(&record).expect("remove test asset");

        // Path mapping is gone.
        assert_eq!(
            get_asset_id_by_path("/photo.jpg").expect("read asset id by photo path"),
            None
        );

        // ID mapping is gone.
        assert!(
            get_asset_by_id(asset_id.as_ref())
                .expect("read asset by id")
                .is_none()
        );

        // Dupe group is gone.
        assert!(
            get_dupe_ids("abc")
                .expect("read dupe group for test hash")
                .is_empty(),
            "removed group reads empty"
        );

        clear_tables();
    }

    #[test]
    fn album_asset_has_no_dupe_group_entry() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ensure_tables();
        clear_tables();

        let record = AssetRecord::new_album("/photos/vacation".into());

        insert_asset(&record).expect("insert test asset");

        // Path and ID mappings exist.
        assert!(
            get_asset_id_by_path("/photos/vacation")
                .expect("read asset id by album path")
                .is_some()
        );
        assert!(
            get_asset_by_id(record.asset_id.as_ref())
                .expect("read asset by id")
                .is_some()
        );

        // No dupe group (albums have no content hash).
        // This should not panic or error.
        let ids = get_dupe_ids("nonexistent_hash").expect("missing hash reads as empty");
        assert!(ids.is_empty(), "unknown hash reads as an empty group");

        clear_tables();
    }

    /// Integration test: verifies the new tables initialize correctly and that
    /// Redb transactions provide the expected isolation and durability semantics.
    #[test]
    fn schema_initialization_and_transaction_wrapper() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;

        // 1. Schema initialization: opening the tables must not error.
        let txn = TREE
            .in_disk
            .begin_write()
            .expect("begin_write must succeed for schema init");
        txn.open_table(ASSET_BY_PATH)
            .expect("ASSET_BY_PATH table creation");
        txn.open_table(ASSET_BY_ID)
            .expect("ASSET_BY_ID table creation");
        txn.open_table(DUPE_INDEX)
            .expect("DUPE_INDEX table creation");
        txn.commit().expect("commit must succeed after schema init");

        // 2. Write transaction durability: committed data is visible to
        //    subsequent read transactions.
        let txn = TREE.in_disk.begin_write().expect("begin_write");
        {
            let mut table = txn.open_table(ASSET_BY_PATH).expect("open ASSET_BY_PATH");
            table
                .insert("/durability_test.jpg", "id_durability")
                .expect("insert");
        }
        txn.commit().expect("commit write");

        let txn = TREE.in_disk.begin_read().expect("begin_read");
        {
            let table = txn
                .open_table(ASSET_BY_PATH)
                .expect("open ASSET_BY_PATH for read");
            let guard = table.get("/durability_test.jpg").expect("get");
            assert!(
                guard.is_some(),
                "committed data must be visible to read txn"
            );
            let val = guard.expect("committed row visible to read txn");
            assert_eq!(val.value(), "id_durability");
        }

        // 3. Cleanup
        let txn = TREE.in_disk.begin_write().expect("begin_write cleanup");
        {
            let mut table = txn.open_table(ASSET_BY_PATH).expect("open for cleanup");
            table.remove("/durability_test.jpg").expect("remove");
        }
        txn.commit().expect("commit cleanup");
    }

    #[test]
    fn list_asset_paths_under_filters_by_root() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ensure_tables();
        clear_tables();

        let id_root: ArrayString<64> = "id_root".parse().expect("id parses");
        let id_child: ArrayString<64> = "id_child".parse().expect("id parses");
        let id_sibling: ArrayString<64> = "id_sibling".parse().expect("id parses");

        put_asset_by_path("/root", id_root).expect("put root");
        put_asset_by_path("/root/child.jpg", id_child).expect("put child");
        put_asset_by_path("/root_sibling/other.jpg", id_sibling).expect("put sibling");

        let under = list_asset_paths_under("/root").expect("list under root");
        let paths: Vec<String> = under.into_iter().map(|(p, _)| p).collect();
        assert!(paths.contains(&"/root".to_string()));
        assert!(paths.contains(&"/root/child.jpg".to_string()));
        assert!(!paths.contains(&"/root_sibling/other.jpg".to_string()));

        clear_tables();
    }

    #[test]
    fn list_album_assets_under_filters_by_root_and_kind() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ensure_tables();
        clear_tables();

        let album_under = AssetRecord::new_album("/root/album".into());
        let album_outside = AssetRecord::new_album("/other/album".into());
        let media_under = AssetRecord::new_media(
            AssetKind::Image,
            "/root/photo.jpg".into(),
            None,
            10,
            "jpg".into(),
            0,
        );

        insert_asset(&album_under).expect("insert album under");
        insert_asset(&album_outside).expect("insert album outside");
        insert_asset(&media_under).expect("insert media under");

        let under = list_album_assets_under("/root").expect("list album under root");
        let paths: Vec<String> = under.into_iter().map(|(p, _)| p).collect();
        assert!(paths.contains(&"/root/album".to_string()));
        assert!(!paths.contains(&"/other/album".to_string()));
        assert!(!paths.contains(&"/root/photo.jpg".to_string()));

        clear_tables();
    }
}
