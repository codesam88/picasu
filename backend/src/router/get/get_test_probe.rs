// src/router/get/get_test_probe.rs
use arrayvec::ArrayString;
use redb::ReadableDatabase;
use rocket::get;
use rocket::serde::json::Json;
use serde::Serialize;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::response::FileModify;
use crate::router::auth::GuardAuth;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::{DUPE_INDEX, METADATA_TABLE, TREE};

/// The record's stored alias (0 or 1 entries: path-primary records hold a
/// single path, `None` when pruned renders as an empty list). Only reachable
/// in test builds when the bootstrap opts in; API E2E scenarios use this
/// endpoint to observe the raw stored path instead.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestRecordProbe {
    #[schema(value_type = String)]
    pub hash: ArrayString<64>,
    pub aliases: Vec<FileModify>,
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
            (status = 200, description = "Test-only record probe with the full alias list", body = TestRecordProbe),
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
        .open_table(METADATA_TABLE)
        .or_raise(|| (ErrorKind::Database, "Failed to open METADATA_TABLE"))?;

    // Look up by asset_id only — no hash fallback.
    let abstract_data = table
        .get(&*asset_id)
        .or_raise(|| {
            (
                ErrorKind::Database,
                "Failed to read record from METADATA_TABLE",
            )
        })?
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Record not found"))?
        .value();

    Ok(Json(TestRecordProbe {
        hash: asset_id,
        aliases: abstract_data.alias().into_iter().cloned().collect(),
    }))
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
