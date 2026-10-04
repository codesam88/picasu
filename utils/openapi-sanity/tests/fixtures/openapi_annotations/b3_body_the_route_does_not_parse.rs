// A B3 fixture: a declared request body that is not the type the route's
// `data = "…"` binds.
//
// utoipa takes the declared schema and never compares it to what Rocket parses,
// so each handler here advertises a body the route will reject every time:
//
// - `edit_flags` declares `EditRatingData`, the route parses `EditFlagsData`;
// - `import_config` declares `AppConfig`, the route parses `ConfigImport`.
//
// The two shapes that must not be reported are in the conforming counterpart, so
// a rule that compared too eagerly fails `b3_body_the_route_does_not_parse`'s
// counterpart rather than passing quietly.

/// Edit flags.
#[utoipa::path(
        tag = "assets",
        request_body = EditRatingData,
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[put("/put/edit_flags", format = "json", data = "<json_data>")]
pub fn edit_flags(json_data: Json<EditFlagsData>) -> AppResult<Json<()>> {
    let _ = json_data;
    Ok(Json(()))
}

/// Import a configuration.
#[utoipa::path(
        tag = "config",
        request_body = AppConfig,
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[post("/post/config/import", data = "<file>")]
pub fn import_config(file: Json<ConfigImport>) -> AppResult<Status> {
    let _ = file;
    Ok(Status::Ok)
}