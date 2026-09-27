//! The subject taxonomy gate: one test per way an operation's tags can violate it.
//!
//! The vocabulary is a closed list on purpose — the generated reference groups by
//! it, so a tag nobody thought about is a group of one — and `pages` is reserved
//! for the routes that serve the SPA shell. Every rule is driven from a
//! *conforming* baseline: the `untagged/` tree carries one instance of each
//! failure mode and its report is asserted as a whole, which is what makes a rule
//! that stopped reporting, or started reporting twice, a test failure rather than
//! a quieter gate.

use openapi_sanity::{KNOWN_TAGS, spec_operations};

mod support;

use support::{Fixture, TEST_PREFIX};

/// A router tree and document carrying one instance of every tag failure mode.
const UNTAGGED: &str = "untagged";

/// A router tree and document whose tagging follows the taxonomy.
const CLEAN: &str = "clean";

/// The prefix the public artifact omits on purpose, as the gate is run in the
/// repository.
const REPOSITORY_PREFIX: &str = "/get/test/";

// ── A conforming tree ─────────────────────────────────────────────────────────

#[test]
fn a_taxonomy_following_document_reports_nothing() {
    assert_eq!(
        Fixture::load(CLEAN).tag_findings(&[TEST_PREFIX]),
        Vec::<String>::new(),
        "a document whose every operation carries a known tag, and whose pages carry \
         `pages`, must not be reported"
    );
}

// ── One test per failure mode ─────────────────────────────────────────────────
//
// Each asserts the exact diagnostic, so a change to the wording or the anchor of
// a rule shows up in a diff rather than silently loosening the gate.

#[test]
fn reports_an_operation_with_no_tags() {
    let fixture = Fixture::load(UNTAGGED);

    fixture.assert_tag_reports(&format!(
        "{}: GET /get/get-albums: declares no tags",
        fixture.label("openapi.json")
    ));
}

#[test]
fn reports_a_tag_outside_the_taxonomy() {
    let fixture = Fixture::load(UNTAGGED);

    fixture.assert_tag_reports(&format!(
        "{}: GET /get/edit-tag: unknown tag `metadata`",
        fixture.label("openapi.json")
    ));
}

#[test]
fn reports_a_data_api_operation_tagged_pages() {
    let fixture = Fixture::load(UNTAGGED);

    fixture.assert_tag_reports(&format!(
        "{}: GET /get/get-data: data-API path carries the `pages` tag",
        fixture.label("openapi.json")
    ));
}

#[test]
fn reports_a_page_operation_without_the_pages_tag() {
    // The other direction of the reserved tag: an SPA page route that lost, or
    // never got, the tag is the drift that randomizes the grouping in the first
    // place.
    let fixture = Fixture::load(UNTAGGED);

    fixture.assert_tag_reports(&format!(
        "{}: GET /login: SPA page path must carry `pages`",
        fixture.label("openapi.json")
    ));
}

#[test]
fn the_untagged_tree_reports_exactly_one_finding_per_rule() {
    let fixture = Fixture::load(UNTAGGED);
    let document = fixture.label("openapi.json");

    assert_eq!(
        fixture.tag_findings(&[TEST_PREFIX]),
        vec![
            format!("{document}: GET /get/edit-tag: unknown tag `metadata`"),
            format!("{document}: GET /get/get-albums: declares no tags"),
            format!("{document}: GET /get/get-data: data-API path carries the `pages` tag"),
            format!("{document}: GET /login: SPA page path must carry `pages`"),
        ],
        "each rule must report exactly once, and the report must stay sorted by \
         file, line and message"
    );
}

#[test]
fn a_conforming_operation_in_a_drifting_tree_is_not_reported() {
    // The `untagged/` tree carries a tagged page route and a data route tagged
    // outside the taxonomy; nothing about `/setting` is wrong, and a rule that
    // reported the whole path would say so here.
    let fixture = Fixture::load(UNTAGGED);
    let reported = fixture.tag_findings(&[TEST_PREFIX]);

    assert!(
        !reported.iter().any(|finding| finding.contains("/setting")),
        "an operation that follows the taxonomy must not be reported: {reported:#?}"
    );
}

#[test]
fn the_excluded_prefix_is_not_asked_to_follow_the_taxonomy() {
    // The fixture's probe is untagged as well as excluded, so the exclusion is
    // what keeps it out of the report. Widening the compared contract to the
    // test-only surface is the artifact owner's decision, and the artifact does
    // strip it.
    let fixture = Fixture::load(UNTAGGED);
    let probe = fixture.document()["paths"]["/get/test/record/{asset_id}"]["get"].clone();
    assert!(
        probe.get("tags").is_none(),
        "the fixture's probe is untagged on purpose: {probe}"
    );

    assert!(
        !fixture
            .tag_findings(&[TEST_PREFIX])
            .iter()
            .any(|finding| finding.contains("/get/test/")),
        "an excluded operation is outside the compared contract"
    );
}

