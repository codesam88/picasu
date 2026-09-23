use anyhow::{Context, Result};
use arrayvec::ArrayString;
use redb::ReadableDatabase;

use crate::model::asset::AssetRecord;
use crate::storage::db::{ASSET_BY_ID, ASSET_BY_PATH, DUPE_INDEX, TREE};

// ── Asset by path ────────────────────────────────────────────────────────────

/// Look up an asset ID by its canonical path.
pub fn get_asset_id_by_path(canonical_path: &str) -> Result<Option<ArrayString<64>>> {
    let txn = TREE
        .in_disk
        .begin_read()
        .context("Failed to begin read transaction for ASSET_BY_PATH")?;
    let table = txn
        .open_table(ASSET_BY_PATH)
        .context("Failed to open ASSET_BY_PATH")?;

    match table.get(canonical_path)? {
        Some(guard) => {
            let id_str = guard.value();
            let id = ArrayString::from(id_str)
                .map_err(|_| anyhow::anyhow!("Asset ID exceeds 64 bytes: {id_str}"))?;
            Ok(Some(id))
        }
        None => Ok(None),
    }
}

/// Insert or update a `path → asset_id` mapping.
pub fn put_asset_by_path(canonical_path: &str, asset_id: ArrayString<64>) -> Result<()> {
    let txn = TREE
        .in_disk
        .begin_write()
        .context("Failed to begin write transaction for ASSET_BY_PATH")?;
    {
        let mut table = txn
            .open_table(ASSET_BY_PATH)
            .context("Failed to open ASSET_BY_PATH for write")?;
        table
            .insert(canonical_path, &*asset_id)
            .context("Failed to insert into ASSET_BY_PATH")?;
    }
    txn.commit()
        .context("Failed to commit ASSET_BY_PATH insert")
}

