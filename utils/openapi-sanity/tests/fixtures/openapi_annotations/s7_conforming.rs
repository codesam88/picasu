// S7 conforming counterpart: an explicitly public handler with `security(())`
// and no guards, plus a guarded handler whose `security(...)` matches.

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    security(()),
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
pub async fn public_widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    security(("cookie_jwt" = [])),
    extensions(("x-picasu-auth" = json!({
        "guards": ["GuardAuth"],
        "tokenTyp": ["admin"],
        "admin": "admin",
        "shareScope": "n-a",
        "readonly": false,
        "reauth": false,
        "public": false,
        "effects": [],
        "invariants": []
    }))),
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget2")]
pub async fn guarded_widget(auth: GuardAuth) -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
