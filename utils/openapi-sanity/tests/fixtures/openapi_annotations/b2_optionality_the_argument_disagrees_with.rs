// A B2 fixture: declared parameters whose documented `required` disagrees with
// the handler argument of the same name.
//
// utoipa 5.5 has no `required` key in a parameter tuple, so the documented
// `required` is derived from the declared type alone. Both directions of the
// disagreement are here, because they mislead in opposite ways:
//
// - `page` is declared `String` and bound `String` — conforming, so a rule that
//   flagged every declared parameter would fail here;
// - `limit` is declared `Option<u64>` and bound `u64`: the document tells a
//   client it may omit the query parameter, and the route rejects the request
//   without it;
// - `locate` is declared `String` and bound `Option<String>`: the document tells
//   a client it must send a query parameter the route is happy without.

/// List assets with a declared optionality the route disagrees with.
#[utoipa::path(
        tag = "assets",
        params(
            ("page" = String, Query, description = "Page number to read"),
            ("limit" = Option<u64>, Query, description = "Rows to return"),
            ("locate" = String, Query, description = "Timeline position to start at"),
        ),
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[get("/get/get-rows?<page>&<limit>&<locate>")]
pub fn get_rows(page: String, limit: u64, locate: Option<String>) -> AppResult<Json<Rows>> {
    let _ = (page, limit, locate);
    Ok(Json(Rows::default()))
}