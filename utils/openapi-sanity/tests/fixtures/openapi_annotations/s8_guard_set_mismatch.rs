// S8 violation: the extension names a guard set the handler does not carry.
// Every static rule reads the signature, so without S8 this array could drift
// silently while the document still looks consistent.

/// Fetch one widget.
///
/// The widget endpoint requires an admin session.
#[utoipa::path(
    tag = "assets",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardUser"],
        "credentials": ["session"],
        "admin": "admin",
        "shareScope": "n/a",
        "effects": [],
        "invariants": []
    }))),
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn widget(auth: GuardResult<GuardAdmin>) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
