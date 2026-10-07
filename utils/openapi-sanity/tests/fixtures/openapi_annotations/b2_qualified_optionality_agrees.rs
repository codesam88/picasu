// B2 fixture: a fully-qualified std::option::Option has the same optionality as
// the unqualified Option spelling, in either the declaration or the argument.

/// Read optional query values.
#[utoipa::path(
    tag = "assets",
    params(
        ("declared_qualified" = std::option::Option<u64>, Query, description = "Declared with a qualified Option"),
        ("argument_qualified" = Option<u64>, Query, description = "Declared with a bare Option"),
    ),
    responses((status = 200, description = "Ok"))
)]
#[get("/get/scroll-bar?<declared_qualified>&<argument_qualified>")]
pub fn get_scroll_bar(
    declared_qualified: Option<u64>,
    argument_qualified: std::option::Option<u64>,
) -> AppResult<Json<ScrollBar>> {
    let _ = (declared_qualified, argument_qualified);
    Ok(Json(ScrollBar::default()))
}
