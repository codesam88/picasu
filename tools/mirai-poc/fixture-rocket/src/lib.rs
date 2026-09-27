//! Plan step 4, part 3: the same handler shapes as `../fixture-handler`, but
//! routed through the real Rocket: the `#[get]`/`#[post]` attribute, Rocket's
//! `Json<T>` data guard, and a real `tokio::task::spawn_blocking` closure.
//!
//! The point of this crate is the macro expansion and the framework plumbing,
//! not the panics themselves. Each case is the corresponding `h*` case from
//! `../fixture-handler` with the Rocket attribute added, so the two fixtures can
//! be compared case by case.
//!
//! Two known limits of the shape, both inherited rather than tested here, are
//! noted in the cases that hit them: `Option::expect` is not a MIRAI sink, and
//! an `async fn` body is not analyzed.
//!
//! Nothing in this crate runs a server. Rocket only has to type-check the
//! routes, which is what a route handler's signature has to satisfy anyway.

use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use rocket::serde::json::Json;
use rocket::{get, post};

// ---------------------------------------------------------------- guard

/// Stand-in for `crate::router::auth::GuardTimestamp`. The backend implements
/// `FromRequest` with `#[rocket::async_trait]`, and so does this: without the
/// attribute the impl does not match Rocket's trait.
pub struct GuardTimestamp;

#[rocket::async_trait]
impl<'r> FromRequest<'r> for GuardTimestamp {
    type Error = Status;

    async fn from_request(_req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        Outcome::Success(GuardTimestamp)
    }
}

// -------------------------------------------------------------- data types

/// Body of `EditRatingData`: `rating` is the optional request primitive and
/// `index_array` the request primitive list that `edit_rating` iterates.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct EditRatingData {
    pub timestamp: i64,
    pub index_array: Vec<usize>,
    pub rating: Option<u8>,
}

#[derive(Debug, serde::Serialize)]
pub struct ScrollBarData {
    pub year: i32,
    pub month: u32,
}

// ------------------------------------------------------- private storage API

/// Stand-in for the `TREE_SNAPSHOT.read_scrollbar` /
/// `TREE_SNAPSHOT.read_row` methods in `backend/src/storage/cache.rs`.
pub struct TreeSnapshot;

impl TreeSnapshot {
    pub fn read_scrollbar(&'static self, timestamp: i64) -> Result<Vec<ScrollBarData>, Status> {
        if timestamp < 0 {
            return Err(Status::BadRequest);
        }
        Ok(vec![ScrollBarData {
            year: 2026,
            month: 9,
        }])
    }

    pub fn read_row(&'static self, index: usize, timestamp: i64) -> Result<Vec<u8>, Status> {
        if index == usize::MAX {
            return Err(Status::InternalServerError);
        }
        Ok(vec![timestamp as u8, index as u8])
    }
}

pub static TREE_SNAPSHOT: TreeSnapshot = TreeSnapshot;

/// Stand-in for the validating conversion that `read_scrollbar` uses on stored
/// dates, and the panic sink the plan's step 5 is written around.
fn to_millis(date: i64) -> Option<i64> {
    if date < 0 {
        None
    } else {
        Some(date)
    }
}

fn row_sink(id: Option<u32>) -> u32 {
    id.unwrap()
}

// ---------------------------------------------------- 1: `#[get]` + query

/// `#[get]` on a synchronous handler with a query primitive, reaching a private
/// helper and `Option::unwrap`. The `get_scroll_bar` shape without the guard.
#[get("/get/get-rows-primitive?<timestamp>")]
pub fn r1_get_primitive_to_helper_unwrap(timestamp: i64) -> Result<Json<Vec<u8>>, Status> {
    Ok(Json(vec![to_millis(timestamp).unwrap() as u8]))
}

/// The full `get_scroll_bar` shape: the `#[get]` attribute, a guard propagated
/// with `let _ = auth?;`, and the singleton call behind `Result::unwrap`.
#[get("/get/get-scroll-bar?<timestamp>")]
pub fn r2_get_guard_propagated_result_unwrap(
    auth: Result<GuardTimestamp, Status>,
    timestamp: i64,
) -> Result<Json<Vec<ScrollBarData>>, Status> {
    let _ = auth?;
    let data = TREE_SNAPSHOT.read_scrollbar(timestamp).unwrap();
    Ok(Json(data))
}

/// The same route with `Option::expect` as the sink, which is what plan step 5
/// looks for in the backend.
#[get("/get/get-scroll-bar-expect?<timestamp>")]
pub fn r3_get_guard_propagated_expect(
    auth: Result<GuardTimestamp, Status>,
    timestamp: i64,
) -> Result<Json<Vec<ScrollBarData>>, Status> {
    let _ = auth?;
    let data = TREE_SNAPSHOT
        .read_scrollbar(timestamp)
        .expect("snapshot must exist");
    Ok(Json(data))
}

// ------------------------------------------------------ 2: `#[post]` + Json

/// Rocket's `Json<T>` data guard, field read through `Deref`.
#[post("/put/edit_rating", format = "json", data = "<json_data>")]
pub fn r4_post_json_body_unwrap(json_data: Json<EditRatingData>) -> Result<Json<u8>, Status> {
    Ok(Json(json_data.rating.unwrap()))
}

/// Rocket's `Json<T>` data guard driving an index sink.
#[post("/put/edit_rating_index", format = "json", data = "<json_data>")]
pub fn r5_post_json_body_index(json_data: Json<EditRatingData>) -> Result<Json<usize>, Status> {
    Ok(Json(json_data.index_array[0]))
}

// --------------------------------------- 3: `#[get]` async + spawn_blocking

/// The `get_rows` shape: an asynchronous route, both request primitives moved
/// into a `tokio::task::spawn_blocking` closure, and the sink in a private
/// helper called by that closure.
#[get("/get/get-rows?<index>&<timestamp>")]
pub async fn r6_get_async_spawn_blocking_helper(
    auth: Result<GuardTimestamp, Status>,
    index: usize,
    timestamp: i64,
) -> Result<Json<Vec<u8>>, Status> {
    let _ = auth?;
    let rows = TREE_SNAPSHOT
        .read_row(index, timestamp)
        .map_err(|status| status)?;
    Ok(Json(rows))
}

/// An asynchronous route with a guard, and a `move` closure handed to
/// `tokio::task::spawn_blocking` that panics through a private helper.
#[get("/get/get-rows-closure?<id>")]
pub async fn r7_get_async_spawn_blocking_closure(
    auth: Result<GuardTimestamp, Status>,
    id: Option<u32>,
) -> Result<Json<u32>, Status> {
    let _ = auth?;
    let value = tokio::task::spawn_blocking(move || row_sink(id))
        .await
        .map_err(|_| Status::InternalServerError)?;
    Ok(Json(value))
}
