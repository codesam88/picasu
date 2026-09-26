//! Content-based format detection used to reject files whose bytes contradict
//! their declared type.
//!
//! Detection reads a leading window of the file only and never performs a full
//! decode. Note that the WebP signature is weak: `infer` checks only that bytes
//! 8..12 are `WEBP`, without confirming the `RIFF` prefix, so a file crafted
//! with those four bytes is accepted as WebP.

use std::path::Path;

/// A media format this application accepts, and how it is recognized.
///
/// This table is the single source of truth for what is supported. The
/// extension allowlists, the image/video classification, and the
/// content-to-format mapping are all derived from it, so adding a format means
/// adding one row rather than editing a list in three places.
///
/// Two decoders exist and they are not interchangeable: images are decoded with
/// the `image` crate and videos with ffmpeg/ffprobe. `kind` records which one
/// handles a format, and therefore which decoder a new format needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectedFormat {
    Jpeg,
    Png,
    Gif,
    Webp,
    Tiff,
    Bmp,
    Mp4,
    Mov,
    M4v,
    Mkv,
    Webm,
    Avi,
    Flv,
    Wmv,
    Mpeg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// Decoded by the `image` crate.
    Image,
    /// Probed and decoded by ffmpeg/ffprobe.
    Video,
}

pub struct SupportedFormat {
    pub format: DetectedFormat,
    /// The extension `infer` reports for this format. Not always the extension
    /// we accept: MPEG is detected as `mpg` but accepted as `mpeg`.
    pub detected_extension: &'static str,
    /// Extensions a client may declare for this format.
    pub extensions: &'static [&'static str],
    /// Formats sharing a container cannot be told apart from content alone, so
    /// they accept each other's extensions. Unique per container.
    pub family: &'static str,
    pub kind: MediaKind,
}

/// The accepted formats.
///
/// `gif` is deliberately a video: it is probed with ffprobe and recorded as
/// `ObjectType::Video`, which is long-standing behavior rather than a claim
/// about which decoder could handle it.
pub static SUPPORTED_FORMATS: &[SupportedFormat] = &[
    SupportedFormat {
        format: DetectedFormat::Jpeg,
        detected_extension: "jpg",
        extensions: &["jpg", "jpeg", "jfif", "jpe"],
        family: "jpeg",
        kind: MediaKind::Image,
    },
    SupportedFormat {
        format: DetectedFormat::Png,
        detected_extension: "png",
        extensions: &["png"],
        family: "png",
        kind: MediaKind::Image,
    },
    SupportedFormat {
        format: DetectedFormat::Gif,
        detected_extension: "gif",
        extensions: &["gif"],
        family: "gif",
        kind: MediaKind::Video,
    },
    SupportedFormat {
        format: DetectedFormat::Webp,
        detected_extension: "webp",
        extensions: &["webp"],
        family: "webp",
        kind: MediaKind::Image,
    },
    SupportedFormat {
        format: DetectedFormat::Tiff,
        detected_extension: "tif",
        extensions: &["tif", "tiff"],
        family: "tiff",
        kind: MediaKind::Image,
    },
    SupportedFormat {
        format: DetectedFormat::Bmp,
        detected_extension: "bmp",
        extensions: &["bmp"],
        family: "bmp",
        kind: MediaKind::Image,
    },
    SupportedFormat {
        format: DetectedFormat::Mp4,
        detected_extension: "mp4",
        extensions: &["mp4"],
        family: "isobmff",
        kind: MediaKind::Video,
    },
    SupportedFormat {
        format: DetectedFormat::Mov,
        detected_extension: "mov",
        extensions: &["mov"],
        family: "isobmff",
        kind: MediaKind::Video,
    },
    SupportedFormat {
        format: DetectedFormat::M4v,
        detected_extension: "m4v",
        extensions: &["m4v"],
        family: "isobmff",
        kind: MediaKind::Video,
    },
    SupportedFormat {
        format: DetectedFormat::Mkv,
        detected_extension: "mkv",
        extensions: &["mkv"],
        family: "ebml",
        kind: MediaKind::Video,
    },
    SupportedFormat {
        format: DetectedFormat::Webm,
        detected_extension: "webm",
        extensions: &["webm"],
        family: "ebml",
        kind: MediaKind::Video,
    },
    SupportedFormat {
        format: DetectedFormat::Avi,
        detected_extension: "avi",
        extensions: &["avi"],
        family: "avi",
        kind: MediaKind::Video,
    },
    SupportedFormat {
        format: DetectedFormat::Flv,
        detected_extension: "flv",
        extensions: &["flv"],
        family: "flv",
        kind: MediaKind::Video,
    },
    SupportedFormat {
        format: DetectedFormat::Wmv,
        detected_extension: "wmv",
        extensions: &["wmv"],
        family: "wmv",
        kind: MediaKind::Video,
    },
    SupportedFormat {
        format: DetectedFormat::Mpeg,
        detected_extension: "mpg",
        extensions: &["mpeg"],
        family: "mpeg",
        kind: MediaKind::Video,
    },
];

