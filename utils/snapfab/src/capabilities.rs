use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

const SUPPORTED_SCHEMA_VERSION: u32 = 1;
const METADATA_SOURCES: &[&str] = &["embedded", "sidecar"];
const FAILURE_CLASSES: &[&str] = &[
    "none",
    "signature_mismatch",
    "empty",
    "truncated",
    "random_bytes",
    "corrupt_metadata",
];

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
    pub metadata_fields: HashMap<String, Vec<String>>,
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
            if field.is_empty() || sources.is_empty() {
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
            "expectedFailureClasses": ["none"]
        }"#
        .to_string()
    }

    fn manifest_with(entry: &str) -> String {
        format!(r#"{{"schemaVersion": 1, "formats": [{entry}]}}"#)
    }

    #[test]
    fn repository_manifest_declares_verified_formats() {
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
        let error = parse_manifest(&format!(
            r#"{{"schemaVersion": 1, "formats": [{first}, {entry}]}}"#
        ))
        .expect_err("duplicate extension should fail");

        assert!(matches!(error, CapabilityError::Validation(_)));
    }

    #[test]
    fn unsupported_schema_version_is_rejected() {
        let entry = valid_entry();
        let error = parse_manifest(
            &manifest_with(&entry).replace(r#""schemaVersion": 1"#, r#""schemaVersion": 2"#),
        )
        .expect_err("unknown schema version should fail");

        assert!(matches!(error, CapabilityError::Validation(_)));
    }

    #[test]
    fn invalid_content_signature_is_rejected() {
        let entry = valid_entry().replace("89504e470d0a1a0a", "not-hex");
        let error =
            parse_manifest(&manifest_with(&entry)).expect_err("non-hex signature should fail");

        assert!(matches!(error, CapabilityError::Validation(_)));
    }

    #[test]
    fn unrecognized_enum_values_are_rejected() {
        let cases = [
            valid_entry().replace(r#"{"exif": ["embedded"]}"#, r#"{"exif": ["inline"]}"#),
            valid_entry().replace(
                r#""expectedFailureClasses": ["none"]"#,
                r#""expectedFailureClasses": ["vibes"]"#,
            ),
        ];

        for json in cases {
            let error = parse_manifest(&manifest_with(&json))
                .expect_err("unrecognized enum value should fail");
            assert!(matches!(error, CapabilityError::Validation(_)));
        }
    }
}
