//! Unit tests for the AST-based route analysis shared with `build.rs`.
//!
//! These are negative tests for the generator itself. The analysis decides
//! which handlers are registered in `paths(...)`; when it mis-parses, the
//! affected routes are mounted but absent from the spec, and the only
//! build-time signal is a coverage warning that is easy to miss. The
//! annotation check in particular used to be file-scoped — the whole file had
//! to contain the string `utoipa::path` — which credited a function with a
//! sibling's annotation and failed later as a missing `__path_*` import
//! instead of as a warning; `annotation_is_per_function_not_per_file` is the
//! test that fails under that rule.

#[path = "../../build/ast_scan.rs"]
mod ast_scan;

// `build.rs` declares `route_path` beside `ast_scan`, and the path-agreement
// check calls it as `super::route_path::to_spec_path` — this include is what
// makes that `super::` path resolve in the test crate.
#[path = "../../build/route_path.rs"]
mod route_path;

use std::path::Path;

use ast_scan::{Finding, HandlerRef, scan_handlers, scan_routes};

fn handler(module_path: &str, handler: &str) -> HandlerRef {
    HandlerRef {
        module_path: module_path.to_string(),
        handler: handler.to_string(),
    }
}

/// Run the scan and the path-agreement check the way `build.rs` combines
/// them: `path_mismatch` itself applies the shared `route_path::to_spec_path`
/// translation when deciding whether the Rocket URI and the annotation's
/// `path = "..."` agree.
fn scan_handlers_with_agreement(content: &str, file: &Path) -> ast_scan::HandlersScan {
    let mut scan = scan_handlers(content, file);
    for handler in &scan.handlers {
        if let Some(finding) = handler.path_mismatch(file) {
            scan.findings.push(finding);
        }
    }
    scan
}

/// Every Rocket URI declared under `src/router`, discovered by walking the
/// router sources with the AST pass — no hand-maintained route list. The
/// spec-agreement test in `tests/route_path.rs` translates these and
/// compares them with the committed spec.
pub(crate) fn declared_rocket_uris() -> std::collections::BTreeSet<String> {
    let router_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("router");
    let mut uris = std::collections::BTreeSet::new();
    for entry in walkdir::WalkDir::new(&router_root)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.path().extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        for handler in scan_handlers(&content, entry.path()).handlers {
            if let Some(uri) = handler.rocket_path {
                uris.insert(uri);
            }
        }
    }
    uris
}

/// Every finding rendered the way `build.rs` prints it: `cargo:warning=`
/// followed by [`Finding::message`].
fn messages(findings: &[Finding]) -> Vec<String> {
    findings.iter().map(Finding::message).collect()
}

#[test]
fn rocket_attribute_yields_verb_and_declared_path() {
    // The first string literal in the attribute is the URI, including Rocket's
    // `?<query>` suffix. The multi-line form is how the handlers with a
    // `data =` body are written in `router/`.
    let scan = scan_handlers(
        r#"
#[get("/get/get-data?<start>&<end>")]
pub async fn get_data() -> i32 { 0 }

#[put(
    "/put/edit_description",
    data = "<req>"
)]
pub fn edit_description() {}
"#,
        Path::new("src/router/router.rs"),
    );

    assert!(scan.findings.is_empty(), "{:?}", scan.findings);
    assert_eq!(scan.handlers.len(), 2);
    assert_eq!(scan.handlers[0].name, "get_data");
    assert_eq!(scan.handlers[0].verb, "get");
    assert_eq!(
        scan.handlers[0].rocket_path.as_deref(),
        Some("/get/get-data?<start>&<end>")
    );
    assert_eq!(scan.handlers[1].name, "edit_description");
    assert_eq!(scan.handlers[1].verb, "put");
    assert_eq!(
        scan.handlers[1].rocket_path.as_deref(),
        Some("/put/edit_description")
    );
    // A Rocket attribute alone is not an utoipa annotation.
    assert!(!scan.handlers[0].annotated);
    assert!(!scan.handlers[1].annotated);
}

