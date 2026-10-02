// A4 conforming counterpart: a doc comment in any shape satisfies the rule. A4
// asks for *a* comment; A5 is what asks for the right one.

/// Fetch one widget.
///
/// A longer second paragraph, which utoipa derives `description` from, is
/// exactly what A4 wants and exactly what A5 says nothing about.
#[utoipa::path(
    tag = "assets",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn documented_widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}