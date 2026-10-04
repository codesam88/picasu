// P4: this handler is infallible, its route binds neither a body nor a query,
// and no guard or body kind can produce 400 — the declared 400 is a lie the
// document would tell every client.

/// Index status.
#[utoipa::path(
    tag = "index",
    responses(
        (status = 200, description = "Ok"),
        (status = 400, description = "Invalid input"),
    )
)]
#[get("/get/index/status")]
pub fn index_status() -> Json<IndexStatus> {
    Json(IndexStatus)
}
