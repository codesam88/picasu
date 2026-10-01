// C1b fixture: plain Rocket request guards. Rocket runs each of them during
// request handling and short-circuits on failure, so the handler body is free
// to ignore the value entirely — which is what the tree does in five handlers.

#[utoipa::path(
    tag = "data",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/unused-plain-guard")]
pub async fn unused_plain_guard(auth: GuardAuth, mode: GuardReadOnlyMode) -> AppResult<()> {
    Ok(())
}

// The tree's spelling: an unused plain guard binds `_auth`, so rustc does not
// even warn. It must not produce a finding either.
#[utoipa::path(
    tag = "data",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/underscore-plain-guard")]
pub async fn underscore_plain_guard(_auth: GuardAuth) -> AppResult<()> {
    Ok(())
}

// Mixed: the one fallible guard propagates, the plain guard is ignored.
#[utoipa::path(
    tag = "data",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/mixed-guards")]
pub async fn mixed_guards(
    auth: GuardResult<GuardAuth>,
    share: GuardShare,
) -> AppResult<Json<Claims>> {
    let claims = auth?;
    Ok(Json(claims))
}