//! The source/spec contract gate: one test per failure mode, driven by fixture
//! router sources and fixture documents.
//!
//! Every rule is checked from a *conforming* baseline. A rule that stopped
//! reporting would leave the clean fixture passing and the drift fixture
//! silently shorter, so the drift fixture is also asserted as a whole: exactly
//! one finding per rule, no more and no fewer. That is what makes a neutered
//! rule a test failure instead of a quieter gate.

use std::fs;
use std::path::{Path, PathBuf};

use openapi_sanity::{
    HttpMethod, SCANNED_MODULES, SourceUnit, check_contract, handler_module_path,
    referenced_handler_files, spec_operations,
};

/// The fixture router files, in a fixed order so the loaded unit set never
/// depends on directory iteration. A fixture file missing from this list is
/// simply not read.
const FIXTURE_FILES: &[&str] = &["get/mod.rs", "get/data.rs", "get/page.rs", "get/probe.rs"];

/// The prefix the public artifact omits on purpose, as the gate is run in the
/// repository. The fixture probe lives under it.
const TEST_PREFIX: &str = "/get/test/";

/// A router tree and document that agree: nothing to report.
const CLEAN: &str = "clean";

/// A router tree and document carrying one instance of every failure mode.
const DRIFT: &str = "drift";

// ── A conforming tree ─────────────────────────────────────────────────────────

#[test]
fn a_conforming_router_reports_nothing() {
    assert_eq!(
        findings(CLEAN, &[TEST_PREFIX]),
        Vec::<String>::new(),
        "a tree whose annotations, routes and document agree must not be reported"
    );
}

/// A `routes![]` block names handler modules its own file does not define, so a
/// caller that read only the scanned module list is missing most of the handlers
/// and every one of them would be reported as undeclared.
#[test]
fn the_handler_modules_of_a_route_table_are_listed_separately() {
    let fixture = Fixture::load(CLEAN);

    assert_eq!(
        referenced_handler_files(&fixture.units())
            .into_iter()
            .collect::<Vec<String>>(),
        vec![
            "get/data.rs".to_string(),
            "get/page.rs".to_string(),
            "get/probe.rs".to_string(),
        ]
    );
}

/// `page.rs` annotates and declares `/setting`, which no `routes![]` entry
/// registers and the fixture document does not list. A gate that compared every
/// annotated function would report it here and in the repository.
#[test]
fn a_handler_outside_the_route_table_is_not_compared_with_the_document() {
    let fixture = Fixture::load(CLEAN);
    assert!(fixture.read("get/page.rs").contains("fn setting()"));

    assert_eq!(fixture.findings(&[TEST_PREFIX]), Vec::<String>::new());
}

#[test]
fn the_excluded_prefix_is_load_bearing() {
    // Without the exclusion the probe is reported as missing from the document,
    // which is what it would be if the public artifact stopped stripping the
    // test-only surface. An exclusion that swallowed everything instead would
    // pass this test and fail the drift assertions below.
    let fixture = Fixture::load(CLEAN);

    assert_eq!(
        fixture.findings(&[]),
        vec![format!(
            "{}:7: probe::probe_record: GET /get/test/record/{{asset_id}} is declared in source \
             but absent from the spec",
            fixture.label("get/probe.rs")
        )]
    );
}

// ── One test per failure mode ─────────────────────────────────────────────────
//
// Each asserts the exact diagnostic, so a change to the wording or the anchor of
// a rule shows up in a diff rather than silently loosening the gate.

#[test]
fn reports_a_registered_handler_without_an_annotation() {
    let fixture = Fixture::load(DRIFT);

    fixture.assert_reports(&format!(
        "{}:24: data::get_metadata: registered in routes![] but the function carries no \
         #[utoipa::path] annotation",
        fixture.label("get/data.rs")
    ));
}

#[test]
fn reports_a_path_that_disagrees_with_its_route() {
    let fixture = Fixture::load(DRIFT);

    fixture.assert_reports(&format!(
        "{}:8: data::get_data: the route serves /get/get-data but its #[utoipa::path] \
         declares /get/get-data-RENAMED",
        fixture.label("get/data.rs")
    ));
}

#[test]
fn reports_a_verb_that_disagrees_with_its_route() {
    let fixture = Fixture::load(DRIFT);

    fixture.assert_reports(&format!(
        "{}:14: data::get_rows: the route declares GET but its #[utoipa::path] declares POST",
        fixture.label("get/data.rs")
    ));
}

#[test]
fn reports_a_source_operation_absent_from_the_document() {
    let fixture = Fixture::load(DRIFT);

    fixture.assert_reports(&format!(
        "{}:20: data::path_completion: GET /get/path-completion is declared in source but \
         absent from the spec",
        fixture.label("get/data.rs")
    ));
}

#[test]
fn reports_a_document_operation_no_source_declares() {
    let fixture = Fixture::load(DRIFT);

    fixture.assert_reports(&format!(
        "{}: GET /get/get-albums is in the spec but no scanned route declares it",
        fixture.label("openapi.json")
    ));
}

