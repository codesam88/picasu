// S6 violation: the signature carries `GuardReauth`, but the doc comment does
// not name the `X-Picasu-Password` header, so the re-authentication
// requirement is undocumented.

/// Update the widget.
#[utoipa::path(
    tag = "assets",
    responses((status = 200, description = "Ok"))
)]
#[put("/put/widget")]
pub fn update_widget(_reauth: GuardReauth) -> AppResult<()> {
    Ok(())
}