/// Remove a `path → asset_id` mapping.
pub fn remove_asset_by_path(canonical_path: &str) -> Result<()> {
    let txn = TREE
        .in_disk
        .begin_write()
        .context("Failed to begin write transaction for ASSET_BY_PATH")?;
    {
        let mut table = txn
            .open_table(ASSET_BY_PATH)
            .context("Failed to open ASSET_BY_PATH for write")?;
        table
            .remove(canonical_path)
            .context("Failed to remove from ASSET_BY_PATH")?;
    }
    txn.commit()
        .context("Failed to commit ASSET_BY_PATH remove")
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

/// Find all asset records whose `canonical_path` is a descendant of `dir_path`.
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
        .filter(|r| r.canonical_path.starts_with(&prefix))
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
pub fn insert_asset(record: &AssetRecord) -> Result<()> {
    put_asset_by_id(record)?;
    put_asset_by_path(&record.canonical_path, record.asset_id)?;
    if let Some(hash) = &record.content_hash {
        add_to_dupe_group(hash, record.asset_id)?;
    }
    Ok(())
}

/// Remove a complete asset (record + path mapping + dupe group entry).
/// This is the primary entry point for removing an asset from the stores.
pub fn remove_asset(record: &AssetRecord) -> Result<()> {
    remove_asset_by_id(&record.asset_id)?;
    remove_asset_by_path(&record.canonical_path)?;
    if let Some(hash) = &record.content_hash {
        remove_from_dupe_group(hash, record.asset_id)?;
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
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        ensure_tables();
        clear_tables();

        let record = AssetRecord::new_media(
            AssetKind::Image,
            "/test/path.jpg".into(),
            Some("abc123".parse().unwrap()),
            1024,
            "jpg".into(),
            0,
        );

        put_asset_by_path(&record.canonical_path, record.asset_id).unwrap();
        let found = get_asset_id_by_path(&record.canonical_path).unwrap();
        assert_eq!(found, Some(record.asset_id));

        clear_tables();
    }

    #[test]
    fn roundtrip_asset_by_id() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        ensure_tables();
        clear_tables();

        let record = AssetRecord::new_media(
            AssetKind::Image,
            "/test.jpg".into(),
            Some("hash123".parse().unwrap()),
            500,
            "jpg".into(),
            0,
        );

        put_asset_by_id(&record).unwrap();
        let found = get_asset_by_id(&record.asset_id.to_string()).unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.asset_id, record.asset_id);
        assert_eq!(found.canonical_path, "/test.jpg");
        assert_eq!(found.kind, AssetKind::Image);

        clear_tables();
    }

    #[test]
    fn dupe_group_insert_and_query() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        ensure_tables();
        clear_tables();

        let id1: ArrayString<64> = "asset001".parse().unwrap();
        let id2: ArrayString<64> = "asset002".parse().unwrap();
        let hash = "deadbeef";

        add_to_dupe_group(hash, id1).unwrap();
        add_to_dupe_group(hash, id2).unwrap();

        let ids = get_dupe_ids(hash).unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));

        clear_tables();
    }

    #[test]
    fn dupe_group_remove_one() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        ensure_tables();
        clear_tables();

        let id1: ArrayString<64> = "asset001".parse().unwrap();
        let id2: ArrayString<64> = "asset002".parse().unwrap();
        let hash = "deadbeef";

        add_to_dupe_group(hash, id1).unwrap();
        add_to_dupe_group(hash, id2).unwrap();
        remove_from_dupe_group(hash, id1).unwrap();

        let ids = get_dupe_ids(hash).unwrap();
        assert_eq!(ids.len(), 1);
        assert!(ids.contains(&id2));
        assert!(!ids.contains(&id1));

        clear_tables();
    }

    #[test]
    fn dupe_group_remove_last_deletes_group() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        ensure_tables();
        clear_tables();

        let id: ArrayString<64> = "asset001".parse().unwrap();
        let hash = "deadbeef";

        add_to_dupe_group(hash, id).unwrap();
        remove_from_dupe_group(hash, id).unwrap();

        let ids = get_dupe_ids(hash).unwrap();
        assert!(ids.is_empty());

        // Verify the key is actually removed from the table.
        let txn = TREE.in_disk.begin_read().unwrap();
        let table = txn.open_table(DUPE_INDEX).unwrap();
        assert!(table.get(hash).unwrap().is_none());

        clear_tables();
    }

    #[test]
    fn insert_asset_creates_all_mappings() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        ensure_tables();
        clear_tables();

        let record = AssetRecord::new_media(
            AssetKind::Image,
            "/photo.jpg".into(),
            Some("abc".parse().unwrap()),
            100,
            "jpg".into(),
            0,
        );
        let asset_id = record.asset_id;

        insert_asset(&record).unwrap();

        // Path mapping exists.
        assert_eq!(get_asset_id_by_path("/photo.jpg").unwrap(), Some(asset_id));

        // ID mapping exists.
        let found = get_asset_by_id(&asset_id.to_string()).unwrap();
        assert!(found.is_some());

        // Dupe group contains this asset.
        let ids = get_dupe_ids("abc").unwrap();
        assert!(ids.contains(&asset_id));

        clear_tables();
    }

    #[test]
    fn remove_asset_cleans_up_all_mappings() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        ensure_tables();
        clear_tables();

        let record = AssetRecord::new_media(
            AssetKind::Image,
            "/photo.jpg".into(),
            Some("abc".parse().unwrap()),
            100,
            "jpg".into(),
            0,
        );
        let asset_id = record.asset_id;

        insert_asset(&record).unwrap();
        remove_asset(&record).unwrap();

        // Path mapping is gone.
        assert_eq!(get_asset_id_by_path("/photo.jpg").unwrap(), None);

        // ID mapping is gone.
        assert!(get_asset_by_id(&asset_id.to_string()).unwrap().is_none());

        // Dupe group is gone.
        assert!(get_dupe_ids("abc").unwrap().is_empty());

        clear_tables();
    }

    #[test]
    fn album_asset_has_no_dupe_group_entry() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        ensure_tables();
        clear_tables();

        let record = AssetRecord::new_album("/photos/vacation".into());

        insert_asset(&record).unwrap();

        // Path and ID mappings exist.
        assert!(get_asset_id_by_path("/photos/vacation").unwrap().is_some());
        assert!(
            get_asset_by_id(&record.asset_id.to_string())
                .unwrap()
                .is_some()
        );

        // No dupe group (albums have no content hash).
        // This should not panic or error.
        let ids = get_dupe_ids("nonexistent_hash").unwrap();
        assert!(ids.is_empty());

        clear_tables();
    }

    /// Integration test: verifies the new tables initialize correctly and that
    /// Redb transactions provide the expected isolation and durability semantics.
    #[test]
    fn schema_initialization_and_transaction_wrapper() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
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
            let val = guard.unwrap();
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
}
