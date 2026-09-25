use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::LazyLock;

const SUPPORTED_SCHEMA_VERSION: u32 = 1;
/// Fields the backend actually reads. A field outside this list is rejected so a
/// typo cannot masquerade as a capability claim.
const METADATA_FIELDS: &[&str] = &["exif", "xmp"];
const METADATA_SOURCES: &[&str] = &["embedded", "sidecar"];
const FAILURE_CLASSES: &[&str] = &[
    "none",
    "signature_mismatch",
    "empty",
    "truncated",
    "random_bytes",
    "corrupt_metadata",
];

/// The formats snapfab generates fixtures for, and the capabilities those
/// fixtures exercise.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityManifest {
    pub schema_version: u32,
    pub formats: Vec<FormatCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormatCapability {
    pub format: String,
    pub extensions: Vec<String>,
    pub content_signature: ContentSignature,
    /// Metadata fields this manifest positively claims, mapped to the sources
    /// the backend reads them from.
    pub metadata_fields: HashMap<String, Vec<String>>,
    /// `field:source` pairs deliberately excluded from the claims above,
    /// recorded so that a consumer can distinguish "not claimed" from "not
    /// applicable". This is a fixture-coverage policy, not an assertion about
    /// what the backend would do if such a packet were present.
    pub unsupported_metadata_fields: Vec<String>,
    pub expected_failure_classes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentSignature {
    pub offset: usize,
    pub bytes_hex: String,
    pub mime: String,
}

impl ContentSignature {
    pub fn bytes(&self) -> Vec<u8> {
        (0..self.bytes_hex.len())
            .step_by(2)
            .map(|index| {
                u8::from_str_radix(&self.bytes_hex[index..index + 2], 16).unwrap_or_default()
            })
            .collect()
    }
}

#[derive(Debug, PartialEq)]
pub enum CapabilityError {
    Parse(String),
    Validation(String),
}

impl fmt::Display for CapabilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(message) => write!(formatter, "capability manifest parse error: {message}"),
            Self::Validation(message) => {
                write!(formatter, "capability manifest validation error: {message}")
            }
        }
    }
}

impl std::error::Error for CapabilityError {}

static CAPABILITIES: LazyLock<CapabilityManifest> =
    LazyLock::new(|| load_capabilities().expect("checked-in capability manifest must be valid"));

pub fn capabilities() -> &'static CapabilityManifest {
    &CAPABILITIES
}

pub fn load_capabilities() -> Result<CapabilityManifest, CapabilityError> {
    parse_manifest(include_str!("../capabilities.json"))
}

pub fn parse_manifest(input: &str) -> Result<CapabilityManifest, CapabilityError> {
    let manifest: CapabilityManifest =
        serde_json::from_str(input).map_err(|error| CapabilityError::Parse(error.to_string()))?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

pub fn validate_manifest(manifest: &CapabilityManifest) -> Result<(), CapabilityError> {
    if manifest.schema_version != SUPPORTED_SCHEMA_VERSION {
        return validation_error("unsupported schema version");
    }
    if manifest.formats.is_empty() {
        return validation_error("manifest must contain at least one format");
    }

    let mut formats = HashSet::new();
    let mut extensions = HashSet::new();
    for entry in &manifest.formats {
        validate_identifier(&entry.format, "format identifiers")?;
        if !formats.insert(entry.format.as_str()) {
            return validation_error("format identifiers must be unique");
        }
        if entry.extensions.is_empty() {
            return validation_error("each format must declare at least one extension");
        }
        for extension in &entry.extensions {
            validate_identifier(extension, "extensions")?;
            if !extensions.insert(extension.as_str()) {
                return validation_error("extensions must be unique");
            }
        }
        if entry.content_signature.bytes_hex.is_empty()
            || entry.content_signature.bytes_hex.len() % 2 != 0
            || !entry
                .content_signature
                .bytes_hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return validation_error("content signatures must contain non-empty even-length hex");
        }
        if entry.content_signature.mime.is_empty() {
            return validation_error("each format must declare a content MIME type");
        }
        if entry.metadata_fields.is_empty() {
            return validation_error("each format must declare at least one metadata field");
        }
        for (field, sources) in &entry.metadata_fields {
            if !METADATA_FIELDS.contains(&field.as_str()) {
                return validation_error("metadata field is not recognized");
            }
            if sources.is_empty() {
                return validation_error("metadata fields must map to at least one source");
            }
            for source in sources {
                if !METADATA_SOURCES.contains(&source.as_str()) {
                    return validation_error("metadata source is not recognized");
                }
            }
        }
        if entry.expected_failure_classes.is_empty() {
            return validation_error("expected failure classes must not be empty");
        }
        for failure_class in &entry.expected_failure_classes {
            if !FAILURE_CLASSES.contains(&failure_class.as_str()) {
                return validation_error("expected failure class is not recognized");
            }
        }
        let mut unsupported_seen = HashSet::new();
        for unsupported in &entry.unsupported_metadata_fields {
            if !unsupported_seen.insert(unsupported.as_str()) {
                return validation_error("unsupported metadata fields must be unique");
            }
            let parts = unsupported.split(':').collect::<Vec<_>>();
            let [field, source] = parts.as_slice() else {
                return validation_error("unsupported metadata fields must use `field:source`");
            };
            if !METADATA_FIELDS.contains(field) {
                return validation_error("unsupported metadata field is not recognized");
            }
            if !METADATA_SOURCES.contains(source) {
                return validation_error("unsupported metadata source is not recognized");
            }
            if entry
                .metadata_fields
                .get(*field)
                .is_some_and(|sources| sources.iter().any(|value| value == source))
            {
                return validation_error(
                    "a metadata field cannot be both supported and unsupported",
                );
            }
        }
    }
    Ok(())
}

