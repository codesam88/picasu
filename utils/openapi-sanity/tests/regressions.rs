//! Incidents that already happened, pinned in the shape they happened in.
//!
//! Every other test in this crate drives a *rule* — a fixture carrying one
//! instance of each failure mode, or a mutation of a conforming tree. This file
//! drives the *incidents*: source shapes the repository actually shipped with,
//! each of which passed the checks available at the time. A rule test says a
//! check works; an incident test says the check still finds the thing it was
//! written for, so a refactor of the parser, of the module list or of a
//! diagnostic cannot quietly stop.
//!
//! Three rules hold for every test here:
//!
//! - the source is the *minimal* shape of the real defect, not a generic variant
//!   of it, so a pass is evidence about the incident;
//! - the finding is asserted exactly — file, line and message — so a rule that
//!   moves its anchor or rewords itself fails here;
//! - the same shape, conforming, is asserted silent, so a test cannot pass
//!   against a checker that flags everything.
//!
//! Where an incident is already pinned in its own right, the doc comment says so
//! and names the test. What is added here is the part that test does not reach:
//! the consequence the incident had, end to end, or the property from the side of
//! the constant that declares it.

use std::path::{Path, PathBuf};

use openapi_sanity::{AuthRule, GuardClass, SCANNED_MODULES};

mod support;

use support::{Fixture, write_tree};

/// The router root the crate's own default gate invocation reads.
const REPOSITORY_ROUTER: &str = "backend/src/router";

/// The contract gate's report for a tree, as it prints it.
fn contract_findings(tree: &Fixture) -> Vec<String> {
    tree.findings(&[])
}

/// The auth gate's report for a tree against `policy`.
fn auth_findings(tree: &Fixture, policy: &[AuthRule]) -> Vec<String> {
    tree.auth_findings(&[], policy)
}

/// The tag gate's report for a tree's document.
fn tag_findings(tree: &Fixture) -> Vec<String> {
    tree.tag_findings(&[])
}

/// The report a conforming tree produces, for the assertions that require silence.
///
/// Spelled out rather than written inline: `Vec::new()` has an ambiguous element
/// type under `PartialEq`, and rustc parses `Vec::<String>::new()` as a comparison
/// in this position.
fn no_findings() -> Vec<String> {
    Vec::new()
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits in <repo>/utils/openapi-sanity")
        .to_path_buf()
}

// ── A module list entry whose file no longer exists ───────────────────────────

/// `c6e599a9` ("attempt refactor") flattened `router/fairing/*` into
/// `router/auth.rs`, taking the token-renewal route table with it. The build
/// script's module list kept `("fairing", "fairing/mod.rs")`, and
/// `collect_all_routes` did `let Ok(content) = read_to_string(&path) else {
/// continue }`, so the dead entry shrank the scanned set with nothing in the build
/// log. It surfaced when the list moved into this crate in `af6578d5`; the entry was
/// removed and `every_scanned_router_module_exists` was added to
/// `backend/src/tests/route_scan.rs`.
///
/// That check lives in the backend, which is where the file lives. The constant is
/// declared here, so the crate that owns it holds the same property: a consumer of
/// `SCANNED_MODULES` resolving it against a tree of its own gets a failing test out
/// of `cargo test -p openapi-sanity`, rather than a gate invocation that has to be
/// run for a dead entry to fail at all.
#[test]
fn every_scanned_module_names_a_file_that_exists() {
    let router = repository_root().join(REPOSITORY_ROUTER);

    assert!(
        !SCANNED_MODULES.is_empty(),
        "an empty list scans nothing and compares nothing"
    );
    for (group, relative) in SCANNED_MODULES {
        let path = router.join(relative);

        assert!(
            path.is_file(),
            "SCANNED_MODULES lists {relative} (group `{group}`), but {} does not exist; \
             a module nothing can read is a module whose routes are mounted but \
             undocumented",
            path.display()
        );
    }
}

// ── A group-root route table on one line ──────────────────────────────────────

