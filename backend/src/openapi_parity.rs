//! The route-set parity gate behind the binary's `--check-openapi` flag.
//!
//! The spec is generated from two things that are not the runtime route table:
//! `build.rs` scans `routes![]` invocations and utoipa reads the
//! `#[utoipa::path]` annotations. A handler can therefore be mounted and
//! documented nowhere, or documented and no longer mounted, without a build or
//! a test failure. This module is the check that can see it, because it reads
//! Rocket's own mount table from a real `build_rocket()` and the **committed**
//! `openapi.json` from disk — the artifact the review made its claim about,
//! rather than the compiled-in copy the code would agree with by construction.
//!
//! The rule is asymmetric, which is what feature-gating requires:
//!
//! - every route this build mounts, inside the contract, must be documented —
//!   a hard failure;
//! - a documented operation this build does not mount is accepted only when it
//!   carries a [`FEATURE_MARKER`] naming a feature that is *disabled* here, and
//!   is drift otherwise.
//!
//! The comparison is a pure function of the mounted routes, the document's
//! operations and this build's enabled features, so the rule is testable
//! without a server and without a product build — and so is it one
//! implementation, read by the flag and by the backend's parity self-checks
//! alike.
//!
//! The spec is a review artifact, not a deployment dependency: nothing here runs
//! unless `--check-openapi` is asked for, so the server's boot does not depend
//! on `openapi.json` and cannot fail because it is missing.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use rocket::http::Method;

use crate::model::config::{APP_CONFIG, AppConfig};
use crate::openapi_public::CONTRACT_EXCLUSION_PREFIXES;
use crate::spec_path::to_spec_path;

/// The vendor extension a feature-gated operation carries, set by the annotation
/// author with utoipa's `extensions(("x-picasu-feature" = json!("<feature>")))`.
///
/// The key is spelled in full even though utoipa's `ExtensionsBuilder` adds the
/// specification's `x-` prefix to a bare key, so that the serialized name and
/// the name this module reads are the same string. `every_feature_marker_in_
/// the_committed_spec_is_a_declared_feature` holds the markers to features the
/// manifest declares, because a marker naming a feature nobody declares is
/// permanently false in the `cfg!` read below and would excuse its operation
/// forever.
pub const FEATURE_MARKER: &str = "x-picasu-feature";

/// A mounted route or a documented operation, as a comparable identity.
pub type Operation = (Method, String);

/// A documented operation that this build does not mount, and the feature that
/// excuses it: the feature is declared but disabled here.
pub type Excused = (Operation, String);

/// The drift `--check-openapi` reports.
///
/// `excused` is not drift — it is the reason the rule is asymmetric, kept in the
/// report so a reviewer can see which operations a build is deliberately not
/// serving.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RouteSetReport {
    /// Mounted, inside the contract, and absent from the document.
    pub undocumented: Vec<Operation>,
    /// In the document, not mounted here, and not excused by a disabled feature.
    pub unmounted: Vec<Operation>,
    /// In the document, not mounted here, feature-gated with the feature off.
    pub excused: Vec<Excused>,
    /// How many routes this build mounts inside the contract, exclusions dropped.
    pub routes_in_contract: usize,
    /// How many operations the document carries inside the contract.
    pub operations_in_contract: usize,
    /// How many routes and operations the policy dropped from the comparison.
    pub outside_the_contract: usize,
    /// Mounted routes dropped by the contract policy.
    pub outside_mounted: Vec<Operation>,
    /// Documented operations dropped by the contract policy.
    pub outside_documented: Vec<Operation>,
}

impl RouteSetReport {
    /// Whether the two views agree, which is the only condition under which the
    /// gate passes.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.undocumented.is_empty() && self.unmounted.is_empty()
    }

    /// The clean summary or drift details, including waived and excluded routes.
    #[must_use]
    pub fn render(&self) -> String {
        let mut sections = Vec::new();
        if self.is_clean() {
            sections.push(format!(
                "{} routes matched to spec.",
                self.routes_in_contract
            ));
        }
        if !self.undocumented.is_empty() {
            sections.push(section(
                "mounted route",
                "mounted routes",
                "missing from the spec",
                &self.undocumented,
            ));
        }
        if !self.unmounted.is_empty() {
            sections.push(section(
                "documented operation",
                "documented operations",
                "no route mounts",
                &self.unmounted,
            ));
        }
        if !self.excused.is_empty() {
            let mut lines = vec!["Waived feature-gated operations:".to_owned()];
            lines.extend(self.excused.iter().map(|((method, path), feature)| {
                format!("  - {method} {path} (feature: {feature})")
            }));
            sections.push(lines.join("\n"));
        }
        if !self.outside_mounted.is_empty() || !self.outside_documented.is_empty() {
            let mut lines = vec!["Excluded from OpenAPI contract:".to_owned()];
            lines.extend(
                self.outside_mounted
                    .iter()
                    .map(|(method, path)| format!("  - mounted {method} {path}")),
            );
            lines.extend(
                self.outside_documented
                    .iter()
                    .map(|(method, path)| format!("  - documented {method} {path}")),
            );
            sections.push(lines.join("\n"));
        }
        sections.join("\n\n")
    }
}

