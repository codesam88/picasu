// A7 conforming counterpart: the doc comment says all of it, and the annotation
// says nothing it could repeat. The per-response `description` is a different
// key in a different place — it is how a status code's text is written, and
// nothing derives it — so it is the one `description` this rule leaves alone.

/// Move the widget into the album's directory on disk.
///
/// The album must be a directory album; a manual album answers 400. The
/// conflict outcome is reported rather than assumed, so the caller is never
/// silent about what happened to the widget.
#[utoipa::path(
    tag = "albums",
    responses(
        (status = 200, description = "Item assigned to album"),
        (status = 400, description = "Invalid input or item not found"),
    )
)]
#[put("/put/assign_album")]
pub async fn derived_prose() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

type AppResult<T> = Result<T, AppError>;