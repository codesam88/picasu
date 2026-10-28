// S2 conforming counterpart: `GuardShare` implies the two-group declaration,
// and `GuardReadOnlyMode` contributes no scheme, so it stays excluded.

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    security(("album_id" = [], "share_id" = []), ("cookie_jwt" = [])),
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn widget(
    share: GuardShare,
    _read_only: GuardReadOnlyMode,
) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
