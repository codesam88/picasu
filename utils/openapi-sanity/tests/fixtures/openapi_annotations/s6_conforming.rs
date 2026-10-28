// S6 conforming counterpart: `GuardReauth` is in the signature and the doc
// comment names the `X-Picasu-Password` header, so the re-authentication
// requirement is documented.

/// Update the widget.
///
/// Requires the `X-Picasu-Password` header.
#[utoipa::path(
    tag = "assets",
    responses((status = 200, description = "Ok"))
)]
#[put("/put/widget")]
pub fn update_widget(_reauth: GuardReauth) -> AppResult<()> {
    Ok(())
}
