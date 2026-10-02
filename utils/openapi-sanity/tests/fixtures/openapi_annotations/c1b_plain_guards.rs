// C1b fixture: plain Rocket request guards. Rocket runs each of them during
// request handling and short-circuits on failure, so the handler body is free
// to ignore the value entirely — which is what the tree does in five handlers.

/// Fetch one widget, ignoring the guards Rocket has already run.
#[utoipa::path(
    tag = "auth",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, description = "Unauthorized"),
        (status = 405, description = "Read-only mode"),
    )
)]
#[get("/get/unused-plain-guard")]
pub async fn unused_plain_guard(
    _auth: GuardAuth,
    _read_only_mode: GuardReadOnlyMode,
) -> AppResult<()> {
    Ok(())
}

// The tree's spelling: an unused plain guard binds `_auth`, so rustc does not
// even warn. It must not produce a finding either.
/// Fetch one widget, binding the guard as `_auth` as the tree does.
#[utoipa::path(
    tag = "auth",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, description = "Unauthorized"),
    )
)]
#[get("/get/underscore-plain-guard")]
pub async fn underscore_plain_guard(_auth: GuardAuth) -> AppResult<()> {
    Ok(())
}

// Mixed: the one fallible guard propagates, the plain guard is ignored.
/// Fetch one widget, propagating the fallible guard and ignoring the plain one.
#[utoipa::path(
    tag = "auth",
    responses(
        (status = 200, description = "Ok"),
        (status = 401, description = "Unauthorized"),
    )
)]
#[get("/get/mixed-guards")]
pub async fn mixed_guards(
    auth: GuardResult<GuardAuth>,
    share: GuardShare,
) -> AppResult<Json<Claims>> {
    let claims = auth?;
    Ok(Json(claims))
}