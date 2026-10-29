// S5 violation: the signature carries `GuardMutate`, but `responses(...)`
// does not declare 405, so the read-only rejection is undocumented.

/// Update the widget.
#[utoipa::path(
    tag = "assets",
    responses((status = 200, description = "Ok"))
)]
#[put("/put/widget")]
pub fn update_widget(_read_only: GuardMutate) -> AppResult<()> {
    Ok(())
}
