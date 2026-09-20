// src/router/get/get_test_probe.rs
use arrayvec::ArrayString;
use redb::ReadableDatabase;
use rocket::get;
use rocket::serde::json::Json;
use serde::Serialize;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{AppError, ErrorKind, OptionExt, ResultExt};
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
        path = "/get/test/record/{hash}",
        responses(
            (status = 200, description = "Test-only record probe with the full alias list", body = TestRecordProbe),
            (status = 400, description = "Invalid hash"),
            (status = 404, description = "Probe disabled or record not found"),
        )
    )
]
#[get("/get/test/record/<hash>")]
pub fn probe_record(auth: GuardResult<GuardAuth>, hash: &str) -> AppResult<Json<TestRecordProbe>> {
    let _ = auth?;

    if !probe_enabled() {
        return Err(AppError::new(ErrorKind::NotFound, "Not Found"));
    }

    let hash = ArrayString::<64>::from(hash).map_err(|_| {
        AppError::new(
            ErrorKind::InvalidInput,
            format!("Invalid record hash: {hash}"),
        )
    })?;

    let txn = TREE
        .in_disk
        .begin_read()
        .or_raise(|| (ErrorKind::Database, "Failed to begin read transaction"))?;
    let table = txn
        .open_table(DATA_TABLE)
        .or_raise(|| (ErrorKind::Database, "Failed to open DATA_TABLE"))?;

    // Try by asset_id first, then fall back to hash lookup.
    let abstract_data = if let Ok(Some(record)) = table.get(hash.as_str()) {
        record.value()
    } else {
        // Hash might be a content hash — resolve via DUPE_INDEX.
        let asset_id = crate::storage::asset_store::get_dupe_ids(hash.as_str())
            .ok()
            .and_then(|ids| ids.into_iter().next());
        match asset_id {
            Some(aid) => table
                .get(&*aid)
                .or_raise(|| (ErrorKind::Database, "Failed to read record from DATA_TABLE"))?
                .or_raise(|| (ErrorKind::NotFound, "Record not found"))?
                .value(),
            None => return Err(AppError::new(ErrorKind::NotFound, "Record not found")),
        }
    };

    Ok(Json(TestRecordProbe {
        hash,
        aliases: abstract_data.alias().to_vec(),
    }))
}