#[test]
fn single_line_and_multi_line_routes_blocks_yield_the_same_entries() {
    // The exact shape that the previous line-based parser mangled: a
    // single-line `routes![a, b]` came back as one handler named "a, b" and
    // both routes silently vanished from the spec.
    let one_line = "fn generate() -> Vec<Route> { routes![a, b] }";
    let multi_line =
        "fn generate() -> Vec<Route> {\n    routes![\n        a,\n        b,\n    ]\n}";
    let trailing_comma = "fn generate() -> Vec<Route> { routes![a, b,] }";
    let expected = vec![handler("get", "a"), handler("get", "b")];

    for source in [one_line, multi_line, trailing_comma] {
        let scan = scan_routes(source, "get", Path::new("mod.rs"));
        assert_eq!(scan.handlers, expected, "{source:?}");
        assert!(scan.findings.is_empty(), "{source:?}");
    }
}

#[test]
fn qualified_entries_split_module_from_handler() {
    let scan = scan_routes(
        "fn generate() -> Vec<Route> {\n    routes![\n        get_list::get_tags,\n        login,\n    ]\n}",
        "get",
        Path::new("mod.rs"),
    );

    assert_eq!(
        scan.handlers,
        vec![handler("get_list", "get_tags"), handler("get", "login")]
    );
}

#[test]
fn every_block_in_a_file_is_scanned() {
    // Blocks appear as tail expressions and as `let r = routes![...]`
    // bindings (`router/put/mod.rs`), with comments inside the list
    // (`router/get/mod.rs`). All of them must contribute.
    let scan = scan_routes(
        "fn first() -> Vec<Route> { routes![a] }\nfn second() -> Vec<Route> {\n    // data API\n    let r = routes![\n        b,\n        c, // trailing comment\n    ];\n    r\n}\n",
        "post",
        Path::new("mod.rs"),
    );

    assert_eq!(
        scan.handlers,
        vec![
            handler("post", "a"),
            handler("post", "b"),
            handler("post", "c")
        ]
    );
    assert!(scan.findings.is_empty());
}

#[test]
fn statement_position_blocks_are_collected() {
    // `routes![a];` parses as `Stmt::Macro`, not as an expression or an
    // item: a visitor that only overrides the expression and item positions
    // drops it silently — no handler, no finding.
    let scan = scan_routes(
        "fn generate() -> Vec<Route> {\n    routes![a];\n    routes![b]\n}",
        "get",
        Path::new("mod.rs"),
    );

    assert_eq!(
        scan.handlers,
        vec![handler("get", "a"), handler("get", "b")]
    );
    assert!(scan.findings.is_empty(), "{:?}", scan.findings);
}

#[test]
fn annotation_is_per_function_not_per_file() {
    // The bug this replaces: the old check was `file contains "utoipa::path"
    // && file contains "fn unannotated("`, so `unannotated` inherited its
    // sibling's annotation. The build then imported a `__path_unannotated`
    // that utoipa never generated and failed with a confusing compile error
    // instead of the missing-annotation warning.
    let scan = scan_handlers(
        r#"
#[get("/annotated")]
#[utoipa::path(get, path = "/annotated")]
pub fn annotated() {}

#[get("/unannotated")]
pub fn unannotated() {}
"#,
        Path::new("src/router/router.rs"),
    );

    assert_eq!(scan.handlers.len(), 2);
    let annotated = scan
        .handlers
        .iter()
        .find(|handler| handler.name == "annotated")
        .expect("`annotated` is a route handler");
    let unannotated = scan
        .handlers
        .iter()
        .find(|handler| handler.name == "unannotated")
        .expect("`unannotated` is a route handler");
    assert!(annotated.annotated, "own annotation must count");
    assert!(
        !unannotated.annotated,
        "a sibling's annotation must not count for this function"
    );
}

