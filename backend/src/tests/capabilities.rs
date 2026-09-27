use snapfab::capabilities;
use snapfab::selection::randomizable_formats;

/// The manifest must not claim an extension the upload and index paths reject.
/// The reverse is not asserted: the manifest covers the formats snapfab
/// generates, which is a subset of what the backend accepts.
#[test]
fn capability_manifest_extensions_are_accepted_by_the_backend() {
    let manifest = capabilities::load_capabilities().expect("capability manifest must load");

    for entry in &manifest.formats {
        for extension in &entry.extensions {
            assert!(
                crate::process::format::accepted_extensions().contains(&extension.as_str()),
                "manifest format `{}` declares extension `{extension}` that the backend rejects",
                entry.format
            );
        }
    }
}

/// The same rule for the narrower set a randomized scenario can actually pick.
///
/// `capability_manifest_extensions_are_accepted_by_the_backend` covers every
/// declared format; this one covers the formats selection hands to a scenario,
/// which is the set that reaches a real index and a real upload. It is the guard
/// that keeps a format the product rejects out of randomized coverage even if the
/// manifest were ever given a fixture for it: the fixture rule in
/// `snapfab::selection` makes it selectable, and this test then fails because the
/// extension is not in the allowlist. HEIF/HEIC and AVIF have no fixture and so
/// never reach it — `a_format_without_a_verified_fixture_is_never_selected` in
/// snapfab covers that half.
#[test]
fn every_randomizable_format_is_accepted_by_the_backend() {
    let manifest = capabilities::load_capabilities().expect("capability manifest must load");
    let randomizable = randomizable_formats(&manifest);

    assert!(
        !randomizable.is_empty(),
        "no manifest format is randomizable, so no randomized scenario can run"
    );
    for entry in &randomizable {
        assert!(
            crate::process::format::accepted_extensions().contains(&entry.extension.as_str()),
            "randomizable format `{}` writes `{}`, which the backend rejects",
            entry.format,
            entry.extension
        );
    }
}
