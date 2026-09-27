//! `routes![...]` scanning: which handlers the build script may register in
//! `paths(...)`.
//!
//! Negative tests for the generator. A mis-parse here is silent by nature: the
//! affected routes stay mounted and simply vanish from the spec, with a coverage
//! warning as the only signal. The one-line form `routes![a, b]` shipped that
//! way — `POST /post/renew-hash-token` and `POST /post/renew-timestamp-token`
//! were undocumented for as long as the single-line block in `router/auth.rs`
//! existed.

use openapi_sanity::{HandlerRef, scan_routes};

fn handler(module_path: &str, handler: &str) -> HandlerRef {
    HandlerRef {
        module_path: module_path.to_string(),
        handler: handler.to_string(),
    }
}

/// A `routes![...]` body inside a function, which is where Rocket route tables
/// live. `source` is parsed as a complete file, so a bare block is not valid
/// input on its own.
fn in_function(body: &str) -> String {
    format!("pub fn generate_routes() -> Vec<Route> {{\n{body}\n}}\n")
}

#[test]
fn single_line_block_registers_every_handler() {
    // The exact shape in router/auth.rs. A line-based parser yields one entry
    // named "renew_timestamp_token, renew_hash_token" and drops both routes.
    let scan = scan_routes(
        "src/router/auth.rs",
        "pub fn generate_fairing_routes() -> Vec<Route> {\n    routes![renew_timestamp_token, renew_hash_token]\n}\n",
        "auth",
    );

    assert_eq!(
        scan.handlers,
        vec![
            handler("auth", "renew_timestamp_token"),
            handler("auth", "renew_hash_token")
        ]
    );
    assert!(scan.findings.is_empty());
}

#[test]
fn a_block_at_item_position_is_scanned() {
    // A `routes!` block is a statement wherever it is; both positions mount the
    // same routes and must register the same handlers.
    let scan = scan_routes("src/router/delete.rs", "routes![delete_data];\n", "delete");

    assert_eq!(scan.handlers, vec![handler("delete", "delete_data")]);
    assert!(scan.findings.is_empty());
}

#[test]
fn multi_line_block_registers_every_handler() {
    let scan = scan_routes(
        "src/router/get/mod.rs",
        "pub fn generate_get_routes() -> Vec<Route> {\n    routes![\n        get_list::get_tags,\n        get_list::get_albums,\n        get_page::login,\n    ]\n}\n",
        "get",
    );

    assert_eq!(
        scan.handlers,
        vec![
            handler("get_list", "get_tags"),
            handler("get_list", "get_albums"),
            handler("get_page", "login"),
        ]
    );
    assert!(scan.findings.is_empty());
}

#[test]
fn layout_does_not_change_the_result() {
    let one_line = "routes![a, b, c]";
    let multi_line = "routes![\n    a,\n    b,\n    c,\n]";
    let trailing_comma = "routes![a, b, c,]";
    let spaced = "routes![\n  a , b ,\n  c\n]\n";

    let expected = vec![
        handler("get", "a"),
        handler("get", "b"),
        handler("get", "c"),
    ];
    for body in [one_line, multi_line, trailing_comma, spaced] {
        let scan = scan_routes("src/router/get/mod.rs", &in_function(body), "get");
        assert_eq!(scan.handlers, expected, "body: {body}");
        assert!(scan.findings.is_empty(), "body: {body}");
    }
}

#[test]
fn comments_and_blank_lines_are_ignored() {
    let scan = scan_routes(
        "src/router/get/mod.rs",
        &in_function(
            "routes![\n    // data API\n    get_data::get_data,\n\n    // page routes follow\n    get_page::login, // serves index.html\n]",
        ),
        "get",
    );

    assert_eq!(
        scan.handlers,
        vec![
            handler("get_data", "get_data"),
            handler("get_page", "login")
        ]
    );
    assert!(scan.findings.is_empty());
}

#[test]
fn every_block_in_a_file_is_scanned() {
    let scan = scan_routes(
        "src/router/get/mod.rs",
        "fn first() -> Vec<Route> { routes![a] }\nfn second() -> Vec<Route> {\n    routes![\n        b,\n        c,\n    ]\n}\n",
        "post",
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
fn nested_brackets_do_not_end_the_block_early() {
    // Commas inside a nested token tree belong to that tree, so an ignorable
    // entry cannot swallow the handlers after it.
    let scan = scan_routes(
        "src/router/get/mod.rs",
        &in_function("routes![first, computed[index], second]"),
        "get",
    );

    assert_eq!(
        scan.handlers,
        vec![handler("get", "first"), handler("get", "second")]
    );
    assert_eq!(scan.findings.len(), 1, "the indexed entry is reported once");
}

#[test]
fn malformed_entries_are_reported_not_guessed() {
    // A macro call, an index expression or a literal cannot name a handler.
    // Registering a bogus `__path_*` for it would break the build; skipping it
    // without a diagnostic would hide the omission, so each one is reported.
    let scan = scan_routes(
        "src/router/get/mod.rs",
        "routes![real_handler, some_macro!(), computed[index], 42, \"literal\", trailing::];\n",
        "get",
    );

    assert_eq!(scan.handlers, vec![handler("get", "real_handler")]);
    assert_eq!(scan.findings.len(), 5, "got: {:?}", scan.findings);
    for finding in &scan.findings {
        assert_eq!(finding.file, "src/router/get/mod.rs");
        assert_eq!(finding.line, Some(1), "line must be stable");
        assert!(
            finding.message.contains("routes![] entry"),
            "unhelpful message: {}",
            finding.message
        );
    }
}

#[test]
fn unparsable_file_is_reported_without_handlers() {
    // A truncated block is a syntax error, not a block with fewer entries: it is
    // reported rather than scanned as far as it goes.
    let scan = scan_routes(
        "src/router/get/mod.rs",
        "pub fn routes() { routes![a, b",
        "get",
    );

    assert!(scan.handlers.is_empty());
    assert_eq!(scan.findings.len(), 1);
    assert_eq!(scan.findings[0].file, "src/router/get/mod.rs");
    assert_eq!(scan.findings[0].line, Some(1));
}

#[test]
fn a_file_without_a_routes_macro_yields_nothing() {
    let scan = scan_routes(
        "src/router/mod.rs",
        "pub mod get;\npub fn build() {}\n",
        "get",
    );

    assert!(scan.handlers.is_empty());
    assert!(scan.findings.is_empty());
}

#[test]
fn a_qualified_routes_macro_is_still_scanned() {
    // Rocket re-exports the macro, so a `rocket::routes!` block mounts routes
    // exactly like a local `routes!` block does.
    let scan = scan_routes(
        "src/router/builder.rs",
        &in_function("app.mount(\"/\", rocket::routes![assets])"),
        "get",
    );

    assert_eq!(scan.handlers, vec![handler("get", "assets")]);
    assert!(scan.findings.is_empty());
}