impl CapabilityManifest {
    pub fn capability_for_format(&self, format: &str) -> Option<&FormatCapability> {
        self.formats.iter().find(|entry| entry.format == format)
    }

    pub fn capability_for_extension(&self, extension: &str) -> Option<&FormatCapability> {
        self.formats
            .iter()
            .find(|entry| entry.extensions.iter().any(|value| value == extension))
    }
}

fn validate_identifier(value: &str, kind: &str) -> Result<(), CapabilityError> {
    if value.is_empty() || value != value.to_lowercase() {
        return validation_error(&format!("{kind} must be non-empty lowercase strings"));
    }
    Ok(())
}

fn validation_error(message: &str) -> Result<(), CapabilityError> {
    Err(CapabilityError::Validation(message.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{CapabilityError, load_capabilities, parse_manifest};

    fn valid_entry() -> String {
        r#"{
            "format": "png",
            "extensions": ["png"],
            "contentSignature": {
                "offset": 0,
                "bytesHex": "89504e470d0a1a0a",
                "mime": "image/png"
            },
            "metadataFields": {"exif": ["embedded"]},
            "unsupportedMetadataFields": [],
            "expectedFailureClasses": ["none"]
        }"#
        .to_string()
    }

    fn manifest_with(entries: &[&str]) -> String {
        let entries = entries.join(",");
        format!(r#"{{"schemaVersion": 1, "formats": [{entries}]}}"#)
    }

    fn assert_valid(json: &str) {
        let error = parse_manifest(json).err();
        assert!(error.is_none(), "expected a valid manifest, got: {error:?}");
    }

    fn assert_validation_error(entries: &[&str]) {
        let json = manifest_with(entries);
        let error = parse_manifest(&json).expect_err("manifest should be rejected");
        assert!(
            matches!(error, CapabilityError::Validation(_)),
            "expected a validation error, got: {error:?}\ninput: {json}"
        );
    }

    /// Positive control: every negative test below builds on `valid_entry`, so
    /// it must itself be accepted.
    #[test]
    fn valid_entry_is_accepted() {
        assert_valid(&manifest_with(&[&valid_entry()]));
    }

    #[test]
    fn repository_manifest_declares_generator_formats() {
        let manifest = load_capabilities().expect("manifest should load");

        assert_eq!(manifest.schema_version, 1);
        assert_eq!(
            manifest
                .formats
                .iter()
                .map(|entry| entry.format.as_str())
                .collect::<Vec<_>>(),
            ["jpeg", "png"]
        );
    }

    #[test]
    fn lookup_handles_aliases_and_unknown_extensions() {
        let manifest = load_capabilities().expect("manifest should load");

        assert_eq!(
            manifest
                .capability_for_extension("jpeg")
                .expect("jpeg alias should resolve")
                .format,
            "jpeg"
        );
        assert!(manifest.capability_for_extension("webp").is_none());
    }

    #[test]
    fn content_signature_decodes_to_bytes() {
        let manifest = load_capabilities().expect("manifest should load");

        assert_eq!(
            manifest
                .capability_for_format("png")
                .expect("png should be declared")
                .content_signature
                .bytes(),
            vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]
        );
    }

    #[test]
    fn missing_required_field_is_a_parse_error() {
        let error = parse_manifest(
            r#"{
                "schemaVersion": 1,
                "formats": [{
                    "format": "png",
                    "extensions": ["png"],
                    "contentSignature": {"offset": 0, "bytesHex": "89504e47", "mime": "image/png"},
                    "metadataFields": {"exif": ["embedded"]}
                }]
            }"#,
        )
        .expect_err("missing expected failure classes should fail");

        assert!(matches!(error, CapabilityError::Parse(_)));
    }

    #[test]
    fn duplicate_extensions_are_rejected() {
        let entry = valid_entry().replace(r#"["png"]"#, r#"["jpg"]"#);
        let first = entry.replace(r#""png""#, r#""jpeg""#);

        assert_validation_error(&[&first, &entry]);
    }

    #[test]
    fn duplicate_format_identifiers_are_rejected() {
        let entry = valid_entry();

        assert_validation_error(&[&entry, &entry]);
    }

    #[test]
    fn unsupported_schema_version_is_rejected() {
        let entry = valid_entry();
        let json =
            manifest_with(&[&entry]).replace(r#""schemaVersion": 1"#, r#""schemaVersion": 2"#);
        let error = parse_manifest(&json).expect_err("unknown schema version should fail");

        assert!(matches!(error, CapabilityError::Validation(_)));
    }

    #[test]
    fn empty_manifest_is_rejected() {
        let error = parse_manifest(r#"{"schemaVersion": 1, "formats": []}"#)
            .expect_err("an empty manifest should fail");

        assert!(matches!(error, CapabilityError::Validation(_)));
    }

    #[test]
    fn invalid_content_signature_is_rejected() {
        assert_validation_error(&[&valid_entry().replace("89504e470d0a1a0a", "not-hex")]);
    }

    #[test]
    fn unrecognized_enum_values_are_rejected() {
        assert_validation_error(&[
            &valid_entry().replace(r#"{"exif": ["embedded"]}"#, r#"{"exif": ["inline"]}"#)
        ]);
        assert_validation_error(&[&valid_entry().replace(
            r#""expectedFailureClasses": ["none"]"#,
            r#""expectedFailureClasses": ["vibes"]"#,
        )]);
    }

    #[test]
    fn empty_and_mixed_case_identifiers_are_rejected() {
        let cases = [
            valid_entry().replace(r#""format": "png""#, r#""format": """#),
            valid_entry().replace(r#""format": "png""#, r#""format": "PNG""#),
            valid_entry().replace(r#""extensions": ["png"]"#, r#""extensions": []"#),
            valid_entry().replace(r#""extensions": ["png"]"#, r#""extensions": [""]"#),
            valid_entry().replace(r#""extensions": ["png"]"#, r#""extensions": ["PNG"]"#),
            valid_entry().replace(
                r#""metadataFields": {"exif": ["embedded"]}"#,
                r#""metadataFields": {}"#,
            ),
            valid_entry().replace(
                r#""metadataFields": {"exif": ["embedded"]}"#,
                r#""metadataFields": {"Exif": ["embedded"]}"#,
            ),
            valid_entry().replace(
                r#""metadataFields": {"exif": ["embedded"]}"#,
                r#""metadataFields": {"xmop": ["embedded"]}"#,
            ),
            valid_entry().replace(
                r#""expectedFailureClasses": ["none"]"#,
                r#""expectedFailureClasses": []"#,
            ),
        ];

        for entry in &cases {
            assert_validation_error(&[entry]);
        }
    }

    #[test]
    fn unsupported_metadata_fields_are_validated() {
        let with_unsupported = |array_literal: &str| {
            valid_entry().replace(
                r#""unsupportedMetadataFields": []"#,
                &format!(r#""unsupportedMetadataFields": {array_literal}"#),
            )
        };

        for array_literal in [
            r#"["exif"]"#,                // missing source separator
            r#"["exif:inline"]"#,         // unknown source
            r#"[":embedded"]"#,           // empty field
            r#"["exif:"]"#,               // empty source
            r#"["exif:embedded:extra"]"#, // too many segments
            r#"["XMP:embedded"]"#,        // field is not lowercase
            r#"["xmp :embedded"]"#,       // whitespace in the field half
            r#"[" xmp:embedded"]"#,       // leading whitespace
            r#"["xmop:embedded"]"#,       // unknown field (typo)
            // A duplicate that is *not* also a contradiction, so removing the
            // uniqueness check cannot be masked by the contradiction check.
            r#"["xmp:sidecar","xmp:sidecar"]"#,
        ] {
            assert_validation_error(&[&with_unsupported(array_literal)]);
        }
    }

    #[test]
    fn a_metadata_field_cannot_be_both_supported_and_unsupported() {
        let entry = valid_entry().replace(
            r#""unsupportedMetadataFields": []"#,
            r#""unsupportedMetadataFields": ["exif:embedded"]"#,
        );

        assert_validation_error(&[&entry]);
    }

    #[test]
    fn repository_manifest_records_png_embedded_xmp_as_excluded() {
        let manifest = load_capabilities().expect("manifest should load");
        let png = manifest
            .capability_for_format("png")
            .expect("png should be declared");

        assert_eq!(png.unsupported_metadata_fields, ["xmp:embedded"]);
    }

    #[test]
    fn repository_manifest_claims_sidecar_xmp_for_both_formats() {
        let manifest = load_capabilities().expect("manifest should load");

        // `xmp.rs` resolves sidecars and scans embedded packets without any
        // format dispatch, so sidecar XMP applies to JPEG as well as PNG.
        for format in ["jpeg", "png"] {
            let entry = manifest
                .capability_for_format(format)
                .expect("format should be declared");
            assert!(
                entry.metadata_fields["xmp"].contains(&"sidecar".to_string()),
                "{format} should claim sidecar XMP"
            );
        }
    }

    #[test]
    fn capability_errors_render_a_descriptive_message() {
        let error = parse_manifest(r#"{"schemaVersion": 1, "formats": []}"#)
            .expect_err("an empty manifest should fail");

        assert_eq!(
            error.to_string(),
            "capability manifest validation error: manifest must contain at least one format"
        );
        let boxed: Box<dyn std::error::Error> = Box::new(error);
        assert!(boxed.to_string().contains("validation error"));
    }
}
