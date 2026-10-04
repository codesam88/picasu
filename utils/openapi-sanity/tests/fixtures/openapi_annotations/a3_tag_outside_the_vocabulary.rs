// A3 positive fixture: the three ways a tag fails the house rule. `data` is the
// plausible mistake — it names the route prefix rather than a subject in the
// vocabulary.

/// Fetch one widget.
#[utoipa::path(responses((status = 200, description = "Ok")))]
#[get("/get/widget")]
pub async fn no_tag() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Fetch one widget.
#[utoipa::path(
    tag = "data",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget-named-outside-the-vocabulary")]
pub async fn unknown_tag() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    tag = "timeline",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget-tagged-twice")]
pub async fn two_tags() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}