/// The route table of `backend/src/router/auth.rs` as `c6e599a9` wrote it: a
/// group-root file, so its unqualified entries resolve to its own module, and one
/// line, so a parser that splits `routes![]` on newlines reads a single handler
/// named `"renew_timestamp_token, renew_hash_token"`. The pre-`efc2bfe5`
/// `collect_all_routes` did exactly that, and `auth.rs` was in no module list entry
/// either, so `POST /post/renew-timestamp-token` and `POST /post/renew-hash-token`
/// were mounted and undocumented with nothing to say so — until the backend's
/// runtime parity test found them.
///
/// `single_line_block_registers_every_handler` in `tests/routes.rs` already pins the
/// parse of exactly this line. What nothing pins is the consequence: a group-root
/// module as the route table, driven through the contract gate. Every tree under
/// `tests/fixtures/` is a `get/` group, so no check in this crate has compared a
/// group-root `routes![]` block with the document in either direction.
#[test]
fn a_group_root_route_table_on_one_line_reaches_the_document() {
    // The module with the real annotations: each handler declares the path and verb
    // of the route it sits on, so neither the annotation nor the route is the reason
    // a finding appears below.
    let source = r#"use rocket::post;

pub fn generate_fairing_routes() -> Vec<Route> {
    routes![renew_timestamp_token, renew_hash_token]
}

#[utoipa::path(
    post,
    path = "/post/renew-timestamp-token",
    tag = "auth",
    responses((status = 200, description = "Token renewed"), (status = 401, response = Unauthorized))
)]
#[post("/post/renew-timestamp-token", format = "json", data = "<token_request>")]
pub async fn renew_timestamp_token(auth: GuardResult<GuardShare>) {
    let _ = auth?;
}

#[utoipa::path(
    post,
    path = "/post/renew-hash-token",
    tag = "auth",
    responses((status = 200, description = "Token renewed"), (status = 401, response = Unauthorized))
)]
#[post("/post/renew-hash-token", format = "json", data = "<token_request>")]
pub async fn renew_hash_token(auth: TimestampGuardModified) {}
"#;
    let both_documented = r#"{
  "openapi": "3.1.0",
  "paths": {
    "/post/renew-hash-token": {
      "post": {
        "operationId": "renew_hash_token",
        "tags": ["auth"],
        "responses": { "200": { "description": "Token renewed" } }
      }
    },
    "/post/renew-timestamp-token": {
      "post": {
        "operationId": "renew_timestamp_token",
        "tags": ["auth"],
        "responses": { "200": { "description": "Token renewed" } }
      }
    }
  }
}
"#;

    let conforming = write_tree(
        "group-root-route-table",
        &[("auth.rs", source)],
        both_documented,
    );

    assert_eq!(
        contract_findings(&conforming),
        no_findings(),
        "both operations are annotated, registered and documented, so a group-root \
         route table on one line is read like any other"
    );
    assert_eq!(tag_findings(&conforming), no_findings());

    // The state the incident left the repository in: the operations are annotated and
    // registered, and the document carries neither. A route table the scanner cannot
    // see makes these two findings the only trace of the omission.
    let undocumented = write_tree(
        "group-root-route-table-undocumented",
        &[("auth.rs", source)],
        r#"{
  "openapi": "3.1.0",
  "paths": {}
}
"#,
    );

    assert_eq!(
        contract_findings(&undocumented),
        vec![
            format!(
                "{}:14: auth::renew_timestamp_token: POST /post/renew-timestamp-token is \
                 declared in source but absent from the spec",
                undocumented.label("auth.rs")
            ),
            format!(
                "{}:25: auth::renew_hash_token: POST /post/renew-hash-token is declared \
                 in source but absent from the spec",
                undocumented.label("auth.rs")
            ),
        ],
        "both renewal routes are named individually, which is the whole of the \
         incident: one unread line took two documented operations with it"
    );
}

// ── A sibling function's annotation credited to an unannotated handler ─────────

