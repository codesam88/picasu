//! The fixed seed sets the randomized API scenarios run.
//!
//! A scenario that carries a `randomize:` block runs once per seed in the
//! requested set. The set names come from `backend/tests/seeds.json`, which also
//! records — per seed — the format that seed must resolve to. That golden table
//! is what makes a recorded seed meaningful: a change to the capability manifest
//! or to the selector's mapping fails `every_recorded_seed_resolves_to_the_format`
//! here instead of quietly re-pointing every randomized scenario at some other
//! format.
//!
//! Three seed sources, in the order they apply:
//!
//! 1. the set the scenario names (`randomize: {seeds: ci}`), which is the
//!    default and needs no environment at all;
//! 2. `PICASU_RANDOM_SEEDS=<set>`, for running a broader set (`nightly`) on a
//!    schedule without editing a scenario;
//! 3. `PICASU_RANDOM_SEEDS=<seed>[,<seed>...]`, for replaying the one seed a
//!    red run reported. Such a seed needs no entry in the manifest — that is
//!    what makes a failure log line actionable.
//!
//! The default path never reads the environment's effect into the checked-in
//! sets: `cargo test` with no `PICASU_RANDOM_SEEDS` runs exactly the `ci` seeds.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

/// The environment variable that widens or narrows the seeds a randomized
/// scenario runs.
pub const SEED_ENV_VAR: &str = "PICASU_RANDOM_SEEDS";

/// The set a scenario runs when it names none, and the set CI runs.
pub const DEFAULT_SET: &str = "ci";

/// The broader set, run on a schedule rather than on every change.
pub const NIGHTLY_SET: &str = "nightly";

const SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// The checked-in seed sets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeedManifest {
    pub schema_version: u32,
    /// What this file is for, for the human who opens it.
    pub note: String,
    pub sets: BTreeMap<String, SeedSet>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeedSet {
    /// The seeds a scenario in this set runs, in order.
    pub seeds: Vec<u64>,
    /// The format each seed must resolve to. One entry per seed: a missing or
    /// extra entry is a stale golden table, not a subset to be tolerated.
    pub resolves_to: Vec<SeedResolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeedResolution {
    pub seed: u64,
    pub format: String,
}

#[derive(Debug, PartialEq)]
pub enum SeedError {
    Parse(String),
    Validation(String),
    UnknownSet(String),
}

impl fmt::Display for SeedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(message) => write!(formatter, "seed manifest parse error: {message}"),
            Self::Validation(message) => {
                write!(formatter, "seed manifest validation error: {message}")
            }
            Self::UnknownSet(name) => write!(
                formatter,
                "seed set `{name}` is not declared in backend/tests/seeds.json"
            ),
        }
    }
}

impl std::error::Error for SeedError {}

static SEEDS: LazyLock<SeedManifest> =
    LazyLock::new(|| load_seed_manifest().expect("checked-in seed manifest must be valid"));

pub fn seed_manifest() -> &'static SeedManifest {
    &SEEDS
}

pub fn load_seed_manifest() -> Result<SeedManifest, SeedError> {
    parse_seed_manifest(include_str!("../../tests/seeds.json"))
}

pub fn parse_seed_manifest(input: &str) -> Result<SeedManifest, SeedError> {
    let manifest: SeedManifest =
        serde_json::from_str(input).map_err(|error| SeedError::Parse(error.to_string()))?;
    validate_seed_manifest(&manifest)?;
    Ok(manifest)
}