/// The report's ordering key. `Method` is not `Ord`, so the verb is compared as
/// the string it is printed as, which is the order a reader sees.
fn sort_key(operation: &Operation) -> (String, String) {
    (operation.0.as_str().to_string(), operation.1.clone())
}

fn section(singular: &str, plural: &str, predicate: &str, operations: &[Operation]) -> String {
    let noun = if operations.len() == 1 {
        singular
    } else {
        plural
    };
    let mut lines = vec![format!("{} {noun} {predicate}:", operations.len())];
    lines.extend(render_operations(operations));
    lines.join("\n")
}

fn render_operations(operations: &[Operation]) -> Vec<String> {
    operations
        .iter()
        .map(|(method, path)| format!("  {method} {path}"))
        .collect()
}

/// Compare a build's mounted routes with a document's operations.
///
/// `mounted` and the keys of `documented` are both already normalized to
/// `OpenAPI` path templates by [`to_spec_path`]; `documented` carries each
/// operation's [`FEATURE_MARKER`] value, if it has one. Anything under
/// `excluded_prefixes` — the backend's contract policy, [`CONTRACT_EXCLUSION_PREFIXES`]
/// — is dropped from both sides before the comparison, since those surfaces are
/// outside the contract in every build.
///
/// `enabled_features` is what this build compiled with ([`enabled_features`]),
/// which is the only input the excuse branch needs.
#[must_use]
pub fn compare_route_set(
    mounted: &HashSet<Operation, impl std::hash::BuildHasher>,
    documented: &HashMap<Operation, Option<String>, impl std::hash::BuildHasher>,
    enabled_features: &[&str],
    excluded_prefixes: &[&str],
) -> RouteSetReport {
    let mut outside_mounted: Vec<Operation> = mounted
        .iter()
        .filter(|(_, path)| is_outside_contract(path, excluded_prefixes))
        .cloned()
        .collect();
    outside_mounted.sort_by_key(sort_key);
    let mut outside_documented: Vec<Operation> = documented
        .keys()
        .filter(|operation| is_outside_contract(&operation.1, excluded_prefixes))
        .cloned()
        .collect();
    outside_documented.sort_by_key(sort_key);
    let outside_the_contract = outside_mounted.len() + outside_documented.len();

    let mounted: HashSet<&Operation> = mounted
        .iter()
        .filter(|(_, path)| !is_outside_contract(path, excluded_prefixes))
        .collect();
    let documented: HashMap<&Operation, &Option<String>> = documented
        .iter()
        .filter(|(operation, _)| !is_outside_contract(&operation.1, excluded_prefixes))
        .collect();

    let mut undocumented: Vec<Operation> = mounted
        .iter()
        .filter(|operation| !documented.contains_key(*operation))
        .map(|operation| (*operation).clone())
        .collect();
    undocumented.sort_by_key(sort_key);

    let mut unmounted: Vec<Operation> = Vec::new();
    let mut excused: Vec<Excused> = Vec::new();
    for (operation, feature) in &documented {
        if mounted.contains(*operation) {
            continue;
        }
        match excused_by(feature.as_deref(), enabled_features) {
            Some(feature) => excused.push(((*operation).clone(), feature)),
            None => unmounted.push((*operation).clone()),
        }
    }
    // Sorting keeps the report deterministic: the inputs are sets, and a gate
    // whose output changes between runs is a gate people stop reading.
    unmounted.sort_by_key(sort_key);
    excused.sort_by_key(|excused| sort_key(&excused.0));

    RouteSetReport {
        undocumented,
        unmounted,
        excused,
        routes_in_contract: mounted.len(),
        operations_in_contract: documented.len(),
        outside_the_contract,
        outside_mounted,
        outside_documented,
    }
}

