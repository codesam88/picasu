// A8 fixture: guard bindings named after a guard class other than the one they
// bind.
//
// - `share_as_auth` binds `GuardResult<GuardShare>` under the name of the token
//   guard, so this tool's own C1 findings — which name the guard by its type,
//   because the name is what A8 is checking — read as though they were about a
//   token.
// - `mode` binds `GuardResult<GuardReadOnlyMode>` under a name that drops the
//   class's `_mode`, which is the naming the plan lists as a finding.
// - `discarded_timestamp` binds `GuardResult<GuardTimestamp>` with the tree's
//   discarded-value spelling on a guard the body **propagates**: the underscore
//   says the value is thrown away, which is the opposite of what the `?` does.
//
// Every handler documents the status its guard rejects with, so C3 stays silent
// and the findings below are A8's alone.

/// Read one widget behind a share token.
#[utoipa::path(
    tag = "auth",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, response = Unauthorized),
    )
)]
#[get("/get/a8-share-as-auth")]
pub fn share_as_auth(auth: GuardResult<GuardShare>) -> AppResult<Json<Widget>> {
    let _ = auth?;
    Ok(Json(Widget))
}

/// Rename a widget, refused while the build is read-only.
#[utoipa::path(
    tag = "assets",
    responses(
        (status = 200, description = "Ok"),
        (status = 405, description = "Read-only mode"),
    )
)]
#[put("/put/a8-short-mode-name")]
pub fn rename(mode: GuardResult<GuardReadOnlyMode>) -> AppResult<Status> {
    let _ = mode?;
    Ok(Status::Ok)
}

/// Read one widget behind a timestamp token.
#[utoipa::path(
    tag = "auth",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, response = Unauthorized),
    )
)]
#[get("/get/a8-underscore-on-propagated")]
pub fn discarded_timestamp(_timestamp: GuardResult<GuardTimestamp>) -> AppResult<Json<Widget>> {
    let claims = _timestamp?;
    Ok(Json(Widget::from(claims)))
}