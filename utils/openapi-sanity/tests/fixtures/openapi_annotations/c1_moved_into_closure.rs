// Ambiguity case 1, pinned as a decision rather than left to the reader.
//
// A fallible guard moved into a closure and propagated inside it. The walker
// recurses through `Visit`, so the `auth?` inside the closure body is seen and
// the binding counts as propagated. `guard_moved_into_a_closure_is_accepted`
// pins that, with this file as the evidence.

/// Fetch one widget on a blocking task.
#[utoipa::path(
    tag = "auth",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/guarded-in-closure")]
pub async fn guarded_in_closure(auth: GuardResult<GuardAuth>) -> AppResult<Json<Claims>> {
    let claims = tokio::task::spawn_blocking(move || -> Result<Claims, AppError> {
        let claims = auth?;
        Ok(claims)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))?;
    Ok(Json(claims))
}