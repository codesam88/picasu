// S7 violation: a handler with no contributing guard whose `security(...)`
// is not the explicitly-public `security(())`.

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": [],
        "tokenTyp": [],
        "admin": "n-a",
        "shareScope": "n-a",
        "readonly": false,
        "reauth": false,
        "public": true,
        "effects": [],
        "invariants": []
    }))),
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