#[test]
fn reports_a_handler_registered_twice() {
    let fixture = Fixture::load(DRIFT);
    let route_table = fixture.label("get/mod.rs");

    fixture.assert_reports(&format!(
        "{route_table}:10: page::login: registered in routes![] more than once \
         (first at {route_table}:9)"
    ));
}

#[test]
fn reports_duplicate_operation_ids() {
    let fixture = Fixture::load(DRIFT);

    fixture.assert_reports(&format!(
        "{}: duplicate operationId `get_data` claimed by GET /get/get-albums, \
         GET /get/get-data-RENAMED",
        fixture.label("openapi.json")
    ));
}

#[test]
fn a_handler_registered_twice_is_still_checked_once() {
    // The duplicate registration is a finding of its own; it must not also
    // produce the absent-from-the-spec finding that checking the handler once per
    // registration would produce for a document that dropped it.
    let fixture = Fixture::load(DRIFT);
    let reported = fixture.findings(&[TEST_PREFIX]);

    assert_eq!(
        reported
            .iter()
            .filter(|finding| finding.contains("page::login"))
            .count(),
        1,
        "got: {reported:#?}"
    );
}

#[test]
fn a_declaration_that_disagrees_with_its_route_is_not_also_reported_as_spec_drift() {
    // The document is generated from the annotation, so a handler whose
    // annotation and route attribute disagree inherits the disagreement. One
    // cause, one finding: the local one names the fix, and the two operations
    // such a handler owns stay accounted for on the document side.
    let fixture = Fixture::load(DRIFT);
    let reported = fixture.findings(&[TEST_PREFIX]);
    let renamed = "/get/get-data-RENAMED";
    let rows = "/get/get-rows";

    for (operation, described) in [(renamed, "get_data"), (rows, "get_rows")] {
        let on_the_source_side = reported.iter().filter(|finding| {
            finding.contains(described) && finding.contains("#[utoipa::path] declares")
        });
        assert_eq!(
            on_the_source_side.count(),
            1,
            "the disagreement about {operation} is reported once, locally: {reported:#?}"
        );
        assert!(
            !reported
                .iter()
                .any(|finding| finding.contains(operation) && finding.contains("the spec")),
            "{operation} must not also be reported as document drift: {reported:#?}"
        );
    }
}

#[test]
fn the_drift_tree_reports_exactly_one_finding_per_rule() {
    let fixture = Fixture::load(DRIFT);
    let data = fixture.label("get/data.rs");
    let route_table = fixture.label("get/mod.rs");
    let document = fixture.label("openapi.json");

    assert_eq!(
        fixture.findings(&[TEST_PREFIX]),
        vec![
            format!(
                "{data}:8: data::get_data: the route serves /get/get-data but its \
                 #[utoipa::path] declares /get/get-data-RENAMED"
            ),
            format!(
                "{data}:14: data::get_rows: the route declares GET but its #[utoipa::path] \
                 declares POST"
            ),
            format!(
                "{data}:20: data::path_completion: GET /get/path-completion is declared in \
                 source but absent from the spec"
            ),
            format!(
                "{data}:24: data::get_metadata: registered in routes![] but the function \
                 carries no #[utoipa::path] annotation"
            ),
            format!(
                "{route_table}:10: page::login: registered in routes![] more than once \
                 (first at {route_table}:9)"
            ),
            format!(
                "{document}: GET /get/get-albums is in the spec but no scanned route \
                 declares it"
            ),
            format!(
                "{document}: duplicate operationId `get_data` claimed by \
                 GET /get/get-albums, GET /get/get-data-RENAMED"
            ),
        ],
        "each rule must report exactly once, and the report must stay sorted by \
         file, line and message"
    );
}

#[test]
fn the_report_does_not_depend_on_the_order_the_files_were_read_in() {
    // Ordering is what makes a gate's output reviewable in a diff, so the unit
    // set is deliberately reversed here.
    let ordered = Fixture::load(DRIFT);
    let reversed = Fixture::load(DRIFT).reversed();

    assert_eq!(
        reversed.findings(&[TEST_PREFIX]),
        ordered.findings(&[TEST_PREFIX])
    );
}

// ── Reading a document ────────────────────────────────────────────────────────

#[test]
fn a_path_item_key_that_is_not_a_method_is_not_an_operation() {
    // The fixture document carries a `parameters` key inside a path item, and a
    // path item is the only place a non-operation key can appear. Reading it as
    // an operation would report a method-less operation nothing declares.
    let fixture = Fixture::load(CLEAN);
    let document: serde_json::Value = fixture.document();
    let operations = spec_operations(&document);

    assert_eq!(
        operations
            .iter()
            .map(|operation| (operation.method.as_str(), operation.path))
            .collect::<Vec<_>>(),
        vec![
            ("get", "/get/get-data"),
            ("get", "/get/get-rows"),
            ("get", "/get/metadata/{asset_id}"),
            ("get", "/get/path-completion"),
            ("get", "/login"),
        ]
    );
}

