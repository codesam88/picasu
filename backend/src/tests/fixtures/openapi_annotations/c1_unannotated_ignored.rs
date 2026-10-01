// Scope fixture: a route without a `#[utoipa::path]` annotation. Its dropped
// guard is not this rule's business — an undocumented route is a separate
// finding, reported by the contract tests, not by a body check.

#[get("/get/undocumented")]
pub async fn undocumented(auth: GuardResult<GuardAuth>) -> AppResult<()> {
    let _ = auth;
    Ok(())
}