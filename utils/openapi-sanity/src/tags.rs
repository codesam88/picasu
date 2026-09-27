//! The subject taxonomy the generated reference groups operations by, and the
//! checks that hold the document to it.
//!
//! Without it the reference groups by whichever handler was annotated last, which
//! is not a grouping: 39 of 65 operations carried no tag at all before the
//! taxonomy was introduced. A tag is a documentation decision — *which subject does
//! this operation act on* — and it is not a security statement. Nothing here
//! infers who may call an operation from how it is tagged; see [`crate::auth`].

use crate::contract::{SpecOperation, is_excluded, verb};
use crate::finding::Finding;

/// Every subject the taxonomy allows.
///
/// A closed list, because a tag nobody thought about is a reference group of one:
/// the vocabulary is what makes the generated documentation's grouping reviewable
/// rather than incidental. Adding a subject is therefore a deliberate change to
/// this line plus the matching row in `docs/openapi-generator.md` — a new group of
/// operations deserves a decision about where it belongs, not a default.
///
/// It is also a hand-written claim about the document, held to its claim by
/// `the_repository_carries_the_tags_the_taxonomy_names`: a vocabulary entry no
/// operation uses would keep passing, and an operation grouped under a subject
/// nobody listed would be reported by [`check_tags`].
pub const KNOWN_TAGS: &[&str] = &[
    "albums", "assets", "auth", "config", "index", "pages", "serving", "timeline", "upload",
];

/// The subject reserved for the routes that serve the SPA shell.
///
/// The one subject whose *placement* is a rule rather than a preference: a page is
/// not an act on a subject, and an operation under a data-API path is never the
/// shell. It exists because these routes are the ones a reader of the reference
/// is not looking for, and because the SPA behind them being public says nothing
/// about the data the tagged operations serve.
const PAGE_TAG: &str = "pages";

/// Path shapes that identify data-API operations, as opposed to SPA HTML pages.
const DATA_API_PREFIXES: &[&str] = &["/delete/", "/get/", "/object/", "/post/", "/put/"];

/// Whether `path` is a data-API operation rather than an SPA page route.
///
/// Derived from path shape because the document alone does not say which file
/// annotated an operation. Stated assumption: every public-spec operation that
/// is *not* under one of these prefixes (or `POST /upload`) is one of the
/// `router/get/get_page.rs` HTML routes. The two things that would break the
/// assumption — the test-only probes and the `/assets` file server — are excluded
/// from the public spec, and a caller that includes them passes the exclusions.
/// If a data route were ever added outside these shapes, this function would
/// classify it as a page and the placement rule would fail loudly instead of
/// accepting the wrong grouping.
fn is_data_api_path(path: &str) -> bool {
    DATA_API_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
        || path == "/upload"
}

/// Every way an operation's tags can violate the taxonomy.
///
/// Four rules, one per direction they can fail in:
///
/// | Finding                                       | Drift it catches                                          |
/// | --------------------------------------------- | --------------------------------------------------------- |
/// | the operation declares no tags                | an annotation that lost its `tag = "..."`                  |
/// | unknown tag \`x\`                              | a subject added to an operation without a reviewed entry  |
/// | a data-API path carries the \`pages\` tag      | a data operation grouped with the SPA shell                |
/// | an SPA page path must carry \`pages\`          | a page route that is grouped under a subject              |
///
/// The rules are independent, so an operation with no tags on a page path is
/// reported twice: it is missing a tag *and* it is missing the reserved one, and
/// each says something the other does not.
///
/// `spec_label` is the caller's label for the document, and `excluded_prefixes`
/// means what it means in [`crate::check_contract`]: an excluded operation is
/// outside the compared contract, so the test-only surface the public artifact
/// strips is not asked to follow a taxonomy it is not published under.
///
/// Findings are sorted by file, line and message, so two runs over the same
/// document report the same things in the same order.
#[must_use]
pub fn check_tags(
    spec_label: &str,
    spec: &[SpecOperation<'_>],
    excluded_prefixes: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for operation in spec {
        if is_excluded(operation.path, excluded_prefixes) {
            continue;
        }
        check_operation(spec_label, operation, &mut findings);
    }

    findings.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.message.cmp(&b.message))
    });
    findings
}

/// The taxonomy as it applies to one operation.
fn check_operation(spec_label: &str, operation: &SpecOperation<'_>, findings: &mut Vec<Finding>) {
    let identity = format!("{} {}", verb(operation.method), operation.path);
    let mut report = |message: String| {
        findings.push(Finding {
            file: spec_label.to_string(),
            line: None,
            message,
        });
    };

    if operation.tags.is_empty() {
        report(format!("{identity}: declares no tags"));
    }
    for tag in &operation.tags {
        if !KNOWN_TAGS.contains(tag) {
            report(format!("{identity}: unknown tag `{tag}`"));
        }
    }
    if is_data_api_path(operation.path) {
        if operation.tags.contains(&PAGE_TAG) {
            report(format!("{identity}: data-API path carries the `pages` tag"));
        }
    } else if !operation.tags.contains(&PAGE_TAG) {
        report(format!("{identity}: SPA page path must carry `pages`"));
    }
}
