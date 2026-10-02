// A B4 fixture: form routes whose annotation does not name multipart/form-data.
//
// Both spellings of the defect are here. `upload` declares `request_body =
// Value`, which utoipa documents as application/json — the shape both real form
// routes have today. `regenerate` declares no request body at all, so the
// document says nothing about the body it takes.
//
// `set_album_cover` binds a `Json<…>` and declares the type it parses: a media
// type finding about it would be the rule overreaching into non-form routes.

/// Upload files.
#[utoipa::path(
        tag = "upload",
        request_body = Value,
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[post("/upload", data = "<form>")]
pub fn upload(form: Result<Form<UploadForm<'_>>, Errors<'_>>) -> AppResult<()> {
    let _ = form;
    Ok(())
}

/// Regenerate a thumbnail from an uploaded frame.
#[utoipa::path(
        tag = "assets",
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[put("/put/regenerate-thumbnail-with-frame", data = "<form>")]
pub fn regenerate_thumbnail(form: Result<Form<RegenerateThumbnailForm<'_>>, Errors<'_>>) -> AppResult<()> {
    let _ = form;
    Ok(())
}

/// Set the album cover.
#[utoipa::path(
        tag = "albums",
        request_body = SetAlbumCover,
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[put("/put/set_album_cover", data = "<set_album_cover>")]
pub fn set_album_cover(set_album_cover: Json<SetAlbumCover>) -> AppResult<()> {
    let _ = set_album_cover;
    Ok(())
}