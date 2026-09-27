use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::LazyLock;

const SUPPORTED_SCHEMA_VERSION: u32 = 1;
/// Fields the backend actually reads. A field outside this list is rejected so a
/// typo cannot masquerade as a capability claim. `container` is the field for
/// container/stream metadata that does not live in the file's bytes at all:
/// for a video it is produced by `ffprobe`, which is a different contract from
/// `exif` and `xmp` and is recorded as such (`source: "probe"`).
const METADATA_FIELDS: &[&str] = &["exif", "xmp", "container"];
/// `probe` marks a field the backend reads from an external tool rather than
/// from the file itself. The other two are locations inside or beside the file.
const METADATA_SOURCES: &[&str] = &["embedded", "sidecar", "probe"];
const FAILURE_CLASSES: &[&str] = &[
    "none",
    "signature_mismatch",
    "empty",
    "truncated",
    "random_bytes",
    "corrupt_metadata",
];
/// Where a format's fixtures come from: snapfab encodes the format, or the
/// format is covered by checked-in bytes instead.
const FIXTURE_SOURCES: &[&str] = &["generated", "pinned"];
/// How a checked-in fixture was produced. The plan requires a fixture to say
/// which, so a synthetic file is never mistaken for upstream test data.
const FIXTURE_ORIGINS: &[&str] = &["synthetic", "generated", "third-party"];
/// Repository-relative directory the checked-in fixtures live in. Paths are
/// validated against it so a fixture entry cannot point outside the tree, and
/// `capabilities.rs` embeds the same paths with `include_bytes!`.
const FIXTURE_DIRECTORY: &str = "utils/snapfab/fixtures/";

/// The formats snapfab generates fixtures for, and the capabilities those
/// fixtures exercise. Formats snapfab cannot encode are covered by pinned
/// fixtures instead and are recorded here too, with `fixtureSource: "pinned"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityManifest {
    pub schema_version: u32,
    /// Checked-in fixture files, referenced by the `pinned` formats.
    pub fixtures: Vec<PinnedFixture>,
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
    /// `generated` when snapfab encodes the format, `pinned` when checked-in
    /// bytes back its coverage.
    pub fixture_source: String,
    /// Ids from `fixtures`. Required for `pinned` formats, empty for
    /// `generated` ones.
    pub pinned_fixtures: Vec<String>,
}

/// A checked-in fixture file and its provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PinnedFixture {
    pub id: String,
    /// Repository-relative path to the checked-in file.
    pub path: String,
    /// Lowercase hex SHA-256 of the file's bytes.
    pub sha256: String,
    /// `synthetic` (built for this repository), `generated` (produced by a
    /// deterministic tool run) or `third-party` (taken from an upstream project).
    pub origin: String,
    /// Where the bytes come from: an upstream URL, or the construction used
    /// for a synthetic file.
    pub source: String,
    /// The upstream commit or release, or the version of the toolchain that
    /// produced the bytes.
    pub version: String,
    pub license: String,
    /// What a reader must observe in the file. Values are the rendered form
    /// the API returns, so EXIF dates use `YYYY-MM-DD HH:MM:SS`.
    pub expected_metadata: BTreeMap<String, String>,
    /// The failure class this fixture is expected to trigger. `none` for a
    /// fixture that has to index.
    pub intended_failure_class: String,
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

    let fixtures = validate_fixtures(manifest)?;

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
        if !FIXTURE_SOURCES.contains(&entry.fixture_source.as_str()) {
            return validation_error("fixture source is not recognized");
        }
        let mut pinned_seen = HashSet::new();
        for id in &entry.pinned_fixtures {
            validate_identifier(id, "pinned fixture ids")?;
            if !pinned_seen.insert(id.as_str()) {
                return validation_error("pinned fixture ids must be unique per format");
            }
            if !fixtures.contains(id.as_str()) {
                return validation_error("pinned fixture id does not resolve to a fixture");
            }
        }
        // A format is either generated or pinned, never both and never neither:
        // an unbacked capability claim is exactly what this manifest must not
        // contain.
        if entry.fixture_source == "pinned" && entry.pinned_fixtures.is_empty() {
            return validation_error("a pinned format must reference at least one fixture");
        }
        if entry.fixture_source == "generated" && !entry.pinned_fixtures.is_empty() {
            return validation_error("a generated format must not reference pinned fixtures");
        }
    }
    for fixture in &manifest.fixtures {
        if !manifest
            .formats
            .iter()
            .any(|entry| entry.pinned_fixtures.contains(&fixture.id))
        {
            return validation_error("fixture is not referenced by any format");
        }
    }
    Ok(())
}