// ── The vocabulary itself ─────────────────────────────────────────────────────

#[test]
fn the_taxonomy_names_no_tag_twice() {
    // A repeated entry would read as two names for one subject, and the reference
    // groups under the string, so the duplication is invisible in the output and
    // permanent in the policy.
    let mut names: Vec<&str> = KNOWN_TAGS.to_vec();
    names.sort_unstable();
    let duplicates: Vec<&str> = names
        .windows(2)
        .filter(|pair| pair[0] == pair[1])
        .map(|pair| pair[0])
        .collect();

    assert!(
        duplicates.is_empty(),
        "KNOWN_TAGS names the same subject more than once: {duplicates:?}"
    );
}

#[test]
fn the_reserved_tag_is_in_the_vocabulary() {
    // `pages` is the one subject the placement rules key on, so a vocabulary
    // without it would make both directions of that rule unreachable.
    assert!(
        KNOWN_TAGS.contains(&"pages"),
        "`pages` is reserved for the SPA page routes and belongs in the vocabulary"
    );
}

// ── The repository's own document ─────────────────────────────────────────────

#[test]
fn the_repository_operations_follow_the_shared_taxonomy() {
    // The gate runs in `just check` and in CI, so the vocabulary has to describe
    // the document as it is today. Without this, a rule that stopped comparing
    // anything would leave the fixture tests passing over a fixture while the real
    // artifact went unexamined.
    let repository = repository_root();
    let artifact = repository.join("backend").join("openapi.json");

    assert_eq!(
        support::tag_findings_against(&artifact, &[REPOSITORY_PREFIX]),
        Vec::<String>::new(),
        "every operation in the committed document has to follow the taxonomy"
    );
}

#[test]
fn the_repository_carries_the_tags_the_taxonomy_names() {
    // The count is what makes coverage checkable: a subject nobody groups under
    // would otherwise be indistinguishable from a subject with no operations, and
    // a vocabulary entry with no operation would keep passing.
    let repository = repository_root();
    let document: serde_json::Value = serde_json::from_str(&support::read(
        &repository.join("backend").join("openapi.json"),
    ))
    .expect("the committed document is valid JSON");

    let mut subjects: Vec<&str> = spec_operations(&document)
        .iter()
        .flat_map(|operation| operation.tags.iter().copied())
        .collect();
    subjects.sort_unstable();
    subjects.dedup();

    let mut vocabulary: Vec<&str> = KNOWN_TAGS.to_vec();
    vocabulary.sort_unstable();

    assert_eq!(
        subjects, vocabulary,
        "the taxonomy and the document's own tags have \
         drifted apart"
    );
}

#[test]
fn the_repository_gives_every_operation_exactly_one_subject() {
    // The taxonomy is a convention of one tag per operation, and no rule enforces
    // the upper bound — the four rules are about an absent, an unknown and a
    // misplaced tag. The convention is what makes the reference group an operation
    // once, so the artifact it is generated from is held to it here rather than
    // left as a claim in the documentation.
    let repository = repository_root();
    let document: serde_json::Value = serde_json::from_str(&support::read(
        &repository.join("backend").join("openapi.json"),
    ))
    .expect("the committed document is valid JSON");

    let operations: Vec<String> = spec_operations(&document)
        .iter()
        .filter(|operation| !operation.path.starts_with(REPOSITORY_PREFIX))
        .filter(|operation| operation.tags.len() != 1)
        .map(|operation| {
            format!(
                "{} {} carries {:?}",
                operation.method.as_str().to_ascii_uppercase(),
                operation.path,
                operation.tags
            )
        })
        .collect();

    assert!(
        operations.is_empty(),
        "every operation is grouped under exactly one subject, and these are not: \
         {operations:#?}"
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

impl Fixture {
    /// Assert that the tag gate reports exactly `expected`, a complete rendered
    /// diagnostic, so a rule that moves its anchor fails here rather than still
    /// matching on wording.
    fn assert_tag_reports(&self, expected: &str) {
        let reported = self.tag_findings(&[TEST_PREFIX]);

        assert!(
            reported.iter().any(|finding| finding == expected),
            "expected\n  {expected}\nfrom\n{reported:#?}"
        );
    }
}

fn repository_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the crate sits in <repo>/utils/openapi-sanity")
        .to_path_buf()
}
