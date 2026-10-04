// B3 fixture: a custom schema name ending in `Value` is still a concrete type,
// not serde_json::Value's unconstrained-body spelling.

/// Read a concrete body.
#[utoipa::path(
    tag = "assets",
    request_body = ExpectedValue,
    responses((status = 200, description = "Ok"))
)]
#[post("/post/body", data = "<body>")]
pub fn body(body: Json<Actual>) -> AppResult<Json<()>> {
    let _ = body;
    Ok(Json(()))
}