/// Before `84ad1068`, `has_annotation` was
/// `content.contains("utoipa::path") && content.contains(&format!("fn {handler}("))`
/// over the whole handler file. In a file where one handler is annotated — the normal
/// case, and the shape of `router/get/get_page.rs` — that answered yes for every
/// sibling, so a registered handler with no annotation of its own counted as
/// documented on a neighbour's authority.
///
/// `an_annotation_is_not_credited_to_a_sibling_function` in `tests/handlers.rs`
/// already pins the attribution itself. What it does not pin is the diagnostic the
/// gate then produces, and the two are distinguishable: an unannotated handler is
/// reported against its own function, whereas a handler credited with a sibling's
/// annotation forms its operation from the route attribute alone and is reported as
/// document drift instead. A credit therefore shows up as the wrong one of the two,
/// not as an absence of findings.
#[test]
fn an_unannotated_sibling_is_reported_as_unannotated_not_as_document_drift() {
    // The two functions of the incident, in the file they share.
    let unannotated = r#"use rocket::get;

#[utoipa::path(get, path = "/login", tag = "pages")]
#[get("/login")]
pub async fn login() {}

#[get("/setting")]
pub async fn setting() {}
"#;
    // The same file with the sibling annotated, so the only variable left is whether
    // the handler has an annotation of its own.
    let annotated = unannotated.replace(
        "#[get(\"/setting\")]",
        "#[utoipa::path(get, path = \"/setting\", tag = \"pages\")]\n#[get(\"/setting\")]",
    );
    let mod_rs = r"pub mod page;

pub fn generate_get_routes() -> Vec<Route> {
    routes![
        page::login,
        page::setting,
    ]
}
";
    // The document the incident's state produced: a route with no annotation of its own
    // never reaches `paths(...)`, so the document carries `login` only.
    let login_only = r#"{
  "openapi": "3.1.0",
  "paths": {
    "/login": {
      "get": {
        "operationId": "login",
        "tags": ["pages"],
        "responses": { "200": { "description": "Login page" } }
      }
    }
  }
}
"#;
    // And the document that makes the annotated version conforming.
    let both = r#"{
  "openapi": "3.1.0",
  "paths": {
    "/login": {
      "get": {
        "operationId": "login",
        "tags": ["pages"],
        "responses": { "200": { "description": "Login page" } }
      }
    },
    "/setting": {
      "get": {
        "operationId": "setting",
        "tags": ["pages"],
        "responses": { "200": { "description": "Settings page" } }
      }
    }
  }
}
"#;

    let tree = write_tree(
        "sibling-annotation",
        &[("get/mod.rs", mod_rs), ("get/page.rs", unannotated)],
        login_only,
    );
    let drifted = write_tree(
        "sibling-annotation-annotated",
        &[("get/mod.rs", mod_rs), ("get/page.rs", &annotated)],
        login_only,
    );
    let conforming = write_tree(
        "sibling-annotation-conforming",
        &[("get/mod.rs", mod_rs), ("get/page.rs", &annotated)],
        both,
    );

    assert_eq!(
        contract_findings(&tree),
        vec![format!(
            "{}:8: page::setting: registered in routes![] but the function carries no \
             #[utoipa::path] annotation",
            tree.label("get/page.rs")
        )],
        "a handler with no annotation is reported against its own function; the \
         annotation beside it in the same file is not evidence about it"
    );
    assert_eq!(
        contract_findings(&drifted),
        vec![format!(
            "{}:9: page::setting: GET /setting is declared in source but absent from \
             the spec",
            drifted.label("get/page.rs")
        )],
        "with an annotation of its own the handler declares an operation and the \
         diagnostic names the document instead. The credit is visible as this line \
         where the unannotated handler produced the one above, which is why a \
         file-level check cannot be what the first assertion is measuring."
    );
    assert_eq!(
        contract_findings(&conforming),
        no_findings(),
        "both siblings annotated and both operations documented is the conforming \
         version of this exact shape"
    );
}

// ── A handler that drops a deferred guard, with a correct annotation and spec ──

