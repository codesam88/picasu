// A7 positive fixture: the same prose twice. utoipa derives `summary` from the
// doc comment's first paragraph and `description` from the rest, so a hand-set
// pair is a second copy of the comment standing next to it — and A5's premise,
// that the first paragraph *is* the summary, stops holding while it is there.

/// Move the widget into the album's directory on disk.
#[utoipa::path(
    tag = "albums",
    summary = "Move a widget into an album",
    description = "Moves the widget into the album directory on disk.",
    responses((status = 200, description = "Ok"))
)]
#[put("/put/assign_album")]
pub async fn hand_set_prose() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}