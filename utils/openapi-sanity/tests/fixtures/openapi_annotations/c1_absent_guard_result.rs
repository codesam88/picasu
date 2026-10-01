// C1 positive fixture: the binding never reaches the body at all. Nothing
// tells rustc, because dropping an unused argument is not an error.

#[utoipa::path(
    tag = "data",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/unused-guard-result")]
pub async fn unused_guard_result(auth: GuardResult<GuardAuth>) -> AppResult<()> {
    Ok(())
}