/// `84f29aa5` rewrote `GET /get/get-rows` and `GET /get/get-scroll-bar` from
/// `_auth: GuardTimestamp` to `GuardResult<GuardTimestamp>` plus `let _ = auth;`,
/// while converting sibling handlers in the same commit to `let _ = auth?;`. Both
/// handlers' annotations declared `(status = 401, response = Unauthorized)` and the
/// document documented the rejection, so the spec and the handler signature were
/// correct throughout and both routes answered an unauthenticated caller `200`.
/// Recorded in `.plan/bug-get-rows-auth-guard-discarded.md`.
///
/// `a_guard_result_dropped_by_a_wildcard_let_is_discarded` in `tests/guards.rs` pins
/// the binding and `dropping_a_guard_result_fails` in `tests/mutations.rs` pins the
/// auth gate over a mutation of a conforming tree. Neither is the incident's own
/// shape end to end, and the `unauthored/` fixture does not carry it either: there
/// `get_rows` has no guard parameter at all — the missing-guard mode — and it is
/// `get_scroll_bar` that drops one. The timestamp guard of `/get/get-rows`, dropped
/// rather than absent, is in no tree in this crate.
///
/// The document below is the point: it is correct, and byte-identical between the two
/// states the test drives. The only difference is what the body does with `auth`, and
/// it is the whole difference between a finding and silence.
#[test]
fn a_dropped_timestamp_guard_is_found_with_a_correct_annotation_and_spec() {
    let dropped = r#"use rocket::get;

#[utoipa::path(
    get,
    path = "/get/get-rows",
    tag = "timeline",
    responses((status = 200, description = "Rows"), (status = 401, response = Unauthorized))
)]
#[get("/get/get-rows")]
pub async fn get_rows(auth: GuardResult<GuardTimestamp>) {
    let _ = auth;
}
"#;
    // The sibling `get_data`'s propagation, which is the one character the incident
    // lost.
    let propagated = dropped.replace("let _ = auth;", "let _ = auth?;");
    let mod_rs = r"pub mod data;

pub fn generate_get_routes() -> Vec<Route> {
    routes![
        data::get_rows,
    ]
}
";
    // The document the annotation produces: the rejection is declared, so every
    // document-shaped rule is satisfied and none of them can see the defect.
    let document = r##"{
  "openapi": "3.1.0",
  "paths": {
    "/get/get-rows": {
      "get": {
        "operationId": "get_rows",
        "tags": ["timeline"],
        "responses": {
          "200": { "description": "Rows" },
          "401": { "$ref": "#/components/responses/Unauthorized" }
        }
      }
    }
  }
}
"##;
    let policy: &[AuthRule] = &[AuthRule::guarded("get_rows", &[GuardClass::Timestamp])];
    let discarded = |tree: &Fixture| {
        vec![format!(
            "{}:10: get_rows: the deferred guard GuardTimestamp is bound to `auth` and \
             never enforced — `let _ = auth;` drops its failure; propagate it with `?`, \
             return it, or match on it",
            tree.label("get/data.rs")
        )]
    };

    let tree = write_tree(
        "dropped-get-rows",
        &[("get/mod.rs", mod_rs), ("get/data.rs", dropped)],
        document,
    );
    let fixed = write_tree(
        "dropped-get-rows-propagated",
        &[("get/mod.rs", mod_rs), ("get/data.rs", &propagated)],
        document,
    );

    assert_eq!(
        auth_findings(&tree, policy),
        discarded(&tree),
        "the guard is in the signature and its failure is thrown away, which is the \
         defect the incident was"
    );
    assert_eq!(
        contract_findings(&tree),
        discarded(&tree),
        "the contract gate reports the same line from the source scan and nothing \
         else: the annotation, the route and the document agree, so the document is not \
         what finds this"
    );
    assert_eq!(
        tag_findings(&tree),
        no_findings(),
        "the annotation carries the subject the taxonomy names"
    );
    assert_eq!(
        auth_findings(&fixed, policy),
        no_findings(),
        "the same tree with the sibling's `let _ = auth?;` is silent, so the finding \
         above is the dropped failure and not the shape"
    );
    assert_eq!(
        contract_findings(&fixed),
        no_findings(),
        "the document is byte-identical to the state above, so the contract gate is \
         silent on a conforming handler for the same reason it was not silent here"
    );
}
