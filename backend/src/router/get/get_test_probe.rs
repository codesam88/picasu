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
use crate::storage::db::{DATA_TABLE, TREE};

/// Full alias list for a single record. Only reachable in test builds when the
/// bootstrap opts in; the regular API trims alias lists, so API E2E scenarios
/// use this endpoint to observe the raw stored paths instead.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestRecordProbe {
    #[schema(value_type = String)]
    pub hash: ArrayString<64>,
    pub aliases: Vec<FileModify>,
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
        .open_table(DATA_TABLE)
        .or_raise(|| (ErrorKind::Database, "Failed to open DATA_TABLE"))?;

    // Look up by asset_id only — no hash fallback.
    let abstract_data = table
        .get(&*asset_id)
        .or_raise(|| (ErrorKind::Database, "Failed to read record from DATA_TABLE"))?
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Record not found"))?
        .value();

    Ok(Json(TestRecordProbe {
        hash: asset_id,
        aliases: abstract_data.alias().to_vec(),
    }))
}
