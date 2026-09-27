//! Deterministic format selection for randomized scenarios.
//!
//! A randomized scenario randomizes its *input* (which format the file under
//! test is) and never its expectations. That is only sound if the chosen format
//! always comes with a verified fixture and a declared capability contract, so
//! selection is a filter over the capability manifest rather than a free choice:
//!
//! * the format must be declared in `capabilities.json`,
//! * it must have a *verified fixture* — either snapfab encodes it
//!   (`fixtureSource: "generated"`) or it names a checked-in fixture that
//!   resolves (`fixtureSource: "pinned"`),
//! * and its expected failure classes must stay within what a positive
//!   upload → index → metadata → delete flow tolerates.
//!
//! HEIF/HEIC and AVIF are not excluded by a hand-written denylist here. They
//! are excluded by the fixture rule: the manifest declares no encoder for them
//! and no checked-in fixture, so neither shape they could be written in is
//! randomizable. A manifest that *did* back them with a fixture would be caught
//! by the backend's cross-check that every randomizable extension is accepted by
//! the upload allowlist, which is where that policy is owned.
//!
//! The same seed and the same manifest always resolve to the same format, and
//! the eligible list is sorted by format name, so reordering the manifest does
//! not silently re-map every recorded seed. `crate::capabilities` is the only
//! source of truth for what a format claims; this module decides nothing about
//! metadata itself.

use std::fmt;

use crate::capabilities::{CapabilityManifest, FormatCapability};
use crate::test_image::ImageFormat;

/// The failure classes a positive flow tolerates.
///
/// A randomized scenario asserts that a file indexes, serves metadata, and
/// deletes. A format that is expected to fail (a truncated-image class, say)
/// cannot carry such a scenario, so it never enters the eligible set. The set is
/// deliberately not caller-configurable: a second tolerance belongs to a flow
/// that needs it, together with the test that pins why.
const POSITIVE_FLOW_FAILURE_CLASSES: &[&str] = &["none"];

/// How a harness materialises the bytes of a randomizable format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixturePlan {
    /// snapfab encodes the format, so the harness generates the file.
    Generate,
    /// The format is covered by checked-in bytes, so the harness copies `id` —
    /// an entry of the manifest's `fixtures` array — into place.
    CopyFixture { id: String },
}

/// A manifest format a randomized scenario may be handed, with the extension the
/// scenario's file gets and the plan that produces its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RandomizableFormat {
    pub format: String,
    /// The format's first declared extension. Extensions are unranked, so the
    /// manifest's order is the tie-breaker; it is stable because the manifest is
    /// a checked-in file, and it is recorded in the seed manifest so a recorded
    /// scenario filename cannot drift unnoticed.
    pub extension: String,
    pub plan: FixturePlan,
}

impl RandomizableFormat {
    /// A one-line, greppable record of what a seed resolved to, for the test
    /// log. `cargo test` captures stdout, so the harness also puts this line in
    /// the failure message of a randomized run — a reader then finds the seed
    /// both with `--nocapture` and in the panic output of a red run.
    pub fn log_line(&self, seed: u64) -> String {
        match &self.plan {
            FixturePlan::Generate => format!(
                "seed={seed} format={} ext={} source=generated",
                self.format, self.extension
            ),
            FixturePlan::CopyFixture { id } => format!(
                "seed={seed} format={} ext={} source=pinned fixture={id}",
                self.format, self.extension
            ),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SelectionError {
    /// No declared format has a verified fixture a positive flow can use. The
    /// per-format reasons are carried along: an empty eligible set is a manifest
    /// problem, and the reason is what makes it diagnosable.
    NoEligibleFormat { reasons: Vec<String> },
}

impl fmt::Display for SelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoEligibleFormat { reasons } => write!(
                formatter,
                "no manifest format is eligible for randomized selection: {}",
                reasons.join("; ")
            ),
        }
    }
}

impl std::error::Error for SelectionError {}