pub fn validate_seed_manifest(manifest: &SeedManifest) -> Result<(), SeedError> {
    if manifest.schema_version != SUPPORTED_SCHEMA_VERSION {
        return validation_error("unsupported schema version");
    }
    if manifest.note.trim().is_empty() {
        return validation_error("the seed manifest must explain itself in `note`");
    }
    if manifest.sets.is_empty() {
        return validation_error("the seed manifest must declare at least one set");
    }

    for (name, set) in &manifest.sets {
        if set.seeds.is_empty() {
            return validation_error(&format!("seed set `{name}` must not be empty"));
        }
        let unique: BTreeSet<u64> = set.seeds.iter().copied().collect();
        if unique.len() != set.seeds.len() {
            return validation_error(&format!("seed set `{name}` repeats a seed"));
        }

        // The golden table has to cover the set exactly: a seed without a
        // recorded format is an unrecorded resolution, and an extra entry is a
        // seed that is not run.
        let recorded: BTreeSet<u64> = set.resolves_to.iter().map(|entry| entry.seed).collect();
        if recorded.len() != set.resolves_to.len() {
            return validation_error(&format!(
                "seed set `{name}` records a format for one seed twice"
            ));
        }
        let seeds: BTreeSet<u64> = set.seeds.iter().copied().collect();
        if recorded != seeds {
            return validation_error(&format!(
                "seed set `{name}` records a format for exactly the seeds it runs"
            ));
        }
    }

    // The fixed CI set and the broader nightly set are both part of the
    // contract, and the nightly one has to be strictly broader: a nightly run
    // that repeats the CI seeds samples nothing new.
    let ci = manifest
        .sets
        .get(DEFAULT_SET)
        .ok_or_else(|| SeedError::Validation(format!("the `{DEFAULT_SET}` set is required")))?;
    let nightly = manifest
        .sets
        .get(NIGHTLY_SET)
        .ok_or_else(|| SeedError::Validation(format!("the `{NIGHTLY_SET}` set is required")))?;
    let ci_seeds: BTreeSet<u64> = ci.seeds.iter().copied().collect();
    if !ci_seeds.iter().all(|seed| nightly.seeds.contains(seed)) {
        return validation_error(&format!(
            "the `{NIGHTLY_SET}` set must contain every `{DEFAULT_SET}` seed"
        ));
    }
    if nightly.seeds.len() <= ci.seeds.len() {
        return validation_error(&format!(
            "the `{NIGHTLY_SET}` set must be broader than the `{DEFAULT_SET}` set"
        ));
    }
    Ok(())
}

fn validation_error(message: &str) -> Result<(), SeedError> {
    Err(SeedError::Validation(message.to_string()))
}

/// The seeds a randomized scenario runs.
///
/// `requested` is the set its `randomize:` block names. `override_value` is the
/// value of `PICASU_RANDOM_SEEDS`, or `None` when it is unset or blank: a set
/// name switches to that set, and a comma-separated list of seeds replaces the
/// set entirely, which is how a single reported seed is replayed.
pub fn active_seeds(
    manifest: &SeedManifest,
    requested: &str,
    override_value: Option<&str>,
) -> Result<Vec<u64>, SeedError> {
    let Some(override_value) = override_value.map(str::trim).filter(|v| !v.is_empty()) else {
        return seeds_of(manifest, requested);
    };

    if let Ok(seeds) = parse_seed_list(override_value) {
        return Ok(seeds);
    }
    seeds_of(manifest, override_value)
}

fn seeds_of(manifest: &SeedManifest, name: &str) -> Result<Vec<u64>, SeedError> {
    manifest
        .sets
        .get(name)
        .map(|set| set.seeds.clone())
        .ok_or_else(|| SeedError::UnknownSet(name.to_string()))
}

/// A comma-separated list of seeds, or the name of a set. The list is tried
/// first: a set name is never a valid number.
fn parse_seed_list(value: &str) -> Result<Vec<u64>, SeedError> {
    let seeds = value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<u64>()
                .map_err(|error| SeedError::Parse(format!("seed `{part}`: {error}")))
        })
        .collect::<Result<Vec<u64>, SeedError>>()?;
    if seeds.is_empty() {
        return Err(SeedError::Parse("no seeds in the override".to_string()));
    }
    Ok(seeds)
}

