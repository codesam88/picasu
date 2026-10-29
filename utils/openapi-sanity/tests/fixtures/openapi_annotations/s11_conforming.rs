// S11 conforming counterpart: an exemption written down with its reason, on an
// operation that is held to the rule and does not satisfy it otherwise.
// This mirrors the read-only lift.

/// Lifting the kill-switch must work while read-only mode is on.
#[utoipa::path(
    tag = "config",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardAdmin"],
        "credentials": ["session"],
        "admin": "admin",
        "shareScope": "n/a",
        "public": false,
        "effects": ["manage"],
        "invariants": ["AUTH-11", "AUTH-12"],
        "waivers": {
            "AUTH-12": "the lift must stay callable while read-only mode is on"
        }
    }))),
    responses((status = 200, description = "Ok"))
)]
#[post("/post/control-widget")]
pub async fn control_widget(auth: GuardResult<GuardAdmin>) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