/// The outcome of inspecting a file's leading bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    /// The content was identified as a real format that is outside our table,
    /// such as HEIF or AVIF. The extension is kept so callers can report what
    /// was actually found instead of claiming the bytes are unrecognized.
    Unsupported { detected_extension: String },
    /// The content matches an accepted format.
    Supported(DetectedFormat),
}

impl DetectedFormat {
    /// The extension `infer` reports for this format.
    pub fn canonical_extension(self) -> &'static str {
        entry(self).detected_extension
    }
}

pub fn entry(format: DetectedFormat) -> &'static SupportedFormat {
    SUPPORTED_FORMATS
        .iter()
        .find(|candidate| candidate.format == format)
        .expect("every DetectedFormat has a SUPPORTED_FORMATS row")
}

/// Every extension a client may declare, in table order.
#[allow(dead_code)] // exercised by unit tests; production uses kind_for_extension
pub fn accepted_extensions() -> Vec<&'static str> {
    SUPPORTED_FORMATS
        .iter()
        .flat_map(|format| format.extensions.iter().copied())
        .collect()
}

#[allow(dead_code)] // exercised by unit tests; production uses kind_for_extension
pub fn accepted_extensions_of_kind(kind: MediaKind) -> Vec<&'static str> {
    SUPPORTED_FORMATS
        .iter()
        .filter(|format| format.kind == kind)
        .flat_map(|format| format.extensions.iter().copied())
        .collect()
}

/// Which decoder handles a declared extension, or `None` if it is not accepted.
pub fn kind_for_extension(extension: &str) -> Option<MediaKind> {
    SUPPORTED_FORMATS
        .iter()
        .find(|format| format.extensions.contains(&extension))
        .map(|format| format.kind)
}

/// Identify the content of `head`, or `None` when the bytes match no known
/// signature at all.
#[allow(dead_code)] // exercised by unit tests; no production caller yet
pub fn detect_from_bytes(head: &[u8]) -> Option<Detection> {
    infer::get(head).map(|detected| classify(detected.extension()))
}

/// Identify the content of the file at `path`, reading only a leading window.
pub fn detect_from_path(path: &Path) -> std::io::Result<Option<Detection>> {
    Ok(infer::get_from_path(path)?.map(|detected| classify(detected.extension())))
}

/// Whether content detected as `detected` is consistent with the `claimed`
/// extension.
///
/// Formats in the same container family accept each other, because a declared
/// container cannot be distinguished from the detected brand by content alone.
pub fn extension_matches(claimed: &str, detected: DetectedFormat) -> bool {
    let detected_entry = entry(detected);
    detected_entry.extensions.contains(&claimed)
        || SUPPORTED_FORMATS
            .iter()
            .filter(|candidate| candidate.family == detected_entry.family)
            .any(|candidate| candidate.extensions.contains(&claimed))
}

