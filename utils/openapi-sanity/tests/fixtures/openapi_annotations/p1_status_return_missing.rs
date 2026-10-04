// P1/P4: the handler's success is Status::Accepted; the annotation claims 200,
// so P1 misses 202 and P4 finds 200 impossible.

type AppResult<T> = Result<T, AppError>;

/// Start indexing.
#[utoipa::path(
    tag = "index",
    responses((status = 200, description = "Indexing started"))
)]
#[post("/post/index/start")]
pub fn start_index() -> AppResult<Status> {
    Ok(Status::Accepted)
}