/// Validate the checked-in fixtures and return their ids.
fn validate_fixtures(manifest: &CapabilityManifest) -> Result<HashSet<&str>, CapabilityError> {
    let invalid = |message: &str| Err(CapabilityError::Validation(message.to_string()));
    let mut ids = HashSet::new();
    let mut paths = HashSet::new();
    for fixture in &manifest.fixtures {
        validate_identifier(&fixture.id, "fixture ids")?;
        if !ids.insert(fixture.id.as_str()) {
            return invalid("fixture ids must be unique");
        }
        if !paths.insert(fixture.path.as_str()) {
            return invalid("fixture paths must be unique");
        }
        if !fixture.path.starts_with(FIXTURE_DIRECTORY)
            || fixture.path.contains("..")
            || fixture.path.ends_with('/')
        {
            return invalid(&format!(
                "fixture path must be a file under {FIXTURE_DIRECTORY}"
            ));
        }
        if !is_sha256_hex(&fixture.sha256) {
            return invalid("fixture sha256 must be 64 lowercase hex digits");
        }
        if !FIXTURE_ORIGINS.contains(&fixture.origin.as_str()) {
            return invalid("fixture origin is not recognized");
        }
        for (field, value) in [
            ("source", fixture.source.as_str()),
            ("version", fixture.version.as_str()),
            ("license", fixture.license.as_str()),
        ] {
            if value.trim().is_empty() {
                return invalid(&format!("fixture {field} must not be empty"));
            }
        }
        if fixture.expected_metadata.is_empty() {
            return invalid("fixture expected metadata must not be empty");
        }
        for (key, value) in &fixture.expected_metadata {
            if key.trim().is_empty() || value.trim().is_empty() {
                return invalid("fixture expected metadata keys and values must be set");
            }
        }
        if !FAILURE_CLASSES.contains(&fixture.intended_failure_class.as_str()) {
            return invalid("fixture intended failure class is not recognized");
        }
    }
    Ok(ids)
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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

    /// Look up a checked-in fixture by the id a `pinned` format references.
    pub fn fixture_by_id(&self, id: &str) -> Option<&PinnedFixture> {
        self.fixtures.iter().find(|fixture| fixture.id == id)
    }

    /// Resolve a checked-in fixture by its repository-relative path.
    pub fn fixture_by_path(&self, path: &str) -> Option<&PinnedFixture> {
        self.fixtures.iter().find(|fixture| fixture.path == path)
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

    /// Bytes of every checked-in fixture, keyed by the repository-relative
    /// path the manifest records. `include_bytes!` turns a missing or renamed
    /// fixture into a compile error, and
    /// `every_pinned_fixture_is_embedded_for_digest_verification` catches the
    /// other direction.
    fn embedded_fixtures() -> &'static [(&'static str, &'static [u8])] {
        &[
            (
                "utils/snapfab/fixtures/tiff/picasu-tiff-48x32-exif.tif",
                include_bytes!("../fixtures/tiff/picasu-tiff-48x32-exif.tif"),
            ),
            (
                "utils/snapfab/fixtures/webp/picasu-webp-48x32-exif.webp",
                include_bytes!("../fixtures/webp/picasu-webp-48x32-exif.webp"),
            ),
            (
                "utils/snapfab/fixtures/mp4/picasu-mp4-48x32-ffprobe.mp4",
                include_bytes!("../fixtures/mp4/picasu-mp4-48x32-ffprobe.mp4"),
            ),
            (
                "utils/snapfab/fixtures/mov/picasu-mov-48x32-ffprobe.mov",
                include_bytes!("../fixtures/mov/picasu-mov-48x32-ffprobe.mov"),
            ),
        ]
    }

    /// Bytes of the fixture registered under `id`.
    fn fixture_bytes(id: &str) -> &'static [u8] {
        let manifest = load_capabilities().expect("manifest should load");
        let path = &manifest
            .fixture_by_id(id)
            .unwrap_or_else(|| panic!("fixture {id} should be registered"))
            .path;
        embedded_fixtures()
            .iter()
            .find(|(candidate, _)| candidate == path)
            .unwrap_or_else(|| panic!("fixture {id} is not embedded for inspection"))
            .1
    }

    /// The `uuid` box header that carries an Adobe XMP packet in an ISO-BMFF
    /// container: box type `uuid` followed by the extended-type UUID
    /// `BE7ACFCB-97A9-42E8-9C71-999491E3AFAC` [ISO14496-12 4.3, Adobe XMP
    /// spec 1.0 §2]. Written verbatim, no byte order conversion.
    const ADOBE_XMP_UUID_BOX: &[u8] =
        b"uuid\xbe\x7a\xcf\xcb\x97\xa9\x42\xe8\x9c\x71\x99\x94\x91\xe3\xaf\xac";

    fn carries_adobe_xmp_uuid_box(bytes: &[u8]) -> bool {
        bytes
            .windows(ADOBE_XMP_UUID_BOX.len())
            .any(|window| window == ADOBE_XMP_UUID_BOX)
    }

    /// SHA-256 (FIPS 180-4) over `input`, lowercase hex.
    ///
    /// Implemented here rather than pulled in as a dependency: the only
    /// consumer is a fixture-integrity check, and a test-only hash does not
    /// justify a new direct dependency in a generator crate. Correctness is
    /// pinned to the published test vectors in
    /// `sha256_matches_published_test_vectors`.
    fn sha256_hex(input: &[u8]) -> String {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut state: [u32; 8] = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ];

        let mut message = input.to_vec();
        let bit_len = (input.len() as u64).wrapping_mul(8);
        message.push(0x80);
        while message.len() % 64 != 56 {
            message.push(0);
        }
        message.extend_from_slice(&bit_len.to_be_bytes());

        for block in message.chunks_exact(64) {
            let mut w = [0u32; 64];
            for (index, word) in block.chunks_exact(4).enumerate() {
                w[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
            }
            for index in 16..64 {
                let s0 = w[index - 15].rotate_right(7)
                    ^ w[index - 15].rotate_right(18)
                    ^ (w[index - 15] >> 3);
                let s1 = w[index - 2].rotate_right(17)
                    ^ w[index - 2].rotate_right(19)
                    ^ (w[index - 2] >> 10);
                w[index] = w[index - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[index - 7])
                    .wrapping_add(s1);
            }
            let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
            for index in 0..64 {
                let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
                let ch = (e & f) ^ ((!e) & g);
                let temp1 = h
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(K[index])
                    .wrapping_add(w[index]);
                let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
                let maj = (a & b) ^ (a & c) ^ (b & c);
                let temp2 = s0.wrapping_add(maj);
                h = g;
                g = f;
                f = e;
                e = d.wrapping_add(temp1);
                d = c;
                c = b;
                b = a;
                a = temp1.wrapping_add(temp2);
            }
            for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
                *slot = slot.wrapping_add(value);
            }
        }

        state.iter().map(|word| format!("{word:08x}")).collect()
    }

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
            "expectedFailureClasses": ["none"],
            "fixtureSource": "generated",
            "pinnedFixtures": []
        }"#
        .to_string()
    }

    /// A single valid checked-in fixture, `origin` aside: the caller replaces
    /// the fields it wants to exercise.
    fn valid_fixture() -> String {
        r#"{
            "id": "tiff-48x32-exif",
            "path": "utils/snapfab/fixtures/tiff/picasu-tiff-48x32-exif.tif",
            "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
            "origin": "synthetic",
            "source": "constructed for this repository",
            "version": "picasu fixture revision 1",
            "license": "MIT",
            "expectedMetadata": {"width": "48"},
            "intendedFailureClass": "none"
        }"#
        .to_string()
    }

    /// A `pinned` format entry that references `valid_fixture`.
    fn pinned_entry() -> String {
        valid_entry()
            .replace(r#""format": "png""#, r#""format": "tiff""#)
            .replace(r#"["png"]"#, r#"["tif", "tiff"]"#)
            .replace(
                r#""fixtureSource": "generated""#,
                r#""fixtureSource": "pinned""#,
            )
            .replace(
                r#""pinnedFixtures": []"#,
                r#""pinnedFixtures": ["tiff-48x32-exif"]"#,
            )
    }

    fn manifest_with(entries: &[&str]) -> String {
        let entries = entries.join(",");
        format!(r#"{{"schemaVersion": 1, "fixtures": [], "formats": [{entries}]}}"#)
    }

    fn manifest_with_fixtures(fixtures: &[&str], entries: &[&str]) -> String {
        let fixtures = fixtures.join(",");
        let entries = entries.join(",");
        format!(r#"{{"schemaVersion": 1, "fixtures": [{fixtures}], "formats": [{entries}]}}"#)
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

    /// The manifest lists exactly the formats this repository covers with a
    /// verified fixture: four still images plus the two ISO-BMFF video
    /// containers. A format may only enter this list together with its fixture,
    /// so the list is the manifest's coverage boundary in both directions.
    #[test]
    fn repository_manifest_declares_every_covered_format() {
        let manifest = load_capabilities().expect("manifest should load");

        assert_eq!(manifest.schema_version, 1);
        assert_eq!(
            manifest
                .formats
                .iter()
                .map(|entry| entry.format.as_str())
                .collect::<Vec<_>>(),
            ["jpeg", "png", "tiff", "webp", "mp4", "mov"]
        );
    }

    /// HEIF/HEIC and AVIF are rejected by the extension allowlist and out of
    /// scope for metadata coverage, so the manifest must not gain a claim for
    /// them. This is the manifest-side half of that policy.
    #[test]
    fn repository_manifest_claims_nothing_for_rejected_formats() {
        let manifest = load_capabilities().expect("manifest should load");

        for extension in ["heic", "heif", "avif"] {
            assert!(
                manifest.capability_for_extension(extension).is_none(),
                "{extension} is rejected by the backend and must not be declared"
            );
        }
    }

    /// The formats snapfab cannot encode are covered by checked-in bytes
    /// instead, and the manifest has to say so rather than leave the fixture
    /// source implicit.
    #[test]
    fn repository_manifest_declares_a_fixture_source_per_format() {
        let manifest = load_capabilities().expect("manifest should load");

        let sources = manifest
            .formats
            .iter()
            .map(|entry| (entry.format.as_str(), entry.fixture_source.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(
            sources,
            [
                ("jpeg", "generated"),
                ("png", "generated"),
                ("tiff", "pinned"),
                ("webp", "pinned"),
                ("mp4", "pinned"),
                ("mov", "pinned"),
            ]
        );
    }

    #[test]
    fn repository_manifest_pins_a_tiff_and_a_webp_fixture() {
        let manifest = load_capabilities().expect("manifest should load");

        for (format, fixture) in [("tiff", "tiff-48x32-exif"), ("webp", "webp-48x32-exif")] {
            let entry = manifest
                .capability_for_format(format)
                .unwrap_or_else(|| panic!("{format} should be declared"));
            assert_eq!(
                entry.pinned_fixtures,
                [fixture],
                "{format} should be covered by the pinned {fixture} fixture"
            );
            let pinned = manifest
                .fixture_by_id(fixture)
                .unwrap_or_else(|| panic!("{fixture} should be registered"));
            let extension = pinned
                .path
                .rsplit_once('.')
                .map(|(_, extension)| extension)
                .unwrap_or_else(|| panic!("{fixture} path needs an extension"));
            assert!(
                entry.extensions.iter().any(|value| value == extension),
                "{fixture} carries .{extension}, which {format} does not accept: {:?}",
                entry.extensions
            );
            assert_eq!(pinned.intended_failure_class, "none");
            assert_eq!(
                pinned.expected_metadata.get("width").map(String::as_str),
                Some("48")
            );
            assert_eq!(
                pinned.expected_metadata.get("height").map(String::as_str),
                Some("32")
            );
        }
    }

    /// The two video containers are covered by checked-in bytes produced by one
    /// recorded ffmpeg invocation, and the manifest has to carry the
    /// reproducibility record for them: the command, the tool version, the
    /// ffprobe fields a reader must observe, and the one `ftyp` brand that
    /// tells the two containers apart.
    #[test]
    fn repository_manifest_pins_a_deterministic_ffmpeg_video_fixture() {
        let manifest = load_capabilities().expect("manifest should load");

        for (format, fixture, major_brand) in [
            ("mp4", "mp4-48x32-ffprobe", "isom"),
            ("mov", "mov-48x32-ffprobe", "qt  "),
        ] {
            let entry = manifest
                .capability_for_format(format)
                .unwrap_or_else(|| panic!("{format} should be declared"));
            assert_eq!(
                entry.pinned_fixtures,
                [fixture],
                "{format} should be covered by the pinned {fixture} fixture"
            );

            let pinned = manifest
                .fixture_by_id(fixture)
                .unwrap_or_else(|| panic!("{fixture} should be registered"));
            assert_eq!(
                pinned.origin, "generated",
                "{fixture} is ffmpeg output, neither hand-built nor upstream test data"
            );
            assert!(
                pinned.source.contains("ffmpeg -nostdin"),
                "{fixture} must record the generation command so the bytes can be \
                 re-derived, got: {}",
                pinned.source
            );
            assert!(
                pinned.source.contains("+bitexact"),
                "{fixture} must record the flags that pin the bytes, got: {}",
                pinned.source
            );
            assert!(
                pinned.version.contains("ffmpeg "),
                "{fixture} must record the ffmpeg version that produced it, got: {}",
                pinned.version
            );
            assert_eq!(pinned.intended_failure_class, "none");

            for (key, value) in [
                ("width", "48"),
                ("height", "32"),
                ("thumbnail", "generated"),
                // ffprobe reports the whole ISO-BMFF group as one `format_name`
                // for both fixtures; `TAG:major_brand` read from the `ftyp` box
                // is the only field that separates them.
                ("ffprobe.format_name", "mov,mp4,m4a,3gp,3g2,mj2"),
                ("ffprobe.TAG:major_brand", major_brand),
                ("ffprobe.codec_name", "h264"),
                ("ffprobe.codec_type", "video"),
                ("ffprobe.pix_fmt", "yuv420p"),
            ] {
                assert_eq!(
                    pinned.expected_metadata.get(key).map(String::as_str),
                    Some(value),
                    "{fixture} expectedMetadata[{key}]"
                );
            }
        }
    }

    /// Video metadata comes from ffprobe, and the manifest has to keep that
    /// contract separate from a packet carried in the bytes. The pinned fixture
    /// has no EXIF block the backend reads and no XMP packet, so neither may be
    /// claimed; `container: [probe]` is the honest positive claim, and
    /// `xmp: [sidecar]` holds because `xmp.rs` resolves sidecars with no format
    /// dispatch at all.
    #[test]
    fn repository_manifest_separates_probed_container_metadata_from_embedded_xmp() {
        let manifest = load_capabilities().expect("manifest should load");

        for format in ["mp4", "mov"] {
            let entry = manifest
                .capability_for_format(format)
                .unwrap_or_else(|| panic!("{format} should be declared"));

            assert_eq!(
                entry.metadata_fields.get("container").map(Vec::as_slice),
                Some(&["probe".to_string()][..]),
                "{format} metadata comes from ffprobe, not from bytes in the file"
            );
            assert!(
                !entry.metadata_fields.contains_key("exif"),
                "{format} has no EXIF block the backend reads; the exifVec the API \
                 returns for a video is ffprobe output, not EXIF"
            );
            assert_eq!(
                entry.metadata_fields.get("xmp").map(Vec::as_slice),
                Some(&["sidecar".to_string()][..]),
                "{format} sidecar XMP is read by xmp.rs, which dispatches on no format"
            );
            assert_eq!(
                entry.unsupported_metadata_fields,
                ["exif:embedded", "xmp:embedded"],
                "{format}: UUID-box XMP stays unclaimed until a fixture carries one"
            );
        }
    }

    /// `xmp:embedded` is recorded as unsupported for mp4/mov, so the reason is
    /// pinned against the checked-in bytes rather than left as prose. The
    /// positive control keeps this from passing for the wrong reason: a scanner
    /// that could never recognise a uuid box would also report "absent".
    #[test]
    fn pinned_video_fixtures_carry_no_embedded_xmp_packet() {
        let mut with_uuid_box = ADOBE_XMP_UUID_BOX.to_vec();
        with_uuid_box.extend_from_slice(
            b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
              xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description \
              xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:subject><rdf:Bag>\
              <rdf:li>e2e</rdf:li></rdf:Bag></dc:subject></rdf:Description></rdf:RDF></x:xmpmeta>",
        );
        assert!(
            carries_adobe_xmp_uuid_box(&with_uuid_box),
            "positive control: the uuid-box header must be recognisable"
        );

        for id in ["mp4-48x32-ffprobe", "mov-48x32-ffprobe"] {
            let bytes = fixture_bytes(id);
            assert!(
                !carries_adobe_xmp_uuid_box(bytes),
                "{id} carries an Adobe XMP uuid box, so xmp:embedded would be claimable \
                 and `xmp:embedded` must move out of unsupportedMetadataFields"
            );
            for marker in [b"<x:xmpmeta".as_slice(), b"<dc:subject>".as_slice()] {
                assert!(
                    !bytes.windows(marker.len()).any(|w| w == marker),
                    "{id} carries the XMP marker {marker:?} verbatim"
                );
            }
        }
    }

    /// The declared `contentSignature` of each video format is read back out of
    /// its own fixture. Both signatures are the `ftyp` box header at offset 4
    /// (box size occupies 0..4) plus the major brand that follows it, and the
    /// brand is the only structural difference between the two files — so a
    /// fixture swapped for the other container, or a signature that drifts from
    /// the bytes, both fail here.
    #[test]
    fn video_content_signatures_match_the_brand_in_the_pinned_bytes() {
        let manifest = load_capabilities().expect("manifest should load");

        for (format, fixture, brand) in [
            ("mp4", "mp4-48x32-ffprobe", b"isom".as_slice()),
            ("mov", "mov-48x32-ffprobe", b"qt  ".as_slice()),
        ] {
            let signature = &manifest
                .capability_for_format(format)
                .unwrap_or_else(|| panic!("{format} should be declared"))
                .content_signature;

            assert_eq!(signature.offset, 4, "{format}: `ftyp` starts at byte 4");
            assert_eq!(
                signature.bytes().as_slice(),
                [b"ftyp".as_slice(), brand].concat(),
                "{format} contentSignature should be `ftyp` plus its major brand"
            );

            let bytes = fixture_bytes(fixture);
            let at = signature.offset;
            let expected = signature.bytes();
            assert!(
                bytes
                    .get(at..at + expected.len())
                    .is_some_and(|window| window == expected),
                "{format} signature {} does not match the bytes of {fixture} at offset {at}: \
                 found {:?}",
                signature.bytes_hex,
                bytes.get(at..at + expected.len())
            );
        }
    }

    #[test]
    fn repository_manifest_claims_exif_for_tiff_and_webp() {
        let manifest = load_capabilities().expect("manifest should load");

        for format in ["tiff", "webp"] {
            let entry = manifest
                .capability_for_format(format)
                .expect("format should be declared");
            assert!(
                entry.metadata_fields["exif"].contains(&"embedded".to_string()),
                "{format} should claim embedded EXIF: the pinned fixture carries an \
                 Exif IFD and kamadak-exif reads TIFF blocks from both containers"
            );
            assert_eq!(
                entry.unsupported_metadata_fields,
                ["xmp:embedded"],
                "{format} embedded XMP is not covered by a fixture yet"
            );
        }
    }

    /// Every checked-in fixture's digest is verified against the bytes on disk.
    /// `include_bytes!` makes a missing or renamed fixture a compile error, so
    /// this test cannot silently skip one: it fails instead.
    #[test]
    fn pinned_fixture_digests_match_the_checked_in_bytes() {
        let manifest = load_capabilities().expect("manifest should load");
        let embedded = embedded_fixtures();

        for pinned in &manifest.fixtures {
            let bytes = embedded
                .iter()
                .find(|(path, _)| *path == pinned.path)
                .unwrap_or_else(|| {
                    panic!(
                        "fixture {} is declared at {} but not embedded for digest \
                         verification",
                        pinned.id, pinned.path
                    )
                })
                .1;
            assert_eq!(
                sha256_hex(bytes),
                pinned.sha256,
                "fixture {} does not match its recorded SHA-256",
                pinned.id
            );
        }
    }

    /// A fixture nobody references is provenance data that nothing maintains.
    #[test]
    fn every_pinned_fixture_is_referenced_by_a_format() {
        let manifest = load_capabilities().expect("manifest should load");

        let referenced = manifest
            .formats
            .iter()
            .flat_map(|entry| entry.pinned_fixtures.iter())
            .map(String::as_str)
            .collect::<Vec<_>>();

        for pinned in &manifest.fixtures {
            assert!(
                referenced.contains(&pinned.id.as_str()),
                "fixture {} is not referenced by any format",
                pinned.id
            );
        }
    }

    #[test]
    fn sha256_matches_published_test_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // 56 bytes: exercises the padding boundary where a message is one
        // byte short of needing a second block.
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
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
        assert_eq!(
            manifest
                .capability_for_extension("tif")
                .expect("tiff alias should resolve")
                .format,
            "tiff"
        );
        assert!(manifest.capability_for_extension("jpegg").is_none());
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
        let error = parse_manifest(r#"{"schemaVersion": 1, "fixtures": [], "formats": []}"#)
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
        // `container`/`probe` are the vocabulary the video formats use for
        // ffprobe-derived metadata. The field and the source are each closed
        // lists, so a misspelling of either is rejected like any other value —
        // `conatiner` and `inline` must not slip through as valid claims.
        assert_validation_error(&[
            &valid_entry().replace(r#"{"exif": ["embedded"]}"#, r#"{"conatiner": ["probe"]}"#)
        ]);
        assert_validation_error(&[
            &valid_entry().replace(r#"{"exif": ["embedded"]}"#, r#"{"container": ["inline"]}"#)
        ]);
        assert_validation_error(&[
            &valid_entry().replace(r#"{"exif": ["embedded"]}"#, r#"{"container": []}"#)
        ]);
    }

    /// The probed-container claim and its exclusion are mutually exclusive, and
    /// so is the same pair for the fields that already existed. A container
    /// cannot be both read by ffprobe and unsupported.
    #[test]
    fn a_probed_container_field_cannot_be_both_supported_and_unsupported() {
        let claiming = |unsupported: &str| {
            valid_entry()
                .replace(r#"{"exif": ["embedded"]}"#, r#"{"container": ["probe"]}"#)
                .replace(
                    r#""unsupportedMetadataFields": []"#,
                    &format!(r#""unsupportedMetadataFields": {unsupported}"#),
                )
        };

        // Positive control, and the proof that `container`/`probe` is itself an
        // accepted vocabulary: without it the rejection below could be caused
        // by the unrecognized pair rather than by the contradiction.
        assert_valid(&manifest_with(&[&claiming("[]")]));

        assert_validation_error(&[&claiming(r#"["container:probe"]"#)]);
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
                r#""metadataFields": {"exif": ["embedded"]}"#,
                r#""metadataFields": {"container": ["guess"]}"#,
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

    /// Every declared format claims sidecar XMP. `xmp.rs` resolves sidecars
    /// without any format dispatch, so the claim is format-independent; the
    /// per-format scenarios are what keep it honest.
    #[test]
    fn repository_manifest_claims_sidecar_xmp_for_every_declared_format() {
        let manifest = load_capabilities().expect("manifest should load");

        for entry in &manifest.formats {
            assert!(
                entry.metadata_fields["xmp"].contains(&"sidecar".to_string()),
                "{} should claim sidecar XMP",
                entry.format
            );
        }
    }

    /// A `pinned` format is pinned *because* snapfab cannot encode it. If a
    /// format gains an encoder, this fails and the manifest entry has to be
    /// re-decided instead of quietly keeping two sources.
    #[test]
    fn repository_manifest_pins_exactly_the_formats_snapfab_cannot_encode() {
        let manifest = load_capabilities().expect("manifest should load");

        for entry in &manifest.formats {
            let generatable = crate::test_image::ImageFormat::from_name(&entry.format).is_some();
            assert_eq!(
                entry.fixture_source == "pinned",
                !generatable,
                "{} declares fixtureSource {} but snapfab {} encode it",
                entry.format,
                entry.fixture_source,
                if generatable { "can" } else { "cannot" }
            );
        }
    }

    /// Negative coverage for the fixture-source and pinned-fixture rules.
    /// `pinned_entry`/`valid_fixture` are the positive control: the pair
    /// together is accepted by `a_pinned_format_with_a_registered_fixture_is_accepted`.
    ///
    /// Each case names the fixture list it needs. That is deliberate: a case
    /// that leaves a registered fixture unreferenced is rejected by the
    /// referential rule too, which would mask the rule under test.
    #[test]
    fn fixture_source_and_pinned_fixture_rules_are_validated() {
        let other_fixture = || {
            valid_fixture()
                .replace("tiff-48x32-exif", "tiff-48x32-other")
                .replace("picasu-tiff-48x32-exif.tif", "picasu-tiff-48x32-other.tif")
        };
        // (label, fixtures, format entry, distinguishing error message)
        let cases = [
            (
                "unknown fixture source",
                vec![valid_fixture()],
                pinned_entry().replace(
                    r#""fixtureSource": "pinned""#,
                    r#""fixtureSource": "vendored""#,
                ),
                "fixture source is not recognized",
            ),
            (
                // A pinned format with no fixture is an unbacked claim.
                "pinned format referencing no fixture",
                vec![],
                pinned_entry().replace(
                    r#""pinnedFixtures": ["tiff-48x32-exif"]"#,
                    r#""pinnedFixtures": []"#,
                ),
                "pinned format must reference at least one fixture",
            ),
            (
                "generated format referencing pinned fixtures",
                vec![valid_fixture()],
                valid_entry().replace(
                    r#""pinnedFixtures": []"#,
                    r#""pinnedFixtures": ["tiff-48x32-exif"]"#,
                ),
                "generated format must not reference pinned fixtures",
            ),
            (
                "dangling pinned fixture id",
                vec![valid_fixture(), other_fixture()],
                pinned_entry().replace(
                    r#""pinnedFixtures": ["tiff-48x32-exif"]"#,
                    r#""pinnedFixtures": ["tiff-48x32-other", "tiff-48x32-missing"]"#,
                ),
                "pinned fixture id does not resolve",
            ),
            (
                // The identifier check and the referential check overlap, so
                // this case is pinned by the message it must produce.
                "mixed-case pinned fixture reference",
                vec![valid_fixture()],
                pinned_entry().replace(
                    r#""pinnedFixtures": ["tiff-48x32-exif"]"#,
                    r#""pinnedFixtures": ["tiff-48x32-exif", "TIFF-48x32-exif"]"#,
                ),
                "pinned fixture ids must be non-empty lowercase",
            ),
        ];

        for (label, fixtures, entry, expected) in cases {
            let fixture_refs = fixtures.iter().map(String::as_str).collect::<Vec<_>>();
            let entry_refs = fixture_refs;
            let json = manifest_with_fixtures(&entry_refs, &[&entry]);
            let error = parse_manifest(&json).expect_err(&format!("{label} should be rejected"));
            assert!(
                matches!(error, CapabilityError::Validation(_)),
                "expected a validation error for {label}, got: {error:?}"
            );
            assert!(
                error.to_string().contains(expected),
                "{label}: expected an error mentioning {expected:?}, got: {error}"
            );
        }
    }

    /// Negative coverage for the checked-in fixture record itself: provenance a
    /// consumer cannot verify is not provenance.
    #[test]
    fn pinned_fixture_records_are_validated() {
        let with_field = |field: &str, literal: &str| {
            let fixture = valid_fixture();
            let start = fixture.find(&format!("\"{field}\"")).expect("field exists");
            let value_start = start + field.len() + 3;
            let value_end = value_start + fixture[value_start..].find([',', '\n']).unwrap();
            let mut out = fixture.clone();
            out.replace_range(value_start..value_end, literal);
            out
        };

        let cases = [
            with_field("sha256", &format!("\"{}\"", "g".repeat(64))), // not hex
            with_field("sha256", &format!("\"{}\"", "A".repeat(64))), // uppercase
            with_field("sha256", &format!("\"{}\"", "a".repeat(63))), // too short
            with_field("origin", "\"borrowed\""),                     // unknown origin
            with_field("source", "\"  \""),                           // blank provenance
            with_field("version", "\"\""),                            // blank version
            with_field("license", "\"\""),                            // blank license
            with_field("expectedMetadata", "{}"),                     // no expectation
            with_field("intendedFailureClass", "\"flaky\""),          // unknown class
            valid_fixture().replace(
                r#""path": "utils/snapfab/fixtures/tiff/picasu-tiff-48x32-exif.tif""#,
                r#""path": "../../etc/passwd""#,
            ),
            valid_fixture().replace(
                r#""path": "utils/snapfab/fixtures/tiff/picasu-tiff-48x32-exif.tif""#,
                r#""path": "/etc/passwd""#,
            ),
            valid_fixture().replace(
                r#""path": "utils/snapfab/fixtures/tiff/picasu-tiff-48x32-exif.tif""#,
                r#""path": "utils/snapfab/fixtures/tiff/"#,
            ),
        ];

        for fixture in &cases {
            let json = manifest_with_fixtures(&[fixture], &[&pinned_entry()]);
            let error = parse_manifest(&json).expect_err("manifest should be rejected");
            assert!(
                matches!(
                    error,
                    CapabilityError::Validation(_) | CapabilityError::Parse(_)
                ),
                "expected a rejection, got: {error:?}\ninput: {json}"
            );
        }

        // A fixture id is checked as an identifier in its own right, so a
        // mixed-case id is rejected even when the format references exactly
        // that id and the referential rule is satisfied.
        let mixed_case = valid_fixture().replace("\"tiff-48x32-exif\"", "\"TIFF-48x32-exif\"");
        let entry = pinned_entry().replace("tiff-48x32-exif", "TIFF-48x32-exif");
        let json = manifest_with_fixtures(&[&mixed_case], &[&entry]);
        let error = parse_manifest(&json).expect_err("a mixed-case fixture id should fail");
        assert!(
            error
                .to_string()
                .contains("fixture ids must be non-empty lowercase"),
            "expected the identifier rule, got: {error}"
        );

        // Two records may not claim the same identity. Each case moves exactly
        // one field and references every record it declares, so the rule named
        // by `expected` is the only one that can fire.
        for (second, references, expected) in [
            (
                valid_fixture().replace(
                    "picasu-tiff-48x32-exif.tif",
                    "picasu-tiff-48x32-duplicate.tif",
                ),
                vec!["tiff-48x32-exif"],
                "fixture ids must be unique",
            ),
            (
                valid_fixture().replace("\"tiff-48x32-exif\"", "\"tiff-48x32-other\""),
                vec!["tiff-48x32-exif", "tiff-48x32-other"],
                "fixture paths must be unique",
            ),
        ] {
            let pinned = references
                .iter()
                .map(|id| format!("\"{id}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let entry = pinned_entry().replace(
                r#""pinnedFixtures": ["tiff-48x32-exif"]"#,
                &format!(r#""pinnedFixtures": [{pinned}]"#),
            );
            let json = manifest_with_fixtures(&[&valid_fixture(), &second], &[&entry]);
            let error = parse_manifest(&json).expect_err("a duplicate fixture should fail");
            assert!(
                error.to_string().contains(expected),
                "expected {expected:?}, got: {error}"
            );
        }
    }

    /// A fixture no format references is provenance nothing maintains.
    #[test]
    fn an_unreferenced_fixture_is_rejected() {
        let json = manifest_with_fixtures(&[&valid_fixture()], &[&valid_entry()]);
        let error = parse_manifest(&json).expect_err("an unreferenced fixture should fail");

        assert!(matches!(error, CapabilityError::Validation(_)));
    }

    /// Positive control for the two tests above: a pinned format with a
    /// registered fixture is a complete, accepted entry.
    #[test]
    fn a_pinned_format_with_a_registered_fixture_is_accepted() {
        let json = manifest_with_fixtures(&[&valid_fixture()], &[&pinned_entry()]);

        assert_valid(&json);
    }

    #[test]
    fn capability_errors_render_a_descriptive_message() {
        let error = parse_manifest(r#"{"schemaVersion": 1, "fixtures": [], "formats": []}"#)
            .expect_err("an empty manifest should fail");

        assert_eq!(
            error.to_string(),
            "capability manifest validation error: manifest must contain at least one format"
        );
        let boxed: Box<dyn std::error::Error> = Box::new(error);
        assert!(boxed.to_string().contains("validation error"));
    }
}
