use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::album::ResolvedShare;
use crate::model::expression::{AlbumFilterValue, Expression};
use crate::model::response::DatabaseTimestamp;
use crate::model::response::Prefetch;
use crate::model::response::ReducedData;
use crate::router::AppResult;
use crate::router::GuardResult;
use crate::router::auth::ClaimsTimestamp;
use crate::router::auth::GuardShare;
use crate::storage::cache::QUERY_SNAPSHOT;
use crate::storage::cache::TREE_SNAPSHOT;
use crate::storage::db::TREE;
use crate::storage::db::VERSION_COUNT_TIMESTAMP;
use crate::tasks::BATCH_COORDINATOR;

use crate::tasks::batcher::flush_query_snapshot::FlushQuerySnapshotTask;
use crate::tasks::batcher::flush_tree_snapshot::FlushTreeSnapshotTask;

use anyhow::Result;
use bitcode::{Decode, Encode};
use chrono::Utc;
use log::info;
use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use std::hash::Hasher;
use std::hash::{DefaultHasher, Hash};
use std::mem;
use std::sync::atomic::Ordering;
use std::time::Instant;

#[derive(Debug, Clone, Deserialize, Serialize, Decode, Encode)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct PrefetchReturn {
    pub prefetch: Prefetch,
    pub token: String,
    pub resolved_share_opt: Option<ResolvedShare>,
}

impl PrefetchReturn {
    fn new(prefetch: Prefetch, token: String, resolved_share_opt: Option<ResolvedShare>) -> Self {
        Self {
            prefetch,
            token,
            resolved_share_opt,
        }
    }
}

// -----------------------------------------------------------------------------
// Convenience: &DatabaseTimestamp → ReducedData
// -----------------------------------------------------------------------------
impl From<&DatabaseTimestamp> for ReducedData {
    fn from(source: &DatabaseTimestamp) -> Self {
        Self {
            hash: source.abstract_data.hash(),
            width: source.abstract_data.width(),
            height: source.abstract_data.height(),
            date: source.timestamp,
        }
    }
}

// -----------------------------------------------------------------------------
// ── Helper functions for each step ──────────────────────────────────────────
// -----------------------------------------------------------------------------

fn check_query_cache(
    query_hash: u64,
    resolved_share_option: &mut Option<ResolvedShare>,
) -> Option<Json<PrefetchReturn>> {
    let find_cache_start_time = Instant::now();

    // Check cache first
    if let Ok(Some(prefetch)) = QUERY_SNAPSHOT.read_query_snapshot(query_hash) {
        let duration = format!("{:?}", find_cache_start_time.elapsed());
        info!(duration = &*duration; "Query cache found");
        let claims = ClaimsTimestamp::new(mem::take(resolved_share_option), prefetch.timestamp);
        return Some(Json(PrefetchReturn::new(
            prefetch,
            claims.encode(),
            claims.resolved_share_opt,
        )));
    }

    let duration = format!("{:?}", find_cache_start_time.elapsed());
    info!(duration = &*duration; "Query cache not found. Generate a new one.");
    None
}

fn filter_items(
    expression_option: Option<Expression>,
    resolved_share_option: Option<&ResolvedShare>,
) -> Result<Vec<ReducedData>, AppError> {
    let filter_items_start_time = Instant::now();

    let tree_guard = TREE.in_memory.read().map_err(|err| {
        AppError::new(
            ErrorKind::Internal,
            format!("Failed to read tree in memory: {err:?}"),
        )
    })?;
    let reduced_data_vector: Vec<ReducedData> = match (expression_option, resolved_share_option) {
        // If we have a resolved share then it must have a filter expression
        (Some(expr), Some(resolved_share)) => {
            let filter_fn = if resolved_share.share.show_metadata {
                expr.generate_filter()
            } else {
                expr.generate_filter_hide_metadata(resolved_share.album_id)
            };
            tree_guard
                .par_iter()
                .filter(|db_ts| filter_fn(&db_ts.abstract_data))
                .map(std::convert::Into::into)
                .collect()
        }
        (Some(expr), None) => {
            let filter_fn = expr.generate_filter();
            tree_guard
                .par_iter()
                .filter(|database_timestamp| filter_fn(&database_timestamp.abstract_data))
                .map(std::convert::Into::into)
                .collect()
        }
        (None, _) => tree_guard
            .par_iter()
            .map(std::convert::Into::into)
            .collect(),
    };

    let duration = format!("{:?}", filter_items_start_time.elapsed());
    info!(duration = &*duration; "Filter items");

    Ok(reduced_data_vector)
}

fn compute_locate(
    reduced_data_vector: &[ReducedData],
    locate_option: Option<&String>,
) -> Option<usize> {
    let layout_start_time = Instant::now();

    // Find locate index if requested
    let locate_to_index = locate_option.and_then(|hash| {
        reduced_data_vector
            .par_iter()
            .position_first(|reduced| reduced.hash.as_str() == hash)
    });

    let duration = format!("{:?}", layout_start_time.elapsed());
    info!(duration = &*duration; "Compute layout");

    locate_to_index
}