#[test]
fn non_path_routes_entries_are_reported_not_guessed() {
    // A macro call, an index expression, a literal or a dangling `::` cannot
    // be resolved to a handler. Guessing would register a nonexistent
    // `__path_*` import; each such entry is reported instead. The bracketed
    // entry sits first and a resolvable handler after it: a scanner that
    // ended the block at the first `]` would drop `real_handler`.
    let scan = scan_routes(
        "fn generate() -> Vec<Route> { routes![computed[index], real_handler, some_macro!(), 42, \"literal\", trailing::] }",
        "get",
        Path::new("mod.rs"),
    );

    assert_eq!(scan.handlers, vec![handler("get", "real_handler")]);
    let reported = messages(&scan.findings);
    assert_eq!(reported.len(), 5, "{reported:?}");
    for entry in ["computed", "some_macro", "42", "literal", "trailing"] {
        assert!(
            reported.iter().any(|message| message
                .starts_with("ignoring unparsable routes![] entry: ")
                && message.contains(entry)),
            "expected an unparsable-entry finding containing {entry}: {reported:?}"
        );
    }
}

#[test]
fn duplicate_names_prefer_the_annotated_candidate() {
    // Several functions can share a name — a `#[cfg(test)]` duplicate or a
    // test-module helper declared before the real handler; the scan does not
    // evaluate cfgs. First-in-source order would pick the duplicate, find no
    // annotation and drop the real route from the spec.
    let scan = scan_handlers(
        r#"
#[cfg(test)]
#[get("/timeline-v2")]
pub fn timeline() {}

#[get("/timeline")]
#[utoipa::path(get, path = "/timeline")]
pub fn timeline() {}
"#,
        Path::new("src/router/get/get_page.rs"),
    );

    assert_eq!(
        scan.handlers
            .iter()
            .filter(|handler| handler.name == "timeline")
            .count(),
        2,
        "both duplicates are collected"
    );
    let chosen = scan
        .candidate("timeline")
        .expect("`timeline` is a route handler");
    assert!(
        chosen.annotated,
        "the annotated duplicate must win over the first-in-source one"
    );
    assert_eq!(chosen.rocket_path.as_deref(), Some("/timeline"));
    assert!(scan.candidate("no_such_handler").is_none());

    // With no annotated candidate at all the first in source order is used;
    // it then reports as missing its annotation, as before.
    let unannotated = scan_handlers(
        r#"
#[get("/a")]
pub fn alpha() {}

#[get("/b")]
pub fn alpha() {}
"#,
        Path::new("src/router/router.rs"),
    );
    let chosen = unannotated
        .candidate("alpha")
        .expect("`alpha` is a route handler");
    assert!(!chosen.annotated);
    assert_eq!(chosen.rocket_path.as_deref(), Some("/a"));
}

#[test]
fn attribute_paths_agree_after_known_normalisations() {
    // The shared `to_spec_path` translation erases the by-design differences
    // (Rocket `<param>`/`<param..>`/`<_param..>` versus OpenAPI `{param}`,
    // and the `?<query>` suffix). Handlers whose paths agree under it — or
    // that declare nothing to compare — produce no findings.
    let scan = scan_handlers_with_agreement(
        r#"
#[get("/get/metadata/<asset_id>?<timestamp>")]
#[utoipa::path(get, path = "/get/metadata/{asset_id}")]
pub fn get_metadata() {}

#[get("/share/<_path..>")]
#[utoipa::path(get, path = "/share/{path}")]
pub fn share() {}

#[get("/tags")]
#[utoipa::path(get, path = "/tags")]
pub fn tags() {}

#[get("/uncompared")]
#[utoipa::path(get, tag = "pages")]
pub fn uncompared() {}

#[get(rank = 11)]
#[utoipa::path(get, path = "/ranked")]
pub fn ranked() {}
"#,
        Path::new("src/router/router.rs"),
    );

    assert_eq!(scan.handlers.len(), 5);
    assert!(scan.findings.is_empty(), "{:?}", scan.findings);
    // The comparison being skipped must not silence the annotation check.
    assert!(scan.handlers[3].annotated);
    // No string literal in the Rocket attribute: nothing to compare either.
    assert_eq!(scan.handlers[4].rocket_path, None);
}