/// SplitMix64's mixing function (Steele, Lea & Flood, `splitmix64.c`).
///
/// The seed space is expanded with this rather than with `rand`: `SmallRng` and
/// friends are explicitly *not* reproducible across crate versions, and a
/// recorded seed has to keep resolving to the same format when a dependency
/// moves. The three constants are the ones `rand` itself uses in
/// `Xoshiro256PlusPlus::seed_from_u64`.
fn splitmix64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The formats a randomized scenario may pick, sorted by format name.
///
/// The sort is load-bearing: the seed indexes into this list, so a
/// manifest-order change would otherwise re-map every recorded seed.
pub fn randomizable_formats(manifest: &CapabilityManifest) -> Vec<RandomizableFormat> {
    let mut formats: Vec<RandomizableFormat> = manifest
        .formats
        .iter()
        .filter_map(|entry| classify(manifest, entry).ok())
        .collect();
    formats.sort_by(|left, right| left.format.cmp(&right.format));
    formats
}

/// Classify one manifest entry, or say why a positive flow cannot use it.
fn classify(
    manifest: &CapabilityManifest,
    entry: &FormatCapability,
) -> Result<RandomizableFormat, String> {
    let untolerated = entry
        .expected_failure_classes
        .iter()
        .filter(|class| !POSITIVE_FLOW_FAILURE_CLASSES.contains(&class.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !untolerated.is_empty() {
        return Err(format!(
            "{} is expected to fail ({untolerated:?}), which a positive flow does \
             not tolerate",
            entry.format
        ));
    }

    let plan = match entry.fixture_source.as_str() {
        "generated" => {
            if ImageFormat::from_name(&entry.format).is_none() {
                return Err(format!(
                    "{} claims generated fixtures, but snapfab cannot encode it, so \
                     it has no verified fixture",
                    entry.format
                ));
            }
            FixturePlan::Generate
        }
        "pinned" => {
            // The manifest rejects a pinned format with no fixture, so the first
            // id is present; the resolution check is what keeps a hand-edited
            // manifest from selecting a fixture the record does not vouch for.
            let id = entry
                .pinned_fixtures
                .first()
                .ok_or_else(|| format!("{} is pinned but names no fixture", entry.format))?;
            if manifest.fixture_by_id(id).is_none() {
                return Err(format!(
                    "{} pins fixture {id}, which the manifest does not register, so \
                     it has no verified fixture",
                    entry.format
                ));
            }
            FixturePlan::CopyFixture { id: id.clone() }
        }
        other => {
            return Err(format!(
                "{} has an unknown fixture source {other}",
                entry.format
            ));
        }
    };

    let extension = entry.extensions.first().ok_or_else(|| {
        format!(
            "{} declares no extension, so a scenario would have no filename",
            entry.format
        )
    })?;

    Ok(RandomizableFormat {
        format: entry.format.clone(),
        extension: extension.clone(),
        plan,
    })
}

/// The format `seed` resolves to.
///
/// `seed` is mixed with `splitmix64` and reduced modulo the eligible count, so
/// consecutive seeds land on unrelated formats instead of walking the list. The
/// modulo bias is below 1/6 of a step for any realistic eligible count and does
/// not matter here: the contract is reproducibility, not uniform draws.
pub fn select(
    seed: u64,
    manifest: &CapabilityManifest,
) -> Result<RandomizableFormat, SelectionError> {
    let formats = randomizable_formats(manifest);
    if formats.is_empty() {
        let reasons = manifest
            .formats
            .iter()
            .filter_map(|entry| classify(manifest, entry).err())
            .collect();
        return Err(SelectionError::NoEligibleFormat { reasons });
    }
    let index = (splitmix64(seed) % formats.len() as u64) as usize;
    Ok(formats[index].clone())
}

#[cfg(test)]
mod tests {
    use super::{
        FixturePlan, POSITIVE_FLOW_FAILURE_CLASSES, RandomizableFormat, SelectionError,
        randomizable_formats, select, splitmix64,
    };
    use crate::capabilities::{CapabilityManifest, load_capabilities, parse_manifest};

    /// The formats the checked-in manifest covers, with the extension a
    /// randomized scenario's file gets and the plan that produces its bytes.
    /// The extension is part of the contract: it is the filename the scenario
    /// writes, and the seed manifest records the resolution per seed.
    fn repository_expectations() -> [(&'static str, &'static str, FixturePlan); 6] {
        [
            ("jpeg", "jpg", FixturePlan::Generate),
            (
                "mov",
                "mov",
                FixturePlan::CopyFixture {
                    id: "mov-48x32-ffprobe".into(),
                },
            ),
            (
                "mp4",
                "mp4",
                FixturePlan::CopyFixture {
                    id: "mp4-48x32-ffprobe".into(),
                },
            ),
            ("png", "png", FixturePlan::Generate),
            (
                "tiff",
                "tif",
                FixturePlan::CopyFixture {
                    id: "tiff-48x32-exif".into(),
                },
            ),
            (
                "webp",
                "webp",
                FixturePlan::CopyFixture {
                    id: "webp-48x32-exif".into(),
                },
            ),
        ]
    }

    /// A minimal valid manifest entry the caller mutates with `String::replace`.
    fn entry() -> String {
        r#"{
            "format": "png",
            "extensions": ["png"],
            "contentSignature": {
                "offset": 0,
                "bytesHex": "89504e470d0a0a1a",
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

    /// A `pinned` entry that references `valid_fixture`.
    fn pinned_entry() -> String {
        entry()
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

    /// A checked-in fixture record the `pinned` entry resolves.
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

    fn manifest_with_fixtures(fixtures: &[&str], entries: &[&str]) -> String {
        format!(
            r#"{{"schemaVersion": 1, "fixtures": [{}], "formats": [{}]}}"#,
            fixtures.join(","),
            entries.join(",")
        )
    }

    /// The first seed in `0..1024` that resolves to `format`, so a test about the
    /// *log line* does not also depend on which format a hard-coded seed lands
    /// on. A format the selector never reaches is itself a failure.
    fn seed_resolving_to(manifest: &CapabilityManifest, format: &str) -> u64 {
        (0..1024)
            .find(|seed| select(*seed, manifest).is_ok_and(|chosen| chosen.format == format))
            .unwrap_or_else(|| panic!("no seed in 0..1024 selects {format}"))
    }

    /// The checked-in manifest with its `formats` array reversed.
    fn reversed_repository_manifest() -> CapabilityManifest {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities.json")).expect("manifest json");
        let mut formats = value["formats"].as_array().expect("formats array").clone();
        formats.reverse();
        value["formats"] = serde_json::Value::Array(formats);
        parse_manifest(&value.to_string()).expect("reversed manifest should stay valid")
    }

    /// A seed is a pure function of the seed and the eligible list: repeated
    /// calls and repeated parses resolve the same way. This is what makes a
    /// recorded seed replayable.
    #[test]
    fn the_same_seed_selects_the_same_format() {
        let first = load_capabilities().expect("manifest should load");
        let second = parse_manifest(include_str!("../capabilities.json"))
            .expect("a fresh parse of the same manifest should load");

        for seed in [0, 1, 42, 1017, u64::MAX] {
            let a = select(seed, &first).expect("a format should be eligible");
            let b = select(seed, &first).expect("a format should be eligible");
            let c = select(seed, &second).expect("a format should be eligible");
            assert_eq!(a, b, "seed {seed} must resolve the same way twice");
            assert_eq!(a, c, "seed {seed} must not depend on the manifest value");
        }
    }

    /// The eligible list is sorted, so reordering the manifest cannot re-map a
    /// recorded seed. Without the sort this fails the moment a format is added
    /// or moved in `capabilities.json`.
    #[test]
    fn selection_does_not_depend_on_manifest_order() {
        let forwards = load_capabilities().expect("manifest should load");
        let backwards = reversed_repository_manifest();

        for seed in 0..64 {
            assert_eq!(
                select(seed, &forwards).expect("eligible"),
                select(seed, &backwards).expect("eligible"),
                "seed {seed} resolved differently when the manifest order changed"
            );
        }
    }

    /// The eligible set is the manifest's whole coverage boundary: the six
    /// formats this repository has a verified fixture for, each with the
    /// extension its scenario file gets and the plan that produces the bytes.
    #[test]
    fn the_repository_manifest_yields_every_covered_format_with_its_plan() {
        let manifest = load_capabilities().expect("manifest should load");

        let expected = repository_expectations()
            .iter()
            .map(|(format, extension, plan)| RandomizableFormat {
                format: (*format).to_string(),
                extension: (*extension).to_string(),
                plan: plan.clone(),
            })
            .collect::<Vec<_>>();

        assert_eq!(randomizable_formats(&manifest), expected);
    }

    /// Every declared format is reachable, and no format dominates the seed
    /// space. A selection that ignores the seed, or mixes only its low bits,
    /// fails one half of this.
    #[test]
    fn many_seeds_reach_every_eligible_format_evenly() {
        let manifest = load_capabilities().expect("manifest should load");
        let eligible = randomizable_formats(&manifest);
        assert!(
            !eligible.is_empty(),
            "the repository manifest has eligible formats"
        );

        let samples = 512u64;
        let mut counts = std::collections::BTreeMap::new();
        for seed in 0..samples {
            let chosen = select(seed, &manifest).expect("eligible").format;
            *counts.entry(chosen).or_insert(0u32) += 1;
        }

        assert_eq!(
            counts.keys().cloned().collect::<Vec<_>>(),
            eligible
                .iter()
                .map(|entry| entry.format.clone())
                .collect::<Vec<_>>(),
            "seeds 0..{samples} must reach every eligible format"
        );
        for (format, count) in &counts {
            // A uniform split is samples/len; the bound below has a wide margin
            // under it, so a biased mix is caught without pinning the exact
            // distribution.
            assert!(
                u64::from(*count) * 4 >= samples / 3,
                "{format} was selected {count} times out of {samples}: the seed does \
                 not spread across the eligible set"
            );
        }
    }

    /// A format with no verified fixture is never selected. HEIF/HEIC and AVIF
    /// are the shapes the product rejects, and they are excluded by that rule
    /// rather than by a denylist: a manifest that declares one as `generated`
    /// claims an encoder snapfab does not have, so nothing backs the format's
    /// capability contract. (The other shape — declaring one as `pinned` without
    /// registering the fixture — is rejected even earlier, by manifest
    /// validation; `a_pinned_format_without_a_resolvable_fixture` covers the
    /// selector's own check against a manifest that got past it.)
    /// `png` in the same manifest is the positive control, so an exclusion cannot
    /// pass for the wrong reason (a manifest rejected whole).
    #[test]
    fn a_format_without_a_verified_fixture_is_never_selected() {
        let unsupported = |format: &str, extension: &str| {
            entry()
                .replace(r#""format": "png""#, &format!(r#""format": "{format}""#))
                .replace(r#"["png"]"#, &format!(r#"["{extension}"]"#))
        };
        let heif = unsupported("heif", "heif");
        let avif = unsupported("avif", "avif");

        let manifest = parse_manifest(&manifest_with_fixtures(&[], &[&entry(), &heif, &avif]))
            .expect("the manifest is structurally valid");

        assert_eq!(
            randomizable_formats(&manifest)
                .iter()
                .map(|format| format.format.as_str())
                .collect::<Vec<_>>(),
            ["png"],
            "only the format with a verified fixture is eligible"
        );
        for seed in 0..512 {
            let chosen = select(seed, &manifest).expect("png stays eligible");
            assert_eq!(
                chosen.format, "png",
                "seed {seed} selected {}: a format with no verified fixture must \
                 never be selected",
                chosen.format
            );
        }
    }

    /// The fixture-resolution rule is a guard of its own, not a restatement of
    /// manifest validation: a manifest whose `pinnedFixtures` no longer resolve
    /// (hand-edited, or validation loosened later) must drop that format from
    /// selection rather than hand a harness an id it cannot copy. The generated
    /// formats stay eligible, so the drop is per format.
    #[test]
    fn a_pinned_format_without_a_resolvable_fixture_is_never_selected() {
        let mut manifest = parse_manifest(&manifest_with_fixtures(
            &[&valid_fixture()],
            &[&entry(), &pinned_entry()],
        ))
        .expect("structurally valid");
        let tiff = manifest
            .formats
            .iter_mut()
            .find(|format| format.format == "tiff")
            .expect("tiff is declared");
        tiff.pinned_fixtures = vec!["tiff-48x32-missing".to_string()];

        let eligible = randomizable_formats(&manifest);
        assert_eq!(
            eligible
                .iter()
                .map(|format| format.format.as_str())
                .collect::<Vec<_>>(),
            ["png"],
            "a format whose pinned fixture does not resolve is not randomizable"
        );
        for seed in 0..64 {
            assert_eq!(select(seed, &manifest).expect("png").format, "png");
        }
    }

    /// A format expected to fail cannot carry a positive-flow scenario, so its
    /// declared failure classes keep it out of the eligible set. The `[none]`
    /// entry is the positive control.
    #[test]
    fn a_format_whose_failure_classes_exceed_a_positive_flow_is_never_selected() {
        assert_eq!(POSITIVE_FLOW_FAILURE_CLASSES, ["none"]);

        let with_failures = |classes: &str, format: &str| {
            entry()
                .replace(r#""format": "png""#, &format!(r#""format": "{format}""#))
                .replace(r#"["png"]"#, &format!(r#"["{format}"]"#))
                .replace(
                    r#""expectedFailureClasses": ["none"]"#,
                    &format!(r#""expectedFailureClasses": [{classes}]"#),
                )
        };
        let truncated = with_failures(r#""truncated""#, "png");
        let mixed = with_failures(r#""none", "corrupt_metadata""#, "tiff");

        let manifest = parse_manifest(&manifest_with_fixtures(&[], &[&truncated, &mixed]))
            .expect("structurally valid");

        assert!(
            randomizable_formats(&manifest).is_empty(),
            "a format expected to fail must not be randomizable"
        );
        for seed in 0..64 {
            assert!(matches!(
                select(seed, &manifest),
                Err(SelectionError::NoEligibleFormat { .. })
            ));
        }

        let tolerated = parse_manifest(&manifest_with_fixtures(
            &[],
            &[&with_failures(r#""none""#, "png")],
        ))
        .expect("structurally valid");
        assert_eq!(randomizable_formats(&tolerated).len(), 1);
    }

    /// An all-excluded manifest is an error, not a panic and not a silent
    /// `None`, and the message names every exclusion: the eligible set going
    /// empty is a manifest problem, and the reason is what makes it diagnosable.
    #[test]
    fn an_empty_eligible_set_reports_every_exclusion() {
        let unsupported = |format: &str, extensions: &str, failures: &str| {
            entry()
                .replace(r#""format": "png""#, &format!(r#""format": "{format}""#))
                .replace(r#"["png"]"#, extensions)
                .replace(
                    r#""expectedFailureClasses": ["none"]"#,
                    &format!(r#""expectedFailureClasses": {failures}"#),
                )
        };
        let heic = unsupported("heic", r#"["heic"]"#, r#"["none"]"#);
        let avif = unsupported("avif", r#"["avif"]"#, r#"["none", "signature_mismatch"]"#);
        let manifest = parse_manifest(&manifest_with_fixtures(&[], &[&heic, &avif]))
            .expect("structurally valid");

        let error = select(7, &manifest).expect_err("no format is eligible");
        let message = error.to_string();
        assert!(
            matches!(
                error,
                SelectionError::NoEligibleFormat { ref reasons } if reasons.len() == 2
            ),
            "both exclusions must be reported, got: {message}"
        );
        assert!(
            message.contains("heic") && message.contains("avif"),
            "the message must name every excluded format, got: {message}"
        );
    }

    /// The extension a scenario writes resolves back to the same format, so a
    /// renamed or reordered extension list cannot make a scenario write, say,
    /// `.tiff` bytes under a jpeg claim.
    #[test]
    fn the_selected_extension_resolves_back_to_its_format() {
        let manifest = load_capabilities().expect("manifest should load");

        for (format, extension, _) in repository_expectations() {
            let resolved = manifest
                .capability_for_extension(extension)
                .unwrap_or_else(|| panic!("{extension} should resolve"));
            assert_eq!(
                resolved.format, format,
                "the recorded extension for {format} must resolve back to {format}"
            );
            let selected = randomizable_formats(&manifest)
                .into_iter()
                .find(|candidate| candidate.format == format)
                .unwrap_or_else(|| panic!("{format} should be randomizable"));
            assert_eq!(
                selected.extension, extension,
                "{format} must be selected with its canonical extension"
            );
        }
    }

    /// A `pinned` format's plan names the fixture the manifest declares, and
    /// that fixture resolves in the manifest — otherwise the harness would copy
    /// bytes the capability record does not vouch for.
    #[test]
    fn a_pinned_format_plans_a_fixture_that_resolves() {
        let manifest = load_capabilities().expect("manifest should load");

        for declared in &manifest.formats {
            if declared.fixture_source != "pinned" {
                continue;
            }
            let selected = randomizable_formats(&manifest)
                .into_iter()
                .find(|candidate| candidate.format == declared.format)
                .unwrap_or_else(|| panic!("{} should be randomizable", declared.format));
            let FixturePlan::CopyFixture { id } = &selected.plan else {
                panic!(
                    "{} is pinned, so its plan must copy a fixture",
                    declared.format
                );
            };
            assert_eq!(
                *id, declared.pinned_fixtures[0],
                "{} must plan the fixture it declares",
                declared.format
            );
            assert!(
                manifest.fixture_by_id(id).is_some(),
                "{} plans a fixture that does not resolve",
                declared.format
            );
        }
    }

    /// The log line is how a reader learns which format a seed picked, so it has
    /// to carry the seed, the format, the extension the scenario writes, and the
    /// source the bytes come from.
    #[test]
    fn the_log_line_records_the_seed_the_format_and_its_source() {
        let manifest = load_capabilities().expect("manifest should load");

        let tiff_seed = seed_resolving_to(&manifest, "tiff");
        let line = select(tiff_seed, &manifest)
            .expect("eligible")
            .log_line(tiff_seed);
        for expected in [
            &format!("seed={tiff_seed}"),
            "format=tiff",
            "ext=tif",
            "source=pinned",
            "fixture=tiff-48x32-exif",
        ] {
            assert!(line.contains(expected), "{line} must contain {expected}");
        }

        let jpeg_seed = seed_resolving_to(&manifest, "jpeg");
        let line = select(jpeg_seed, &manifest)
            .expect("eligible")
            .log_line(jpeg_seed);
        for expected in [
            &format!("seed={jpeg_seed}"),
            "format=jpeg",
            "ext=jpg",
            "source=generated",
        ] {
            assert!(line.contains(expected), "{line} must contain {expected}");
        }
        assert!(
            !line.contains("fixture="),
            "a generated format has no fixture to name: {line}"
        );
    }

    /// The seed mixing function, pinned so a recorded seed's resolution cannot
    /// move silently. `splitmix64(0)` is also SplitMix64's first published
    /// output; the other values are regression anchors for this table.
    #[test]
    fn the_seed_mixing_function_is_pinned() {
        assert_eq!(splitmix64(0), 0xE220_A839_7B1D_CDAF);
        assert_eq!(splitmix64(1), 0x910A_2DEC_8902_5CC1);
        assert_eq!(splitmix64(2), 0x9758_35DE_1C97_56CE);
        assert_eq!(splitmix64(3), 0x1D0B_14E4_DB01_8FED);
        // Distinct inputs must not collapse onto one value; a mix that ignored
        // the seed's high bits would make the first and last equal.
        assert_ne!(splitmix64(0), splitmix64(1 << 32));
    }
}
