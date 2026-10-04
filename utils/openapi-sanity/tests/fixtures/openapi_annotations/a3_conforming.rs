// A3 conforming counterpart: every tag of the vocabulary is accepted, and one tag
// is the count. The list is asserted against the vocabulary constant in
// `every_tag_of_the_vocabulary_is_accepted`, so a tag added there without a
// fixture here fails rather than going unchecked.

/// Fetch one widget.
#[utoipa::path(
    tag = "assets",
    responses((status = 200, description = "Ok"))
)]
#[get("/get/widget")]
pub async fn widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}

/// Sign in.
#[utoipa::path(
    tag = "auth",
    responses((status = 200, description = "Token", body = String))
)]
#[post("/post/authenticate")]
pub async fn authenticate() -> AppResult<Json<String>> {
    Ok(Json(String::new()))
}