fn build_cache_key(expression_option: Option<&Expression>, locate_option: Option<&String>) -> u64 {
    let cache_key_start_time = Instant::now();

    let mut hasher = DefaultHasher::new();
    expression_option.hash(&mut hasher);
    VERSION_COUNT_TIMESTAMP
        .load(Ordering::Relaxed)
        .hash(&mut hasher);
    locate_option.hash(&mut hasher);
    let query_hash = hasher.finish();

    let duration = format!("{:?}", cache_key_start_time.elapsed());
    info!(duration = &*duration; "Build cache key");

    query_hash
}

fn insert_data_into_tree_snapshot(reduced_data_vector: Vec<ReducedData>) -> (i64, usize) {
    let db_start_time = Instant::now();

    // Persist to snapshot
    let timestamp_millis = Utc::now().timestamp_millis();
    let reduced_data_vector_length = reduced_data_vector.len();
    TREE_SNAPSHOT
        .in_memory
        .insert(timestamp_millis, reduced_data_vector);
    BATCH_COORDINATOR.execute_batch_detached(FlushTreeSnapshotTask);

    let duration = format!("{:?}", db_start_time.elapsed());
    info!(duration = &*duration; "Write cache into memory");

    (timestamp_millis, reduced_data_vector_length)
}

fn create_json_response(
    timestamp_millis: i64,
    locate_to_index: Option<usize>,
    reduced_data_vector_length: usize,
    query_hash: u64,
    resolved_share_option: Option<ResolvedShare>,
) -> Json<PrefetchReturn> {
    let json_start_time = Instant::now();

    let prefetch = Prefetch::new(
        timestamp_millis,
        locate_to_index,
        reduced_data_vector_length,
    );

    // Cache the result
    QUERY_SNAPSHOT.in_memory.insert(query_hash, prefetch);
    BATCH_COORDINATOR.execute_batch_detached(FlushQuerySnapshotTask);

    // Build response
    let claims = ClaimsTimestamp::new(resolved_share_option, timestamp_millis);
    let json = Json(PrefetchReturn::new(
        prefetch,
        claims.encode(),
        claims.resolved_share_opt,
    ));

    let duration = format!("{:?}", json_start_time.elapsed());
    info!(duration = &*duration; "Create JSON response");

    json
}

// -----------------------------------------------------------------------------
// ── Single prefetch function ─────────────────────────────────────────────────
// -----------------------------------------------------------------------------

fn execute_prefetch_logic(
    expression_option: Option<Expression>,
    locate_option: Option<&String>,
    mut resolved_share_option: Option<ResolvedShare>,
) -> Result<Json<PrefetchReturn>, AppError> {
    // Start timer
    let start_time = Instant::now();

    // Step 1: Build cache key for response creation
    let query_hash = build_cache_key(expression_option.as_ref(), locate_option);

    // Step 2: Check if query cache is available
    if let Some(cached_response) = check_query_cache(query_hash, &mut resolved_share_option) {
        return Ok(cached_response);
    }

    // Step 3: Filter items
    let reduced_data_vector = filter_items(expression_option, resolved_share_option.as_ref())?;

    // Step 4: Compute layout
    let locate_to_index = compute_locate(&reduced_data_vector, locate_option);

    // Step 6: Insert data into TREE_SNAPSHOT
    let (timestamp_millis, reduced_data_vector_length) =
        insert_data_into_tree_snapshot(reduced_data_vector);

    // Step 7: Create and return JSON response
    let json = create_json_response(
        timestamp_millis,
        locate_to_index,
        reduced_data_vector_length,
        query_hash,
        resolved_share_option,
    );

    // Total elapsed time
    let duration = format!("{:?}", start_time.elapsed());
    info!(duration = &*duration; "(total time) Get_data_length complete");

    Ok(json)
}

#[utoipa::path(
        post,
        path = "/get/prefetch",
        request_body = serde_json::Value,
        responses(
            (status = 200, description = "Prefetch result", body = PrefetchReturn),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[post("/get/prefetch?<locate>", format = "json", data = "<query_data>")]
pub async fn prefetch(
    auth_guard: GuardResult<GuardShare>,
    query_data: Option<Json<Expression>>,
    locate: Option<String>,
) -> AppResult<Json<PrefetchReturn>> {
    let auth_guard = auth_guard?;
    // Combine album filter (if any) with the client‑supplied query.
    let mut combined_expression_option = query_data.map(rocket::serde::json::Json::into_inner);
    let resolved_share_option = auth_guard.claims.get_share();

    if let Some(resolved_share) = &resolved_share_option {
        let album_filter_expression =
            Expression::Album(AlbumFilterValue::Value(resolved_share.album_id));

        combined_expression_option = Some(match combined_expression_option {
            Some(client_expression) => {
                Expression::And(vec![album_filter_expression, client_expression])
            }
            None => album_filter_expression,
        });
    }

    // Execute on blocking thread
    let job_handle = tokio::task::spawn_blocking(move || {
        execute_prefetch_logic(
            combined_expression_option,
            locate.as_ref(),
            resolved_share_option,
        )
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    Ok(job_handle)
}