#[test]
fn operations_are_reported_in_a_stable_order() {
    let fixture = Fixture::load(DRIFT);
    let document: serde_json::Value = fixture.document();
    let operations: Vec<(HttpMethod, &str)> = spec_operations(&document)
        .iter()
        .map(|operation| (operation.method, operation.path))
        .collect();
    let mut sorted = operations.clone();
    sorted.sort_unstable();

    assert_eq!(
        operations, sorted,
        "spec_operations must sort its result, or two reads of one document would \
         report in whatever order the JSON object happened to be built in"
    );
}

// ── Which files the contract is read from ─────────────────────────────────────

#[test]
fn a_module_list_entry_names_a_group_that_matches_its_layout() {
    // The group prefix a `routes![]` entry resolves against is the group a
    // module belongs to, and the group is the first segment of its path. A list
    // entry that disagrees would resolve every handler of the module elsewhere.
    for (group, relative) in SCANNED_MODULES {
        let unit = SourceUnit::for_relative_path("module", relative, "");

        assert_eq!(
            unit.group_prefix(),
            *group,
            "SCANNED_MODULES says group `{group}` for {relative}"
        );
    }
}

#[test]
fn a_group_root_module_is_its_own_module() {
    // `get/mod.rs` declares the route table of group `get` and the module `get`
    // itself, which is what makes an unqualified entry in it resolve to that
    // file. `delete.rs` is the same thing as a single-segment path.
    let mod_rs = SourceUnit::for_relative_path("mod", "get/mod.rs", "");
    assert_eq!(
        (mod_rs.group_prefix(), mod_rs.module_path()),
        ("get", "get")
    );

    let delete = SourceUnit::for_relative_path("delete", "delete.rs", "");
    assert_eq!(
        (delete.group_prefix(), delete.module_path()),
        ("delete", "delete")
    );

    let page = SourceUnit::for_relative_path("page", "get/get_page.rs", "");
    assert_eq!(
        (page.group_prefix(), page.module_path()),
        ("get", "get_page")
    );
}

#[test]
fn a_handler_resolves_to_the_file_the_build_script_imports_from() {
    // The same mapping the build script uses to emit `__path_*` imports: a
    // qualified entry names a module file, an entry naming the group itself is
    // that group's root file.
    assert_eq!(handler_module_path("get", "get_page"), "get/get_page.rs");
    assert_eq!(handler_module_path("get", "get"), "get.rs");
    assert_eq!(handler_module_path("auth", "auth"), "auth.rs");
}

// ── Fixtures ──────────────────────────────────────────────────────────────────

/// One fixture tree: its router files, its document, and the findings the gate
/// reports for them.
///
/// The files are owned here because a [`SourceUnit`] borrows its label and
/// contents; the tree has to outlive the units built from it.
struct Fixture {
    root: PathBuf,
    files: Vec<(String, String, String)>,
}

impl Fixture {
    /// Read the router files and the document of a named fixture tree.
    fn load(tree: &str) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(tree);
        let files = FIXTURE_FILES
            .iter()
            .map(|relative| {
                (
                    root.join(relative).display().to_string(),
                    (*relative).to_string(),
                    read(&root.join(relative)),
                )
            })
            .collect();

        Self { root, files }
    }

    /// The same tree with the router files in reverse order.
    fn reversed(mut self) -> Self {
        self.files.reverse();
        self
    }

    fn units(&self) -> Vec<SourceUnit<'_>> {
        self.files
            .iter()
            .map(|(label, relative, source)| SourceUnit::for_relative_path(label, relative, source))
            .collect()
    }

    /// The label the checks see for a fixture file.
    fn label(&self, relative: &str) -> String {
        self.root.join(relative).display().to_string()
    }

    fn read(&self, relative: &str) -> String {
        read(&self.root.join(relative))
    }

    fn document(&self) -> serde_json::Value {
        serde_json::from_str(&self.read("openapi.json"))
            .unwrap_or_else(|error| panic!("fixture document is not valid JSON: {error}"))
    }

    /// Every finding of the gate for this tree, rendered as it is printed.
    fn findings(&self, excluded: &[&str]) -> Vec<String> {
        let units = self.units();
        let document = self.document();
        let spec = spec_operations(&document);
        let label = self.label("openapi.json");

        check_contract(&units, &label, &spec, excluded)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    /// Assert that the gate reports exactly `expected`, a complete rendered
    /// diagnostic with its file and line, so a rule that moves its anchor fails
    /// here rather than still matching on wording.
    fn assert_reports(&self, expected: &str) {
        let reported = self.findings(&[TEST_PREFIX]);

        assert!(
            reported.iter().any(|finding| finding == expected),
            "expected\n  {expected}\nfrom\n{reported:#?}"
        );
    }
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// Every finding of the gate for a named fixture tree.
fn findings(tree: &str, excluded: &[&str]) -> Vec<String> {
    Fixture::load(tree).findings(excluded)
}
