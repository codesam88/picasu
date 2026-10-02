// C1 positive fixture: the `84f29aa5` shape. The route binds a fallible guard,
// the handler drops the resulting `Result`, and the rejection is gone.

/// Fetch one widget, dropping the first guard on the way.
#[utoipa::path(
    tag = "auth",
    responses((status = 200, description = "Ok"), (status = 401, description = "Unauthorized"))
)]
#[get("/get/dropped")]
pub async fn dropped_guard_result(
    auth: GuardResult<GuardAuth>,
    timestamp: GuardResult<GuardTimestamp>,
) -> AppResult<()> {
    let _ = auth;
    let _ = timestamp?;
    Ok(())
}