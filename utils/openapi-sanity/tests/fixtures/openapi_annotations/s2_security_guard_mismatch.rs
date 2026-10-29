// S2 violation: `security(...)` declares a scheme that does not match the
// guard in the signature — `GuardAdmin` implies `cookie_jwt`, not
// `bearer_token`.

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    security(("bearer_token" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardAdmin"],
        "credentials": ["admin"],
        "admin": "admin",
        "shareScope": "n-a",
        "effects": [],
        "invariants": []
    }))),
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn widget(auth: GuardAdmin) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