/// `PICASU_RANDOM_SEEDS` as the process sees it, treating blank as unset.
pub fn env_override() -> Option<String> {
    std::env::var(SEED_ENV_VAR)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_SET, NIGHTLY_SET, SEED_ENV_VAR, SeedError, SeedManifest, active_seeds,
        env_override, load_seed_manifest, parse_seed_manifest, seed_manifest,
    };
    use snapfab::selection::{randomizable_formats, select};
    use std::collections::BTreeMap;

    /// The checked-in manifest re-serialized with `patch` applied, for the
    /// structural validation cases. Returning the error keeps every case
    /// asserting the *specific* rule rather than "something was rejected".
    fn with_patch(patch: impl FnOnce(&mut serde_json::Value)) -> Result<SeedManifest, SeedError> {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/seeds.json")).expect("seed manifest");
        patch(&mut value);
        parse_seed_manifest(&value.to_string())
    }

    /// Positive control for the negative cases below.
    #[test]
    fn the_checked_in_seed_manifest_is_valid() {
        let manifest = load_seed_manifest().expect("the checked-in seed manifest must load");

        assert_eq!(manifest.schema_version, 1);
        assert!(manifest.sets.contains_key(DEFAULT_SET));
        assert!(manifest.sets.contains_key(NIGHTLY_SET));
        assert!(!manifest.note.trim().is_empty());
    }

    /// The golden table is the point of the file: a recorded seed has to
    /// resolve to the format the file says, through the real selector and the
    /// real capability manifest.
    #[test]
    fn every_recorded_seed_resolves_to_the_recorded_format() {
        let seeds = seed_manifest();
        let capabilities = snapfab::capabilities::capabilities();

        for (name, set) in &seeds.sets {
            for entry in &set.resolves_to {
                let selected = select(entry.seed, capabilities)
                    .unwrap_or_else(|error| panic!("seed {} in `{name}`: {error}", entry.seed));
                assert_eq!(
                    selected.format, entry.format,
                    "seed {} is recorded as resolving to {} but selects {}",
                    entry.seed, entry.format, selected.format
                );
            }
        }
    }

    /// CI has to reach every format the repository covers, otherwise adding a
    /// format to the capability manifest would ship without the randomized flow
    /// ever running on it. This is the test that fails when a new format needs a
    /// new CI seed.
    #[test]
    fn the_fixed_ci_seed_set_reaches_every_randomizable_format() {
        let capabilities = snapfab::capabilities::capabilities();
        let eligible: Vec<String> = randomizable_formats(capabilities)
            .into_iter()
            .map(|format| format.format)
            .collect();
        let mut reached: BTreeMap<String, usize> = BTreeMap::new();
        for seed in &seed_manifest().sets[DEFAULT_SET].seeds {
            *reached
                .entry(select(*seed, capabilities).expect("eligible").format)
                .or_insert(0) += 1;
        }

        assert_eq!(
            reached.keys().cloned().collect::<Vec<_>>(),
            eligible,
            "the `{DEFAULT_SET}` seed set must reach every randomizable format, so a \
             newly covered format is exercised on every change"
        );
    }

    /// The nightly set exists to sample combinations CI does not, and it must
    /// not be able to degenerate into the CI set.
    #[test]
    fn the_nightly_seed_set_is_broader_than_the_fixed_set() {
        let manifest = seed_manifest();
        let ci = &manifest.sets[DEFAULT_SET].seeds;
        let nightly = &manifest.sets[NIGHTLY_SET].seeds;

        for seed in ci {
            assert!(
                nightly.contains(seed),
                "the `{NIGHTLY_SET}` set must contain the `{DEFAULT_SET}` seed {seed}"
            );
        }
        assert!(
            nightly.len() > ci.len(),
            "the `{NIGHTLY_SET}` set ({} seeds) must be broader than the `{DEFAULT_SET}` \
             set ({} seeds)",
            nightly.len(),
            ci.len()
        );

        // Broader means more than "more of the same": every format has to come
        // up more than once across the nightly set.
        let capabilities = snapfab::capabilities::capabilities();
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for seed in nightly {
            *counts
                .entry(select(*seed, capabilities).expect("eligible").format)
                .or_insert(0) += 1;
        }
        for format in randomizable_formats(capabilities) {
            let count = counts.get(&format.format).copied().unwrap_or(0);
            assert!(
                count >= 2,
                "{format:?} is selected {count} time(s) across the `{NIGHTLY_SET}` set, so \
                 a nightly run adds no combination for it"
            );
        }
    }

    /// The default path is the fixed set, with no environment involved.
    #[test]
    fn without_an_override_a_scenario_runs_its_named_set() {
        let manifest = seed_manifest();
        let ci = manifest.sets[DEFAULT_SET].seeds.clone();

        assert_eq!(
            active_seeds(&manifest, DEFAULT_SET, None).expect("ci is declared"),
            ci
        );
        assert_eq!(
            active_seeds(&manifest, NIGHTLY_SET, None).expect("nightly is declared"),
            manifest.sets[NIGHTLY_SET].seeds
        );
        // A blank value is the same as no value: an exported-but-empty variable
        // must not change what CI runs.
        assert_eq!(
            active_seeds(&manifest, DEFAULT_SET, Some("   ")).expect("ci is declared"),
            ci
        );
    }

    /// The override switches sets, so a scheduled run needs no scenario edit.
    #[test]
    fn an_override_can_select_the_broader_set() {
        let manifest = seed_manifest();

        assert_eq!(
            active_seeds(&manifest, DEFAULT_SET, Some(NIGHTLY_SET)).expect("nightly is declared"),
            manifest.sets[NIGHTLY_SET].seeds
        );
    }

    /// The override can also be a list, which is what a red run's log line is
    /// for: replaying one reported seed needs no manifest entry.
    #[test]
    fn an_override_can_replay_explicit_seeds() {
        let manifest = seed_manifest();

        assert_eq!(
            active_seeds(&manifest, DEFAULT_SET, Some("0, 5")).expect("seeds parse"),
            vec![0, 5]
        );
        assert_eq!(
            active_seeds(&manifest, DEFAULT_SET, Some("999999")).expect("seeds parse"),
            vec![999_999],
            "an unrecorded seed must be replayable"
        );
        assert!(
            active_seeds(&manifest, DEFAULT_SET, Some("1,,2")).is_ok(),
            "blank entries are ignored so a trailing comma is not a typo that fails a run"
        );
    }

    /// An override that is neither a seed list nor a declared set names a set
    /// that does not exist, and the error has to say so instead of running
    /// nothing.
    #[test]
    fn an_unknown_set_is_rejected_by_name() {
        let manifest = seed_manifest();

        for value in ["weekly", "CI", "not a seed"] {
            let error = active_seeds(&manifest, DEFAULT_SET, Some(value))
                .expect_err("an undeclared set should be rejected");
            assert!(
                matches!(error, SeedError::UnknownSet(ref name) if name == value),
                "expected `{value}` to be reported as an unknown set, got: {error}"
            );
        }
        assert!(matches!(
            active_seeds(&manifest, "weekly", None),
            Err(SeedError::UnknownSet(_))
        ));
    }

    /// The environment variable the harness reads is the one documented in
    /// `seeds.json`; a renamed variable would silently stop overriding.
    #[test]
    fn the_override_comes_from_the_documented_variable() {
        assert_eq!(SEED_ENV_VAR, "PICASU_RANDOM_SEEDS");

        // Not set in the test process: reading the real environment must not be
        // able to fail the default path, and must stay deterministic here.
        let observed = env_override();
        if observed.is_some() {
            let manifest = seed_manifest();
            let expected = active_seeds(&manifest, DEFAULT_SET, observed.as_deref())
                .expect("the observed override should be usable");
            assert!(!expected.is_empty());
        }
    }

    /// One test per validation rule, generated from a table.
    ///
    /// A single test looping over the cases would stop at the first rejection,
    /// so every rule after it would go unpinned: the file's rules are one
    /// commit's worth of independent guards, not one assertion.
    macro_rules! validation_cases {
        ($( $name:ident : $patch:expr => $expected:literal ),* $(,)?) => {
            $(
                #[test]
                fn $name() {
                    let error = with_patch($patch)
                        .expect_err(concat!(stringify!($name), " should be rejected"));
                    assert!(
                        matches!(error, SeedError::Validation(_)),
                        concat!("expected a validation error for ", stringify!($name), ", got: {:?}"),
                        error
                    );
                    assert!(
                        error.to_string().contains($expected),
                        concat!(stringify!($name), ": expected an error mentioning ", $expected, ", got: {}"),
                        error
                    );
                }
            )*
        };
    }

    validation_cases! {
        an_unsupported_schema_version_is_rejected: |value: &mut serde_json::Value| {
            value["schemaVersion"] = serde_json::json!(2);
        } => "unsupported schema version",

        a_manifest_without_a_note_is_rejected: |value: &mut serde_json::Value| {
            value["note"] = serde_json::json!("  ");
        } => "must explain itself",

        an_empty_set_is_rejected: |value: &mut serde_json::Value| {
            value["sets"][DEFAULT_SET]["seeds"] = serde_json::json!([]);
            value["sets"][DEFAULT_SET]["resolvesTo"] = serde_json::json!([]);
        } => "must not be empty",

        a_repeated_seed_is_rejected: |value: &mut serde_json::Value| {
            value["sets"][DEFAULT_SET]["seeds"] = serde_json::json!([0, 0]);
        } => "repeats a seed",

        a_golden_table_that_misses_a_seed_is_rejected: |value: &mut serde_json::Value| {
            value["sets"][DEFAULT_SET]["resolvesTo"] =
                serde_json::json!([{ "seed": 0, "format": "mov" }]);
        } => "records a format for exactly the seeds it runs",

        a_golden_table_that_names_a_seed_twice_is_rejected: |value: &mut serde_json::Value| {
            value["sets"][DEFAULT_SET]["seeds"] = serde_json::json!([0]);
            value["sets"][DEFAULT_SET]["resolvesTo"] = serde_json::json!([
                { "seed": 0, "format": "mov" },
                { "seed": 0, "format": "mov" }
            ]);
        } => "records a format for one seed twice",

        a_manifest_without_the_ci_set_is_rejected: |value: &mut serde_json::Value| {
            value["sets"]
                .as_object_mut()
                .expect("sets object")
                .remove(DEFAULT_SET);
        } => "the `ci` set is required",

        a_manifest_without_the_nightly_set_is_rejected: |value: &mut serde_json::Value| {
            value["sets"]
                .as_object_mut()
                .expect("sets object")
                .remove(NIGHTLY_SET);
        } => "the `nightly` set is required",

        a_nightly_set_that_drops_a_ci_seed_is_rejected: |value: &mut serde_json::Value| {
            value["sets"][NIGHTLY_SET]["seeds"] = serde_json::json!([101, 102, 103]);
            value["sets"][NIGHTLY_SET]["resolvesTo"] = serde_json::json!([
                { "seed": 101, "format": "mov" },
                { "seed": 102, "format": "mov" },
                { "seed": 103, "format": "mov" }
            ]);
        } => "must contain every `ci` seed",

        a_nightly_set_that_is_not_broader_than_ci_is_rejected: |value: &mut serde_json::Value| {
            let ci = value["sets"][DEFAULT_SET].clone();
            value["sets"][NIGHTLY_SET] = ci;
        } => "must be broader than",
    }

    /// A malformed file is a parse error, not a validation one: the harness has
    /// to be able to tell "the file is broken" from "the file is wrong".
    #[test]
    fn a_malformed_seed_manifest_is_a_parse_error() {
        for input in [
            r#"{"schemaVersion": 1, "note": "x", "sets": {}}"#,
            r#"{"schemaVersion": 1, "note": "x", "sets": {"ci": {"seeds": [0]}}}"#,
            r#"[]"#,
        ] {
            let error = parse_seed_manifest(input).expect_err("a broken file should be rejected");
            assert!(
                matches!(error, SeedError::Parse(_) | SeedError::Validation(_)),
                "expected a rejection, got: {error:?}"
            );
        }
    }
}
