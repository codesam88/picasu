// Ambiguity case 2, pinned as a decision rather than left to the reader.
//
// A fallible guard rebound one hop: `auth` itself is only ever used on the
// right-hand side of a `let`, and the `?` that propagates the rejection is
// applied to the new name. The recognised propagating positions all require the
// binding itself to appear as `ident?`, as a scrutinee, as a call argument or as
// a returned value, so this is `Use::Discarded` — a finding. Whether that is the
// behaviour we want is a review decision, not something to settle by editing the
// rule; `rebound_guard_result_is_reported` pins what the rule does today.

/// Fetch one widget, rebinding the guard first.
#[utoipa::path(
    tag = "auth",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/rebound-guard-result")]
pub async fn rebound_guard_result(auth: GuardResult<GuardAuth>) -> AppResult<Json<Claims>> {
    let carried = auth;
    let claims = carried?;
    Ok(Json(claims))
}