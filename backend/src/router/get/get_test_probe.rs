// src/router/get/get_test_probe.rs
use arrayvec::ArrayString;
use redb::ReadableDatabase;
use rocket::get;
use rocket::serde::json::Json;
use serde::Serialize;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::asset::AssetKind;
use crate::model::response::FileEntry;
use crate::router::auth::GuardAuth;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::{ASSET_BY_ID, DUPE_INDEX, TREE};

/// Test-only record probe: the asset's identity (`assetId`) and its file
/// entry as composed from the `AssetRecord`, matching
/// `AbstractData::path() -> Option<FileEntry>`. `path` is `None` for
/// albums. Only reachable in test builds when the bootstrap opts in; API
/// E2E scenarios use this endpoint to observe the asset's path instead.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestRecordProbe {
    #[schema(value_type = String)]
    pub asset_id: ArrayString<64>,
    pub path: Option<FileEntry>,
}

/// One member of a `DUPE_INDEX` content-hash group. API scenario tests use
/// [`probe_dupe_group`] to observe hash-group membership, which is otherwise
/// invisible behind the HTTP surface.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DupeGroupMember {
    #[schema(value_type = String)]
    pub asset_id: ArrayString<64>,
}

/// Test-bootstrap opt-in flag. The probe is disabled (404) by default; only the
/// test bootstrap flips it on. Compiled in only under `cfg(test)`, so the probe
/// is unreachable in every production build and hidden behind an explicit
/// opt-in during `cargo test`.
#[cfg(test)]
static PROBE_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Enable the probe for the current process. Called by the test bootstrap.
#[cfg(test)]
pub fn enable_test_probe() {
    PROBE_REQUESTED.store(true, Ordering::SeqCst);
}

#[cfg(test)]
fn probe_enabled() -> bool {
    PROBE_REQUESTED.load(Ordering::SeqCst)
}

#[cfg(not(test))]
fn probe_enabled() -> bool {
    false
}

#[utoipa::path(
        get,
        path = "/get/test/record/{asset_id}",
        responses(
            (status = 200, description = "Test-only record probe with the asset's path", body = TestRecordProbe),
            (status = 400, description = "Invalid asset_id"),
            (status = 404, description = "Probe disabled or record not found"),
        )
    )
]
#[get("/get/test/record/<asset_id>")]
pub fn probe_record(
    auth: GuardResult<GuardAuth>,
    asset_id: &str,
) -> AppResult<Json<TestRecordProbe>> {
    let _ = auth?;

    if !probe_enabled() {
        return Err(AppError::new(ErrorKind::NotFound, "Not Found"));
    }

    let asset_id = ArrayString::<64>::from(asset_id).map_err(|_| {
        AppError::new(
            ErrorKind::InvalidInput,
            format!("Invalid asset_id: {asset_id}"),
        )
    })?;

    let txn = TREE
        .in_disk
        .begin_read()
        .or_raise(|| (ErrorKind::Database, "Failed to begin read transaction"))?;
    let table = txn
        .open_table(ASSET_BY_ID)
        .or_raise(|| (ErrorKind::Database, "Failed to open ASSET_BY_ID"))?;

    // Look up by asset_id only — no hash fallback. The file entry is
    // composed from the identity record; albums have no path.
    let record_json = table
        .get(&*asset_id)
        .or_raise(|| {
            (
                ErrorKind::Database,
                "Failed to read record from ASSET_BY_ID",
            )
        })?
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Record not found"))?;
    let record: crate::model::asset::AssetRecord = serde_json::from_str(record_json.value())
        .or_raise(|| (ErrorKind::Database, "Failed to deserialize AssetRecord"))?;

    let path = match record.kind {
        AssetKind::Album => None,
        AssetKind::Image | AssetKind::Video => Some(FileEntry {
            file: record.canonical_path.clone(),
            modified: record.modified,
            scan_time: record.scan_time,
            is_trashed: record.is_trashed,
        }),
    };

    Ok(Json(TestRecordProbe { asset_id, path }))
}

