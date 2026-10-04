// P1 conforming counterpart: the declared success is the status the handler
// returns.

type AppResult<T> = Result<T, AppError>;

/// Start indexing.
#[utoipa::path(
    tag = "index",
    responses((status = 202, description = "Indexing started"))
)]
#[post("/post/index/start")]
pub fn start_index() -> AppResult<Status> {
    Ok(Status::Accepted)
}
