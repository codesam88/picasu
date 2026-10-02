// A3's exemption fixture, with the control it needs to mean anything.
//
// The two exempt handlers serve routes under the prefixes in
// `EXCLUDED_ROUTE_PREFIXES` — the same set as the backend's
// `CONTRACT_EXCLUSION_PREFIXES` — so their operations are stripped from the
// published document and no tag can file them. The third handler is the control:
// its route is not under a prefix, so the missing tag is still a finding. A
// fixture with only the first two would pass if the exemption had swallowed the
// whole rule.

/// Test-only probe: report one record.
#[utoipa::path(responses((status = 200, description = "Probe record")))]
#[get("/get/test/record/<asset_id>")]
pub async fn probe_record() -> AppResult<Json<Record>> {
    Ok(Json(Record))
}

/// Serve a file from the static mount.
#[utoipa::path(responses((status = 200, description = "Static file")))]
#[get("/assets/index.html")]
pub async fn static_file() -> AppResult<FrontendResponse> {
    Ok(FrontendResponse)
}

/// Fetch one widget.
#[utoipa::path(responses((status = 200, description = "Ok")))]
#[get("/get/widget")]
pub async fn outside_the_excluded_prefixes() -> AppResult<Json<Widget>> {
    Ok(Json(Widget))
}