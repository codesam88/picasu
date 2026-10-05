//! Enforces `backend/tests/schema.json` against the scenario corpus.
//!
//! The schema documents the vocabulary `backend/src/tests/backend_api.rs`
//! accepts. Before this module existed nothing loaded it, so a scenario could
//! drift from the interpreter — a misspelled assertion, an unimplemented verb, a
//! value of the wrong type — and nothing but the run itself would notice, and
//! the run often cannot: a `then:` assertion the interpreter does not dispatch
//! is silently dropped, so a scenario full of them passes. Validating the files
//! is what turns the schema from prose into a check.
//!
//! The corpus is validated as data (`serde_yaml` → `serde_json::Value` → the
//! validator), not executed, so this is a static check and costs no backend
//! process. It is a *lower* bound on the interpreter's acceptance, not an
//! equality: a form the schema allows and the interpreter ignores still passes
//! here, and `selftest/json_absent_catches_*` is the model for how that class of
//! gap gets pinned instead.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The schema, parsed once. Panics on a parse failure: a schema this module
/// cannot read is not a scenario problem, and a test that skipped would report
/// the corpus as clean.
fn schema() -> &'static Value {
    use std::sync::OnceLock;
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        let path = schema_path();
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()))
    })
}

fn schema_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/schema.json")
}

fn scenarios_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios")
}

fn validator() -> &'static jsonschema::Validator {
    use std::sync::OnceLock;
    static VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        jsonschema::validator_for(schema()).unwrap_or_else(|e| {
            panic!(
                "{} does not compile as a JSON Schema: {e}",
                schema_path().display()
            )
        })
    })
}

/// Every scenario file, `selftest/` included, sorted so a red run names the same
/// first failure twice.
fn scenario_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display()));
        for entry in entries {
            let entry = entry.unwrap_or_else(|e| panic!("read entry in {}: {e}", dir.display()));
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "yaml")
            {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&scenarios_root(), &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "{} holds no scenarios, so this test would pass vacuously",
        scenarios_root().display()
    );
    files
}

/// The errors `document` produces, formatted with the instance path so a red run
/// points at the offending assertion rather than at the file.
fn errors_for(document: &Value) -> Vec<String> {
    validator()
        .iter_errors(document)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect()
}

/// The schema declares the dialect it is written in, and the validator compiles
/// it *as* that dialect rather than as whatever the crate defaults to. A schema
/// that quietly moved to a draft this crate does not implement would otherwise be
/// reported as a corpus failure — or, worse, validated under the wrong keyword
/// semantics and pass.
#[test]
fn the_schema_file_parses_and_compiles_as_the_dialect_it_declares() {
    let declared = schema()["$schema"]
        .as_str()
        .unwrap_or_else(|| panic!("{} declares no $schema", schema_path().display()));
    assert!(
        declared.starts_with("https://json-schema.org/draft/"),
        "the declared dialect should be a json-schema.org draft URI, got {declared}"
    );

    // `validator_for` reads `$schema` and picks the draft; compiling the same
    // value with the draft named explicitly proves the crate implements *that*
    // one, so a dialect bump cannot pass by falling back to a default.
    let declared_draft = jsonschema::Draft::from_schema_uri(declared);
    assert_ne!(
        declared_draft,
        jsonschema::Draft::Unknown,
        "the validator does not implement the declared dialect {declared}"
    );

    let compiled = jsonschema::options()
        .with_draft(declared_draft)
        .build(&schema().clone())
        .unwrap_or_else(|e| {
            panic!(
                "{} declares {declared}, which this validator build does not implement: {e}",
                schema_path().display()
            )
        });
    assert_eq!(
        compiled.draft(),
        declared_draft,
        "the validator compiled the schema under a different draft than it declares"
    );
}

/// Every scenario on disk validates. The failure message is the whole audit: one
/// line per offending assertion, so a corpus that has drifted says which keys
/// drifted rather than only how many files are wrong.
#[test]
fn every_scenario_validates_against_the_schema() {
    let mut failures = Vec::new();
    for file in scenario_files() {
        let text = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        let document: Value = match serde_yaml::from_str(&text) {
            Ok(document) => document,
            Err(e) => {
                failures.push(format!("{}: not valid YAML: {e}", file.display()));
                continue;
            }
        };
        for error in errors_for(&document) {
            failures.push(format!("{} {error}", file.display()));
        }
    }

    assert!(
        failures.is_empty(),
        "{} scenario assertion(s) do not match {}:\n  {}",
        failures.len(),
        schema_path().display(),
        failures.join("\n  ")
    );
}

