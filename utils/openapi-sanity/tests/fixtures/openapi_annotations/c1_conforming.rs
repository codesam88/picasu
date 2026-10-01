// C1 conforming counterpart: every position the rule recognises as propagating
// the rejection. Silence here is what keeps the rule from being satisfied by
// flagging every guard argument in the tree.

#[utoipa::path(
    tag = "data",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/propagated")]
pub async fn house_idiom(auth: GuardResult<GuardAuth>) -> AppResult<()> {
    let _ = auth?;
    Ok(())
}

#[utoipa::path(
    tag = "data",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/bound")]
pub async fn bound_before_use(auth: GuardResult<GuardAuth>) -> AppResult<Json<Claims>> {
    let claims = auth?;
    Ok(Json(claims))
}

#[utoipa::path(
    tag = "data",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/matched")]
pub async fn matched(auth: GuardResult<GuardAuth>) -> AppResult<Json<Claims>> {
    let claims = match auth {
        Ok(claims) => claims,
        Err(error) => return Err(error),
    };
    Ok(Json(claims))
}

#[utoipa::path(
    tag = "data",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/if-let")]
pub async fn if_let(auth: GuardResult<GuardAuth>) -> AppResult<Json<Claims>> {
    if let Ok(claims) = auth {
        return Ok(Json(claims));
    }
    Err(AppError::new(ErrorKind::Auth, "Unauthorized"))
}

#[utoipa::path(
    tag = "data",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/forwarded")]
pub async fn forwarded(auth: GuardResult<GuardAuth>) -> AppResult<Json<Claims>> {
    audit(auth)
}

#[utoipa::path(
    tag = "data",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/returned")]
pub async fn returned(auth: GuardResult<GuardAuth>) -> AppResult<Json<Claims>> {
    return audit(auth);
}

#[utoipa::path(
    tag = "data",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/tail")]
pub async fn tail(auth: GuardResult<GuardAuth>) -> GuardResult<Json<Claims>> {
    audit(auth)
}