fn classify(detected_extension: &str) -> Detection {
    match SUPPORTED_FORMATS
        .iter()
        .find(|format| format.detected_extension == detected_extension)
    {
        Some(format) => Detection::Supported(format.format),
        None => Detection::Unsupported {
            detected_extension: detected_extension.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DetectedFormat, Detection, MediaKind, accepted_extensions, accepted_extensions_of_kind,
        detect_from_bytes, extension_matches,
    };

    fn isobmff(brand: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x00, 0x00, 0x00, 0x20];
        bytes.extend_from_slice(b"ftyp");
        bytes.extend_from_slice(brand);
        bytes.resize(32, 0);
        bytes
    }

    fn ebml(doc_type: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x1a, 0x45, 0xdf, 0xa3];
        bytes.extend_from_slice(doc_type);
        bytes.resize(257, 0);
        bytes
    }

    /// One signature per accepted format. This table is the backbone of the
    /// detection tests below, and doubles as the fixture set for the allowlist
    /// coverage test.
    fn signatures() -> Vec<(DetectedFormat, Vec<u8>)> {
        vec![
            (DetectedFormat::Jpeg, vec![0xff, 0xd8, 0xff, 0xe0]),
            (
                DetectedFormat::Png,
                vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a],
            ),
            (DetectedFormat::Gif, b"GIF89a".to_vec()),
            (DetectedFormat::Webp, b"RIFF\x00\x00\x00\x00WEBP".to_vec()),
            (
                DetectedFormat::Tiff,
                vec![0x49, 0x49, 0x2a, 0x00, 0, 0, 0, 0, 0x08, 0x00],
            ),
            (DetectedFormat::Bmp, b"BM\x36\x00".to_vec()),
            (DetectedFormat::Mp4, isobmff(b"isom")),
            (DetectedFormat::Mov, isobmff(b"qt  ")),
            (DetectedFormat::M4v, isobmff(b"M4V ")),
            (DetectedFormat::Mkv, ebml(b"\x42\x82\x88matroska")),
            (DetectedFormat::Webm, ebml(b"\x42\x82\x84webm")),
            (
                DetectedFormat::Avi,
                b"RIFF\x00\x00\x00\x00AVI LIST".to_vec(),
            ),
            (DetectedFormat::Flv, b"FLV\x01\x05".to_vec()),
            (
                DetectedFormat::Wmv,
                vec![0x30, 0x26, 0xb2, 0x75, 0x8e, 0x66, 0xcf, 0x11, 0xa6, 0xd9],
            ),
            // 0x000001ba is the MPEG program stream sequence header, which is
            // what the `mpeg` allowlist entry is meant to cover.
            (DetectedFormat::Mpeg, vec![0x00, 0x00, 0x01, 0xba]),
        ]
    }

    #[test]
    fn every_signature_detects_to_its_own_format() {
        for (expected, bytes) in signatures() {
            assert_eq!(
                detect_from_bytes(&bytes),
                Some(Detection::Supported(expected)),
                "unexpected detection for {expected:?}"
            );
        }
    }

    /// Anchors `canonical_extension` to `infer` rather than to itself, so a
    /// wrong or missing mapping arm cannot pass unnoticed.
    #[test]
    fn canonical_extensions_agree_with_the_infer_database() {
        for (expected, bytes) in signatures() {
            let detected = infer::get(&bytes).expect("signature should be in the infer database");
            assert_eq!(
                detected.extension(),
                expected.canonical_extension(),
                "canonical_extension disagrees with infer for {expected:?}"
            );
        }
    }

    /// Every extension the upload and index paths accept must correspond to a
    /// format this module recognizes, otherwise a legitimate file would be
    /// rejected once the check became unconditional.
    fn sorted(mut extensions: Vec<&'static str>) -> Vec<&'static str> {
        extensions.sort_unstable();
        extensions
    }

    /// Pins the accepted extension lists so a table edit cannot silently change
    /// which files the application takes. `m4v` is the one addition relative to
    /// the previous hand-written lists; it was already detectable and ffmpeg
    /// already handled it, but `.m4v` files on disk used to be skipped.
    #[test]
    fn the_accepted_extension_lists_are_explicit() {
        assert_eq!(
            sorted(accepted_extensions_of_kind(MediaKind::Image)),
            [
                "bmp", "jfif", "jpe", "jpeg", "jpg", "png", "tif", "tiff", "webp"
            ]
        );
        assert_eq!(
            sorted(accepted_extensions_of_kind(MediaKind::Video)),
            [
                "avi", "flv", "gif", "m4v", "mkv", "mov", "mp4", "mpeg", "webm", "wmv"
            ]
        );
    }

    #[test]
    fn every_table_row_is_reachable_from_content_and_claims_an_extension() {
        for format in super::SUPPORTED_FORMATS {
            assert!(
                !format.extensions.is_empty(),
                "{:?} claims no extension",
                format.format
            );
            assert!(
                accepted_extensions_of_kind(format.kind).contains(&format.extensions[0]),
                "{:?} is not reachable from its own extension",
                format.format
            );
        }
    }

    #[test]
    fn every_allowed_extension_has_a_recognized_format() {
        for claimed in accepted_extensions() {
            let covered = signatures()
                .iter()
                .any(|(format, _)| extension_matches(claimed, *format));
            assert!(
                covered,
                "no recognized format satisfies the `{claimed}` extension"
            );
        }
    }

    #[test]
    fn bytes_matching_no_signature_are_not_detected() {
        assert_eq!(detect_from_bytes(b"this is not an image at all"), None);
        assert_eq!(detect_from_bytes(&[]), None);
    }

    /// HEIF/HEIC and AVIF have real `infer` matchers but are rejected by policy.
    /// They must surface as unsupported, not as an unrecognized byte blob, so
    /// the user is told what was actually uploaded.
    #[test]
    fn recognized_but_unsupported_formats_report_their_extension() {
        // `infer` reports every HEIF brand under the single canonical `heif`
        // extension, so the brand cannot be echoed back to the user.
        for (bytes, expected) in [
            (isobmff(b"heic"), "heif"),
            (isobmff(b"heix"), "heif"),
            (isobmff(b"avif"), "avif"),
        ] {
            assert_eq!(
                detect_from_bytes(&bytes),
                Some(Detection::Unsupported {
                    detected_extension: expected.to_string()
                }),
                "expected {expected} to be reported as unsupported"
            );
        }
    }

    #[test]
    fn aliases_of_the_same_format_are_accepted() {
        for (claimed, detected) in [
            ("jpg", DetectedFormat::Jpeg),
            ("jpeg", DetectedFormat::Jpeg),
            ("jfif", DetectedFormat::Jpeg),
            ("jpe", DetectedFormat::Jpeg),
            ("tif", DetectedFormat::Tiff),
            ("tiff", DetectedFormat::Tiff),
            ("png", DetectedFormat::Png),
            ("webp", DetectedFormat::Webp),
            ("bmp", DetectedFormat::Bmp),
            ("gif", DetectedFormat::Gif),
            ("flv", DetectedFormat::Flv),
            ("wmv", DetectedFormat::Wmv),
            ("avi", DetectedFormat::Avi),
        ] {
            assert!(
                extension_matches(claimed, detected),
                "{claimed} should accept {detected:?}"
            );
        }
    }

    #[test]
    fn container_sharing_families_are_accepted_across_brands() {
        // ISO-BMFF: a `.mov` upload is often detected as mp4 and vice versa.
        for (claimed, detected) in [
            ("mp4", DetectedFormat::Mov),
            ("mov", DetectedFormat::Mp4),
            ("m4v", DetectedFormat::Mp4),
            ("m4v", DetectedFormat::Mov),
        ] {
            assert!(
                extension_matches(claimed, detected),
                "{claimed} should accept {detected:?}"
            );
        }

        // EBML: a `.webm` upload is detected as mkv unless it carries the webm
        // DocType, and vice versa.
        for (claimed, detected) in [("mkv", DetectedFormat::Webm), ("webm", DetectedFormat::Mkv)] {
            assert!(
                extension_matches(claimed, detected),
                "{claimed} should accept {detected:?}"
            );
        }
    }

    #[test]
    fn a_mismatched_format_is_rejected() {
        assert!(!extension_matches("png", DetectedFormat::Jpeg));
        assert!(!extension_matches("jpg", DetectedFormat::Png));
        assert!(!extension_matches("mp4", DetectedFormat::Jpeg));
        assert!(!extension_matches("tiff", DetectedFormat::Png));
        assert!(!extension_matches("gif", DetectedFormat::Png));
    }

    #[test]
    fn the_declared_mpeg_extension_differs_from_the_detected_spelling() {
        // `infer` reports MPEG as `mpg`; the allowlist spells it `mpeg`, so the
        // mapping has to be explicit rather than a plain string comparison.
        assert_eq!(DetectedFormat::Mpeg.canonical_extension(), "mpg");
        assert!(extension_matches("mpeg", DetectedFormat::Mpeg));
    }
}