#[test]
fn genuinely_different_attribute_paths_are_flagged() {
    let scan = scan_handlers_with_agreement(
        r#"
#[get("/tags")]
#[utoipa::path(get, path = "/labels")]
pub fn tags() {}
"#,
        Path::new("src/router/get/get_list.rs"),
    );

    assert_eq!(scan.handlers.len(), 1, "the handler itself is still found");
    let reported = messages(&scan.findings);
    assert_eq!(reported.len(), 1, "{reported:?}");
    let message = &reported[0];
    assert!(message.starts_with("utoipa path mismatch"), "{message}");
    assert!(message.contains("tags"), "{message}");
    assert!(message.contains("\"/tags\""), "{message}");
    assert!(message.contains("\"/labels\""), "{message}");
    assert!(message.contains("src/router/get/get_list.rs"), "{message}");
}

#[test]
fn malformed_files_return_findings_instead_of_panicking() {
    // `build.rs` must never abort the build with a panic over a source it
    // cannot parse: the analysis reports the file and moves on.
    for source in ["pub fn broken( {", "fn generate() { routes![a, b"] {
        let routes = scan_routes(source, "get", Path::new("broken.rs"));
        assert!(routes.handlers.is_empty(), "{source:?}");
        assert_eq!(routes.findings.len(), 1, "{source:?}");
        let reported = messages(&routes.findings);
        assert!(
            reported[0].starts_with("failed to parse broken.rs"),
            "{reported:?}"
        );

        let handlers = scan_handlers(source, Path::new("broken.rs"));
        assert!(handlers.handlers.is_empty(), "{source:?}");
        assert_eq!(handlers.findings.len(), 1, "{source:?}");
        assert!(
            messages(&handlers.findings)[0].starts_with("failed to parse broken.rs"),
            "{source:?}"
        );
    }

    // An empty file parses: no handlers, and nothing to warn about.
    let empty = scan_routes("", "get", Path::new("empty.rs"));
    assert!(empty.handlers.is_empty());
    assert!(empty.findings.is_empty());
}

/// Guards the scan list in `build.rs` (moved from the `route_scan` tests with
/// the scanner): dropping a router module from it unmounts nothing but
/// silently documents nothing — the drift the contract gate reports.
#[test]
fn scanned_router_modules_cover_every_mounted_group() {
    let build_rs = Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs");
    let source = std::fs::read_to_string(&build_rs).expect("build.rs is readable");

    for module in [
        "get/mod.rs",
        "post/mod.rs",
        "put/mod.rs",
        "delete.rs",
        "auth.rs",
    ] {
        assert!(
            source.contains(&format!("\"{module}\"")),
            "build.rs no longer scans {module}; its routes would be mounted but \
             undocumented"
        );
    }
}

/// Guards the scan list in `build.rs` a second way: every entry must name a
/// file that exists under `router/`. `collect_all_routes` warns on an
/// unreadable entry instead of skipping it silently, but an entry pointing at
/// a nonexistent file (the dead `fairing/mod.rs` one, for instance) is a
/// mistake to catch at review time, not on every build.
#[test]
fn scan_list_entries_point_at_existing_files() {
    let build_rs = Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs");
    let source = std::fs::read_to_string(&build_rs).expect("build.rs is readable");
    let start = source
        .find("let mod_entries")
        .expect("build.rs declares mod_entries");
    let end = start
        + source[start..]
            .find("];")
            .expect("the mod_entries array is terminated");
    // `split('"')` alternates unquoted and quoted segments; after skipping
    // the leading unquoted one, every second segment is a quoted entry.
    let entries: Vec<&str> = source[start..end]
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|entry| Path::new(entry).extension().is_some_and(|ext| ext == "rs"))
        .collect();
    assert!(!entries.is_empty(), "no scan-list entries parsed");

    let router_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("router");
    for entry in entries {
        assert!(
            router_root.join(entry).exists(),
            "build.rs scans {entry}, which does not exist — drop the entry or \
             create the file"
        );
    }
}
