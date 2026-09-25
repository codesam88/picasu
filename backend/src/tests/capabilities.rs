#[cfg(test)]
use snapfab::capabilities;

use crate::constant::{VALID_IMAGE_EXTENSIONS, VALID_VIDEO_EXTENSIONS};

/// The manifest must not claim an extension the upload and index paths reject.
/// The reverse is not asserted: the manifest covers the formats snapfab
/// generates, which is a subset of what the backend accepts.
#[test]
fn capability_manifest_extensions_are_accepted_by_the_backend() {
    let manifest = capabilities::load_capabilities().expect("capability manifest must load");

    for entry in &manifest.formats {
        for extension in &entry.extensions {
            assert!(
                VALID_IMAGE_EXTENSIONS.contains(&extension.as_str())
                    || VALID_VIDEO_EXTENSIONS.contains(&extension.as_str()),
                "manifest format `{}` declares extension `{extension}` that the backend rejects",
                entry.format
            );
        }
    }
}