/// The feature that excuses an unmounted document operation, if any.
///
/// The excuse needs a marker *and* a feature this build has off: with the
/// feature on, the operation was expected to be mounted and its absence is
/// drift.
fn excused_by(marker: Option<&str>, enabled_features: &[&str]) -> Option<String> {
    let feature = marker?;
    (!enabled_features.contains(&feature)).then(|| feature.to_string())
}

/// Whether a path is one the contract policy declares outside the contract.
#[must_use]
pub fn is_outside_contract(path: &str, excluded_prefixes: &[&str]) -> bool {
    excluded_prefixes
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

/// Parse a document's method key.
///
/// A path item may carry fields that are not operations (`summary`,
/// `description`, `servers`, `parameters`, `$ref`); those are skipped by
/// [`documented_operations`]. A key that is neither a verb nor a path-item field
/// is reported, because a document whose path items carry something this reader
/// does not understand is a document whose operations it cannot enumerate.
///
/// # Errors
/// Returns the offending key when it is neither an HTTP verb nor a path-item
/// field.
pub fn spec_method(key: &str) -> Result<Method, String> {
    match key.to_ascii_lowercase().as_str() {
        "get" => Ok(Method::Get),
        "post" => Ok(Method::Post),
        "put" => Ok(Method::Put),
        "delete" => Ok(Method::Delete),
        "options" => Ok(Method::Options),
        "head" => Ok(Method::Head),
        "patch" => Ok(Method::Patch),
        "trace" => Ok(Method::Trace),
        other => Err(format!(
            "the spec declares unsupported HTTP method `{other}`"
        )),
    }
}

/// The path-item fields that are not operations.
const PATH_ITEM_FIELDS: [&str; 5] = ["summary", "description", "servers", "parameters", "$ref"];

/// Every operation in an `OpenAPI` document, with the feature it is gated
/// behind.
///
/// This is the document side of the comparison, and the only reader of
/// [`FEATURE_MARKER`]: an operation without the extension is ungated, and one
/// whose extension is not a non-empty feature name is an error rather than a
/// silently ignored marker, which would excuse nothing while looking gated.
///
/// # Errors
/// Returns why a document cannot be read: no `paths` object, a path item that is
/// not an object, an operation under a key that is not a verb, or a
/// [`FEATURE_MARKER`] that is not a non-empty feature name.
pub fn documented_operations(
    spec: &serde_json::Value,
) -> Result<HashMap<Operation, Option<String>>, String> {
    let paths = spec
        .get("paths")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "the document has no `paths` object".to_string())?;

    let mut operations = HashMap::new();
    for (path, item) in paths {
        let item = item
            .as_object()
            .ok_or_else(|| format!("`{path}` is not a path item object"))?;
        for (key, operation) in item {
            if PATH_ITEM_FIELDS.contains(&key.as_str()) {
                continue;
            }
            let method = spec_method(key).map_err(|error| format!("{path}: {error}"))?;
            let feature = match operation.get(FEATURE_MARKER) {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(feature)) if !feature.is_empty() => {
                    Some(feature.clone())
                }
                Some(_) => {
                    return Err(format!(
                        "{path} {key}: `{FEATURE_MARKER}` must be a non-empty feature name"
                    ));
                }
            };
            operations.insert((method, path.clone()), feature);
        }
    }
    Ok(operations)
}

/// Read and parse a spec document from disk.
///
/// A missing or unparsable file is an error naming the path and the fix, not a
/// panic: the artifact is generated, so the answer is always "run
/// `just openapi-gen`".
///
/// # Errors
/// Returns the path and the cause when the file is missing or is not JSON.
pub fn read_spec(path: &Path) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "cannot read the OpenAPI document at {}: {error}\n\
             Run `just openapi-gen` to (re)generate it, or pass the path to the spec to check.",
            path.display()
        )
    })?;
    serde_json::from_str(&text).map_err(|error| {
        format!(
            "cannot parse the OpenAPI document at {}: {error}\n\
             Run `just openapi-gen` to (re)generate it, or pass the path to the spec to check.",
            path.display()
        )
    })
}

/// The committed artifact, which is what the check reads when it is given no
/// argument: `backend/openapi.json`, beside the crate manifest.
#[must_use]
pub fn default_spec_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("openapi.json")
}

/// The features this build was compiled with.
///
/// Read with `cfg!` because the excuse rule is about *this* build: a canonical
/// spec describes the union across features, so a build without a feature
/// legitimately mounts less than the document carries. The list is exhaustive
/// over `backend/Cargo.toml`'s `[features]`, which
/// `every_declared_feature_is_readable_by_the_enabled_feature_list` holds it
/// to — a declared feature missing here would be permanently "disabled" and
/// would excuse whatever is marked with it.
#[must_use]
pub fn enabled_features() -> Vec<&'static str> {
    KNOWN_FEATURES
        .into_iter()
        .filter(|feature| is_enabled(feature))
        .collect()
}

