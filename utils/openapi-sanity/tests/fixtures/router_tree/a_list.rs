// Fixed-tree fixture: two plain GET handlers, no parameters, no bodies.
// Shapes mirror the conforming A2/B1 fixtures, so the tree scans clean and
// the coverage test asserts a known inventory instead of live counts.

/// List all widgets.
#[utoipa::path(
        tag = "assets",
        responses(
            (status = 200, description = "Widgets"),
        )
    )
]
#[get("/get/widgets")]
pub fn list_widgets() -> AppResult<Json<Vec<Widget>>> {
    Ok(Json(Vec::new()))
}

/// Show one widget.
#[utoipa::path(
        tag = "assets",
        responses(
            (status = 200, description = "Widget"),
        )
    )
]
#[get("/get/widget")]
pub fn show_widget() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}
