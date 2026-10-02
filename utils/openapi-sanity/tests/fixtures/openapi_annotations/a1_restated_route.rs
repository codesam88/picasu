// A1 positive fixture: the annotation restates what the route attribute already
// says. `rocket_extras` derives the path and the verb from `#[get(…)]`, so both
// restatements are second copies of a fact nothing compares.

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    path = "/get/widget",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn restated_path() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Create one widget.
#[utoipa::path(
    get,
    tag = "assets",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget-created")]
pub async fn restated_verb() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

// `trace` is in the list because utoipa's `HttpMethod` accepts it as a bare
// token, even though no Rocket route uses it. A restatement nobody can write is
// still worth rejecting: the gap is the cost, not the rule.
/// Serve one widget.
#[utoipa::path(
    trace,
    tag = "assets",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget-traced")]
pub async fn restated_trace_verb() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}