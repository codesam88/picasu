// Fixed-tree fixture: one POST handler with two inline query/path
// parameters and one JSON body — the declaration shapes B1, B2 and B3 read.
// Shapes mirror the conforming B1/B3 fixtures.

/// Create a widget for an owner.
#[utoipa::path(
        tag = "assets",
        params(
            ("owner" = String, Path, description = "Owner to attribute"),
            ("rank" = Option<u32>, Query, description = "Rank to assign"),
        ),
        request_body = CreateWidgetData,
        responses(
            (status = 200, description = "Created"),
        )
    )
]
#[post("/post/widgets/<owner>?<rank>", format = "json", data = "<json_data>")]
pub fn create_widget(json_data: Json<CreateWidgetData>) -> AppResult<String> {
    let _ = json_data;
    Ok(String::new())
}
