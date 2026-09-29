//! The `/get/test/` probe registration gate, observed from outside `cfg(test)`.
//!
//! Integration tests link the `picasu` library compiled *without* `cfg(test)`,
//! which is the only vantage point from which the gate is visible: in-crate
//! tests run with `cfg(test)` and see the gated route table. A non-test build
//! must not mount the probe routes at all — their paths are absent from the
//! route table instead of reaching an inert handler.
//!
//! A request-level observation was tried and does not discriminate: Rocket 0.5
//! answers a wrong-method request on a known path with 404, not 405, so
//! `POST /get/test/...` returns 404 whether or not the route is registered.

use picasu::{AppConfig, build_rocket_with_config};

/// The mounted GET routes of a non-test build, as Rocket reports them.
fn non_test_get_routes() -> Vec<String> {
    let rocket = build_rocket_with_config(AppConfig::default());
    rocket
        .routes()
        .map(|route| route.uri.path().to_string())
        .collect()
}

/// The gate: no `/get/test/` path may be registered outside test builds.
#[test]
fn non_test_route_table_mounts_no_probe() {
    let mounted = non_test_get_routes();

    // Guard against the empty-table false pass: the observation is only
    // meaningful if the production routes are there.
    assert!(
        mounted.iter().any(|path| path == "/get/config"),
        "expected the production GET routes to be mounted; got: {mounted:?}"
    );

    let probes: Vec<&str> = mounted
        .iter()
        .map(String::as_str)
        .filter(|path| path.starts_with("/get/test/"))
        .collect();

    assert!(
        probes.is_empty(),
        "a non-test build mounts the test probes: {probes:?} — their registration \
         in `generate_get_routes()` must be gated behind `#[cfg(test)]`"
    );
}