/// The schema has to reject something, or "every scenario validates" is a
/// statement about a schema that accepts everything. Each case below is a way a
/// scenario really has gone wrong in this repository, and each must produce at
/// least one error.
#[test]
fn the_schema_rejects_documents_the_interpreter_would_reject_or_ignore() {
    // A control: the minimal document the interpreter runs. If this one were
    // rejected, the rejection cases below would prove nothing.
    let valid = serde_json::json!({
        "name": "a minimal scenario",
        "given": [{ "empty": true }],
        "when": [{ "call": "GET /get/get-albums" }],
        "then": [{ "response.status": 200 }]
    });
    assert_eq!(
        errors_for(&valid),
        Vec::<String>::new(),
        "the control document must validate, or the rejection cases prove nothing"
    );

    let cases: &[(&str, Value)] = &[
        (
            "a misspelled assertion verb",
            serde_json::json!({
                "name": "typo",
                "when": [{ "call": "GET /get/get-albums" }],
                "then": [{ "file_existz": "a/photo.jpg" }]
            }),
        ),
        (
            "an assertion key outside the response.json form the interpreter dispatches",
            serde_json::json!({
                "name": "bare response path",
                "when": [{ "call": "GET /get/get-albums" }],
                "then": [{ "response.ext": "jpg" }]
            }),
        ),
        (
            "a misspelled given verb",
            serde_json::json!({
                "name": "typo",
                "given": [{ "photoo": "/a/photo.jpg" }],
                "when": [{ "call": "GET /get/get-albums" }]
            }),
        ),
        (
            "a status code outside the HTTP range",
            serde_json::json!({
                "name": "bad code",
                "when": [{ "call": "GET /get/get-albums" }],
                "then": [{ "response.status": 99 }]
            }),
        ),
        (
            "an asset id that is not a variable reference",
            serde_json::json!({
                "name": "bad binding",
                "given": [{ "photo": "/a/photo.jpg", "id_as": "photo" }],
                "when": [{ "call": "GET /get/get-albums" }]
            }),
        ),
        (
            "two assertions in one `then` item, which the interpreter reads as one key",
            serde_json::json!({
                "name": "two keys",
                "when": [{ "call": "GET /get/get-albums" }],
                "then": [{ "response.status": 200, "file_absent": "a/photo.jpg" }]
            }),
        ),
        (
            "a `given` item that is not a mapping",
            serde_json::json!({
                "name": "scalar given",
                "given": ["photo.jpg"],
                "when": [{ "call": "GET /get/get-albums" }]
            }),
        ),
        (
            "an unknown top-level key",
            serde_json::json!({
                "name": "stray key",
                "thn": [{ "response.status": 200 }],
                "when": [{ "call": "GET /get/get-albums" }]
            }),
        ),
    ];

    let mut not_rejected = Vec::new();
    for (label, document) in cases {
        if errors_for(document).is_empty() {
            not_rejected.push(*label);
        }
    }
    assert!(
        not_rejected.is_empty(),
        "the schema accepted documents it should reject: {not_rejected:?}"
    );
}

/// The forms `docs/scenario-dsl.md` documents as implemented have to be
/// expressible. This is the other direction of drift from
/// `every_scenario_validates_against_the_schema`: a schema that tightened past a
/// documented form would make every existing scenario fail, but a *new* form the
/// docs promise would be missing here without any scenario noticing.
#[test]
fn the_documented_response_assertion_forms_are_expressible() {
    let document = serde_json::json!({
        "name": "documented forms",
        "when": [{ "call": "GET /get/get-albums" }],
        "then": [
            { "response.status": 200 },
            { "response.status_not": 404 },
            { "response.json.ext": "jpg" },
            { "response.json.exifVec.TAG:major_brand": "isom" },
            { "response.json.furtherMetadata": "absent" },
            { "response.json.rating": "not_null" },
            { "response.json.tags": { "contains": "a_tag" } },
            { "response.json.tags": { "not_contains": "other_tag" } },
            { "array_min_counts": { "a_tag": 1 } },
            { "array_where": { "where": { "tag": "a_tag" }, "expect": "absent" } },
            { "compare": { "response.json.width": { ">": 0 } } },
            { "file_exists": "a/photo.jpg" },
            { "file_absent": "a/photo.jpg" },
            { "serve_image_ok": "$photo" },
            { "thumb_exists": "$photo" },
            { "thumb_absent": "$photo" },
            { "file.contains": "a/photo.xmp", "text": "a_tag" }
        ]
    });

    let errors = errors_for(&document);
    assert!(
        errors.is_empty(),
        "docs/scenario-dsl.md documents forms the schema does not accept:\n  {}",
        errors.join("\n  ")
    );
}
