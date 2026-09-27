//! Rocket URI to `OpenAPI` path-template translation.
//!
//! This is the one place the mapping is defined. Mounted-route parity compares a
//! runtime route table with the generated spec, so both sides have to agree on
//! the translation — a second copy of this function is how a route silently
//! stops matching its own documentation.

use openapi_sanity::to_spec_path;

#[test]
fn named_segments_become_templates() {
    assert_eq!(
        to_spec_path("/get/metadata/<asset_id>"),
        "/get/metadata/{asset_id}"
    );
}

#[test]
fn a_zero_or_more_segment_drops_the_dots() {
    assert_eq!(
        to_spec_path("/object/compressed/<file_path..>"),
        "/object/compressed/{file_path}"
    );
}

#[test]
fn a_leading_underscore_is_dropped() {
    // `<_path..>` exists to avoid a clash with the handler name; OpenAPI has no
    // counterpart for the underscore.
    assert_eq!(
        to_spec_path("/albums/view/<_path..>"),
        "/albums/view/{path}"
    );
    assert_eq!(to_spec_path("/assets/<_file..>"), "/assets/{file}");
}

#[test]
fn the_query_part_is_not_part_of_the_path() {
    // Query parameters are documented per parameter, not in the path.
    assert_eq!(to_spec_path("/get/prefetch?<locate>"), "/get/prefetch");
    assert_eq!(
        to_spec_path("/upload?<auto_rename>&<on_conflict>"),
        "/upload"
    );
}

#[test]
fn a_path_without_parameters_is_unchanged() {
    assert_eq!(to_spec_path("/upload"), "/upload");
    assert_eq!(to_spec_path("/login"), "/login");
    assert_eq!(to_spec_path(""), "");
}

#[test]
fn an_unterminated_segment_is_kept_verbatim() {
    // Not a segment declaration: the remainder is passed through rather than
    // silently dropped, so a malformed URI is visible in the comparison.
    assert_eq!(to_spec_path("/get/<broken"), "/get/<broken");
}

#[test]
fn several_segments_in_one_path_are_all_translated() {
    assert_eq!(
        to_spec_path("/albums/view/<_path..>/photos/<index>/raw"),
        "/albums/view/{path}/photos/{index}/raw"
    );
}
