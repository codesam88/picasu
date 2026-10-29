// S9 violation: the declared role constraint disagrees with the guards. The
// admin field is what the matrix renders and what the negative matrix asserts,
// so a stale value misdescribes the route while every other rule stays silent.

/// Fetch one widget.
///
/// The widget endpoint requires an admin session.
#[utoipa::path(
    tag = "assets",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardAdmin"],
        "credentials": ["session"],
        "admin": "any",
        "shareScope": "n/a",
        "effects": ["read"],
        "invariants": []
    }))),
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn widget(auth: GuardResult<GuardAdmin>) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
