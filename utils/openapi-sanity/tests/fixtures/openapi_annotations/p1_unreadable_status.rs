// P1: the success is a Status constant this tool cannot map to a code, which is
// an explicit unsupported-syntax finding rather than a silent pass. P4 stays
// quiet while the success is unreadable rather than piling on.

/// Serve a teapot.
#[utoipa::path(
    tag = "pages",
    responses((status = 200, description = "Ok"))
)]
#[get("/teapot")]
pub fn teapot() -> Status {
    Status::Teapot
}