/// Test-only probe: list the `asset_id` members of a `DUPE_INDEX`
/// content-hash group. Returns an empty list when no group exists for `hash`,
/// so scenarios can assert both presence and absence of members. Disabled
/// (404) unless the test bootstrap opted in via `enable_test_probe`.
#[utoipa::path(
        get,
        path = "/get/test/dupe-group/{hash}",
        responses(
            (status = 200, description = "Test-only probe: members of a DUPE_INDEX content-hash group", body = Vec<DupeGroupMember>),
            (status = 404, description = "Probe disabled"),
        )
    )
]
#[get("/get/test/dupe-group/<hash>")]
pub fn probe_dupe_group(
    auth: GuardResult<GuardAuth>,
    hash: &str,
) -> AppResult<Json<Vec<DupeGroupMember>>> {
    let _ = auth?;

    if !probe_enabled() {
        return Err(AppError::new(ErrorKind::NotFound, "Not Found"));
    }

    let txn = TREE
        .in_disk
        .begin_read()
        .or_raise(|| (ErrorKind::Database, "Failed to begin read transaction"))?;
    let table = txn
        .open_table(DUPE_INDEX)
        .or_raise(|| (ErrorKind::Database, "Failed to open DUPE_INDEX"))?;

    let ids: Vec<String> = match table
        .get(hash)
        .or_raise(|| (ErrorKind::Database, "Failed to read DUPE_INDEX group"))?
    {
        Some(guard) => serde_json::from_str(guard.value()).unwrap_or_default(),
        None => Vec::new(),
    };

    Ok(Json(
        ids.iter()
            .filter_map(|id| ArrayString::<64>::from(id).ok())
            .map(|asset_id| DupeGroupMember { asset_id })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Path-primary probe contract: the test-only record probe exposes the
    /// asset's identity (`assetId`, formerly the misleadingly named `hash`)
    /// and its singular stored path entry — never a fake multi-alias list
    /// (`aliases`) and never hash-keyed identity.
    #[test]
    fn test_record_probe_schema_is_path_primary() {
        let spec: serde_json::Value = serde_json::from_str(&crate::openapi::generate_json())
            .expect("generated OpenAPI must be valid JSON");
        let schema = &spec["components"]["schemas"]["TestRecordProbe"];
        let properties = schema["properties"]
            .as_object()
            .expect("TestRecordProbe must declare properties");

        assert!(
            properties.contains_key("assetId"),
            "probe must expose assetId (the lookup key); got keys: {properties:?}"
        );
        assert!(
            properties.contains_key("path"),
            "probe must expose the singular stored path; got keys: {properties:?}"
        );
        assert!(
            !properties.contains_key("hash"),
            "hash must not appear; the field holds an asset ID, not a content hash"
        );
        assert!(
            !properties.contains_key("aliases"),
            "aliases must not appear; the storage model holds one path per asset"
        );

        let required: Vec<&str> = schema["required"]
            .as_array()
            .expect("TestRecordProbe must declare required fields")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(
            required.contains(&"assetId"),
            "assetId must be required; got {required:?}"
        );
        assert!(
            !required.contains(&"aliases"),
            "aliases must not be required; got {required:?}"
        );

        let description =
            spec["paths"]["/get/test/record/{asset_id}"]["get"]["responses"]["200"]["description"]
                .as_str()
                .unwrap_or_default();
        assert!(
            !description.contains("alias"),
            "probe response description must use path-primary wording: {description}"
        );
    }

    /// Wire-shape pin: the probe serializes as `assetId` + singular `path`
    /// (camelCase), with `path: null` when the record has no stored file
    /// entry — and never as `hash` / `aliases`.
    #[test]
    fn test_record_probe_serializes_asset_id_and_singular_path() {
        let probe = TestRecordProbe {
            asset_id: ArrayString::<64>::from("asset-123").expect("valid asset id"),
            path: Some(FileEntry {
                file: "/images/a/photo.jpg".to_string(),
                modified: 1,
                scan_time: 2,
                is_trashed: false,
            }),
        };
        let json = serde_json::to_value(&probe).expect("probe must serialize");
        assert_eq!(json["assetId"], "asset-123");
        assert_eq!(json["path"]["file"], "/images/a/photo.jpg");
        assert!(json.get("hash").is_none(), "hash must not be serialized");
        assert!(
            json.get("aliases").is_none(),
            "aliases must not be serialized"
        );

        let pruned = TestRecordProbe {
            asset_id: ArrayString::<64>::from("asset-456").expect("valid asset id"),
            path: None,
        };
        let json = serde_json::to_value(&pruned).expect("probe must serialize");
        assert!(
            json["path"].is_null(),
            "a record without a stored path serializes path as null: {json}"
        );
    }
}