/// Every feature `backend/Cargo.toml` declares, and so every feature the excuse
/// rule can be asked about. A feature declared but missing here is one no build
/// can ever report as enabled, which would make anything marked with it excused
/// unconditionally;
/// `every_declared_feature_is_readable_by_the_enabled_feature_list` holds the two
/// sets equal.
pub const KNOWN_FEATURES: [&str; 2] = ["embed-frontend", "auto-open-browser"];

/// Whether this build was compiled with `feature`, read with `cfg!`.
///
/// The match is exhaustive over [`KNOWN_FEATURES`] by construction; an unknown
/// name is not enabled, which is the answer the excuse branch wants.
fn is_enabled(feature: &str) -> bool {
    match feature {
        "embed-frontend" => cfg!(feature = "embed-frontend"),
        "auto-open-browser" => cfg!(feature = "auto-open-browser"),
        _ => false,
    }
}

/// Every route the product build mounts, in spec path form.
///
/// This is the real `build_rocket()`, not the test instance: a test build runs
/// without `embed-frontend` and carries the `#[cfg(test)]` probe registrations,
/// so it serves a different table than the one that ships — which is the reason
/// the check cannot live in a test.
///
/// Building the instance is cheap and inert: nothing ignites, launches or opens
/// a database, it reads the route table and drops it. `APP_CONFIG` is set to its
/// default if the caller has not initialized it, so the flag needs no data
/// directory; route registration does not read the config beyond the address,
/// port and upload limit the builder parses.
///
/// Building a `Rocket` also installs Rocket's logger, which reports the asset
/// path the instance would serve from — the placeholder one, since the config
/// here is the default. That belongs to a boot, not to a route-set verdict, so
/// the logger slot is claimed with a no-op before the instance is built; Rocket
/// then leaves the level alone, and the process exits immediately afterwards.
///
/// # Panics
/// Panics if `APP_CONFIG` was already set by the caller, which would mean two
/// owners of the global configuration.
pub fn mounted_operations() -> HashSet<Operation> {
    let _ = log::set_boxed_logger(Box::new(SilentLogger));
    log::set_max_level(log::LevelFilter::Off);
    if APP_CONFIG.get().is_none() {
        APP_CONFIG
            .set(RwLock::new(AppConfig::default()))
            .expect("APP_CONFIG must not already be set");
    }
    crate::router::build_rocket()
        .routes()
        .map(|route| (route.method, to_spec_path(&route.uri.to_string())))
        .collect()
}

/// A logger that discards everything, used to hold the `log` slot before
/// `build_rocket()` installs Rocket's own.
struct SilentLogger;

impl log::Log for SilentLogger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        false
    }

    fn log(&self, _: &log::Record<'_>) {}

    fn flush(&self) {}
}

/// Read the document at `spec_path`, build the route table, and compare them.
///
/// # Errors
/// Returns why the document at `spec_path` could not be read or enumerated.
pub fn check(spec_path: &Path) -> Result<RouteSetReport, String> {
    let document = read_spec(spec_path)?;
    let documented = documented_operations(&document)?;
    let mounted = mounted_operations();
    Ok(compare_route_set(
        &mounted,
        &documented,
        &enabled_features(),
        &CONTRACT_EXCLUSION_PREFIXES,
    ))
}

/// Run the check and report it on the console. The return value is the process
/// exit code, which is all the caller does with it.
///
/// Exit codes follow the contract gate's other tools: `0` clean, `1` drift, `2`
/// an input that cannot be read — where the spec is unreadable is a different
/// failure from the spec being wrong, and neither is a passing run.
#[must_use]
pub fn run(spec_path: &Path) -> i32 {
    match check(spec_path) {
        Err(error) => {
            eprintln!("check-openapi: {error}");
            2
        }
        Ok(report) if report.is_clean() => {
            println!("check-openapi: {}", report.render());
            0
        }
        Ok(report) => {
            eprintln!(
                "check-openapi: the mounted routes and {} disagree:",
                spec_path.display()
            );
            eprintln!("{}", report.render());
            eprintln!(
                "Fix: annotate the mounted routes and run `just openapi-gen`, or remove the \
                 operation from the document. `x-picasu-feature` marks an operation that only \
                 exists under a feature."
            );
            1
        }
    }
}
