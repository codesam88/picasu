//! The public `OpenAPI` artifact: the generated spec with test-only surface removed.
//!
//! The committed reference (`docs/openapi-reference.md`) and `--dump-openapi`
//! must not advertise test-only endpoints. Probe contract tests keep using
//! [`crate::openapi::generate_json`], which returns the full spec.

use crate::openapi::generate_json;

/// Remove test-only probe endpoints (`/get/test/...`) and their schemas from
/// a serialized `OpenAPI` document.
pub fn strip_test_only_endpoints(spec: &mut serde_json::Value) {
    if let Some(paths) = spec.get_mut("paths").and_then(|p| p.as_object_mut()) {
        paths.retain(|key, _| !key.starts_with("/get/test/"));
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
/// document minus test-only probes.
///
/// # Panics
/// Panics if the generated document is not valid JSON.
#[must_use]
pub fn public_json() -> String {
    let mut spec: serde_json::Value =
        serde_json::from_str(&generate_json()).expect("generated OpenAPI must be valid JSON");
    strip_test_only_endpoints(&mut spec);
    serde_json::to_string(&spec).expect("public OpenAPI serialization failed")
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
}
