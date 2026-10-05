use std::path::Path;

use crate::process::format::{Detection, detect_from_path, extension_matches, kind_for_extension};

/// The lowercase extension of `path`, if it is a supported media extension.
///
/// Inspects the name only and never touches the file, so it stays usable for
/// `Remove` events where the file is already gone.
fn media_extension(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_lowercase();
    kind_for_extension(&extension)?;
    Some(extension)
}

/// Whether the path carries a supported media extension.
///
/// Name-only, so it cannot decide whether the content matches. Use
/// [`classify_media_file`] before actually indexing something.
pub fn is_valid_media_file(path: &Path) -> bool {
    media_extension(path).is_some()
}

/// Why a discovered file is not going to be indexed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// The extension is not a supported media extension.
    UnsupportedExtension,
    /// The content matched a signature that is not a format we accept.
    ContentMismatch { detected_extension: String },
    /// The content matched no known signature.
    UnrecognizedContent,
    /// The leading bytes could not be read, so the content cannot be judged.
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaOutcome {
    Index,
    Skip(SkipReason),
}

/// Decide whether a file found on disk should be indexed, using both its
/// extension and its content.
///
/// Files that do not match are not errors: a library legitimately contains
/// things this application does not process, so the outcome is a skip the
/// caller can log and count. The extension is judged by
/// [`is_valid_media_file`], which the removal path also relies on and which
/// therefore must stay name-only.
pub fn classify_media_file(path: &Path) -> MediaOutcome {
    let Some(extension) = media_extension(path) else {
        return MediaOutcome::Skip(SkipReason::UnsupportedExtension);
    };

    let Ok(detected) = detect_from_path(path) else {
        return MediaOutcome::Skip(SkipReason::Unreadable);
    };

    match detected {
        None => MediaOutcome::Skip(SkipReason::UnrecognizedContent),
        Some(Detection::Unsupported { detected_extension }) => {
            MediaOutcome::Skip(SkipReason::ContentMismatch { detected_extension })
        }
        Some(Detection::Supported(format)) if !extension_matches(&extension, format) => {
            MediaOutcome::Skip(SkipReason::ContentMismatch {
                detected_extension: format.canonical_extension().to_string(),
            })
        }
        Some(Detection::Supported(_)) => MediaOutcome::Index,
    }
}

#[cfg(test)]
mod tests {
    use super::{MediaOutcome, SkipReason, classify_media_file};
    use std::io::Write;
    use std::path::Path;

    fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("picasu-media-test-{name}"));
        let mut file = std::fs::File::create(&path).expect("create temp file");
        file.write_all(bytes).expect("write temp file");
        path
    }

    #[test]
    fn a_matching_image_is_indexed() {
        let path = write_temp("match.jpg", &[0xff, 0xd8, 0xff, 0xe0]);

        assert_eq!(classify_media_file(&path), MediaOutcome::Index);
    }

    #[test]
    fn an_unsupported_extension_is_skipped() {
        let path = write_temp("notes.txt", b"hello");

        assert_eq!(
            classify_media_file(&path),
            MediaOutcome::Skip(SkipReason::UnsupportedExtension)
        );
    }

    #[test]
    fn content_contradicting_the_extension_is_skipped() {
        let path = write_temp(
            "mismatch.jpg",
            &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a],
        );

        assert_eq!(
            classify_media_file(&path),
            MediaOutcome::Skip(SkipReason::ContentMismatch {
                detected_extension: "png".to_string()
            })
        );
    }

    #[test]
    fn bytes_matching_no_signature_are_skipped() {
        let path = write_temp("garbage.jpg", b"this is not an image at all");

        assert_eq!(
            classify_media_file(&path),
            MediaOutcome::Skip(SkipReason::UnrecognizedContent)
        );
    }

    #[test]
    fn a_format_we_do_not_accept_is_skipped_by_name() {
        let mut bytes = vec![0x00, 0x00, 0x00, 0x20];
        bytes.extend_from_slice(b"ftypheic");
        bytes.resize(32, 0);
        let path = write_temp("photo.jpg", &bytes);

        assert_eq!(
            classify_media_file(&path),
            MediaOutcome::Skip(SkipReason::ContentMismatch {
                detected_extension: "heif".to_string()
            })
        );
    }

    #[test]
    fn an_unreadable_path_is_skipped_rather_than_indexed() {
        let path = Path::new("/nonexistent/picasu/definitely/missing.jpg");

        assert_eq!(
            classify_media_file(path),
            MediaOutcome::Skip(SkipReason::Unreadable)
        );
    }
}
