//! The public `OpenAPI` artifact: the generated spec with test-only surface removed.
//!
//! The committed reference (`docs/openapi-reference.md`) and `--dump-openapi`
//! must not advertise test-only endpoints. Probe contract tests keep using
//! [`crate::openapi::generate_json`], which returns the full spec.
//!
//! [`public_json`] is the checked-in `backend/openapi.json`: `just openapi-check`
//! regenerates it and fails when it differs, so any change to the public API
//! surface has to show up in a reviewable diff.

use crate::openapi::generate_json;

/// Path prefix of the test-only probe endpoints. Shared with the mounted-route
/// parity test so a probe cannot be documented in one place and hidden in the
/// other.
pub const TEST_ONLY_PATH_PREFIX: &str = "/get/test/";

/// Whether a path belongs to the test-only probe surface, which is mounted in
/// every build but only enabled by the test bootstrap.
#[must_use]
pub fn is_test_only_path(path: &str) -> bool {
    path.starts_with(TEST_ONLY_PATH_PREFIX)
}

/// Remove test-only probe endpoints (`/get/test/...`) and their schemas from
/// a serialized `OpenAPI` document.
pub fn strip_test_only_endpoints(spec: &mut serde_json::Value) {
    if let Some(paths) = spec.get_mut("paths").and_then(|p| p.as_object_mut()) {
        paths.retain(|key, _| !is_test_only_path(key));
    }
    if let Some(schemas) = spec
        .get_mut("components")
        .and_then(|c| c.get_mut("schemas"))
        .and_then(|s| s.as_object_mut())
    {
        schemas.retain(|key, _| key != "TestRecordProbe" && key != "DupeGroupMember");
    }
}

/// The spec for the committed reference and `--dump-openapi`: the generated
/// document minus test-only probes, pretty-printed so the checked-in artifact
/// diffs line by line.
///
/// Object keys are emitted in sorted order because `serde_json::Map` is
/// `BTreeMap`-backed unless the `preserve_order` feature is enabled; the
/// `committed_artifact_is_sorted` test fails if that ever changes.
///
/// # Panics
/// Panics if the generated document is not valid JSON.
#[must_use]
pub fn public_json() -> String {
    let mut spec: serde_json::Value =
        serde_json::from_str(&generate_json()).expect("generated OpenAPI must be valid JSON");
    strip_test_only_endpoints(&mut spec);
    let mut json =
        serde_json::to_string_pretty(&spec).expect("public OpenAPI serialization failed");
    json.push('\n');
    json
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_test_only_endpoints_removes_probes_only() {
        let mut spec: serde_json::Value = serde_json::json!({
            "paths": {
                "/get/test/record/{asset_id}": {"get": {}},
                "/get/test/dupe-group/{hash}": {"get": {}},
                "/put/assign_album": {"put": {}}
            },
            "components": {
                "schemas": {
                    "TestRecordProbe": {"type": "object"},
                    "DupeGroupMember": {"type": "object"},
                    "AssignAlbumData": {"type": "object"}
                }
            }
        });

        strip_test_only_endpoints(&mut spec);

        let paths = spec["paths"].as_object().expect("paths object");
        assert!(!paths.contains_key("/get/test/record/{asset_id}"));
        assert!(!paths.contains_key("/get/test/dupe-group/{hash}"));
        assert!(paths.contains_key("/put/assign_album"));

        let schemas = spec["components"]["schemas"].as_object().expect("schemas");
        assert!(!schemas.contains_key("TestRecordProbe"));
        assert!(!schemas.contains_key("DupeGroupMember"));
        assert!(schemas.contains_key("AssignAlbumData"));
    }

    #[test]
    fn public_json_never_contains_test_probes() {
        let json = public_json();
        assert!(
            !json.contains("/get/test/"),
            "test endpoints leaked into the public spec"
        );
        assert!(
            !json.contains("TestRecordProbe"),
            "test schema leaked into the public spec"
        );
        assert!(
            !json.contains("DupeGroupMember"),
            "test schema leaked into the public spec"
        );
        assert!(
            json.contains("/put/assign_album"),
            "production endpoint missing"
        );
    }

    #[test]
    fn public_json_is_stable_and_pretty_printed() {
        let json = public_json();

        assert_eq!(json, public_json(), "spec generation must be stable");
        assert!(json.ends_with('\n'), "artifact must end with a newline");
        assert!(
            json.lines().count() > 100,
            "artifact must be pretty-printed, not a single minified line"
        );
    }

    #[test]
    fn committed_artifact_is_sorted() {
        // `just openapi-check` diffs the committed artifact against freshly
        // generated output. If object key order stopped being sorted, every run
        // would produce a spurious diff, so the ordering is asserted instead of
        // assumed.
        fn assert_sorted(value: &serde_json::Value, path: &str) {
            if let Some(map) = value.as_object() {
                let keys: Vec<&String> = map.keys().collect();
                let mut sorted = keys.clone();
                sorted.sort_unstable();
                assert_eq!(keys, sorted, "unsorted object keys at {path}");
                for (key, child) in map {
                    assert_sorted(child, &format!("{path}/{key}"));
                }
            }
        }

        let spec: serde_json::Value =
            serde_json::from_str(&public_json()).expect("public spec must be valid JSON");
        assert_sorted(&spec, "");
    }

    /// The committed artifact must equal a fresh generation, so spec drift
    /// fails `cargo test` as well as `just openapi-check`. Without this, a
    /// stale `backend/openapi.json` would only be caught by a recipe that
    /// developers can skip locally.
    #[test]
    fn committed_artifact_is_up_to_date() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("openapi.json");
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!("cannot read {}: {e}", path.display());
        });

        assert_eq!(
            committed,
            public_json(),
            "backend/openapi.json is stale — run `just openapi-gen` and commit the result"
        );
    }
}
