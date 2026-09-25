//! Unit tests for the `routes![]` scanner shared with `build.rs`.
//!
//! These are negative tests for the generator itself. The scanner decides which
//! handlers are registered in `paths(...)`; when it mis-parses, the affected
//! routes are mounted but absent from the spec, and the only build-time signal
//! is a coverage warning that is easy to miss. The one-line form
//! `routes![a, b]` shipped that way: both `POST /post/renew-hash-token` and
//! `POST /post/renew-timestamp-token` were undocumented for as long as the
//! single-line block in `router/auth.rs` existed.

use std::path::Path;

#[path = "../../build/route_scan.rs"]
mod route_scan;

use route_scan::{HandlerRef, scan_routes};

fn handler(module_path: &str, handler: &str) -> HandlerRef {
    HandlerRef {
        module_path: module_path.to_string(),
        handler: handler.to_string(),
    }
}

#[test]
fn single_line_block_registers_every_handler() {
    // The exact shape in router/auth.rs. A line-based parser yields one entry
    // named "renew_timestamp_token, renew_hash_token" and drops both routes.
    let scanned = scan_routes(
        "pub fn generate_fairing_routes() -> Vec<Route> {\n    routes![renew_timestamp_token, renew_hash_token]\n}\n",
        "auth",
    );

    assert_eq!(
        scanned,
        vec![
            handler("auth", "renew_timestamp_token"),
            handler("auth", "renew_hash_token")
        ]
    );
}

#[test]
fn multi_line_block_registers_every_handler() {
    let scanned = scan_routes(
        "pub fn generate_get_routes() -> Vec<Route> {\n    routes![\n        get_list::get_tags,\n        get_list::get_albums,\n        get_page::login,\n    ]\n}\n",
        "get",
    );

    assert_eq!(
        scanned,
        vec![
            handler("get_list", "get_tags"),
            handler("get_list", "get_albums"),
            handler("get_page", "login"),
        ]
    );
}

#[test]
fn layout_does_not_change_the_result() {
    let one_line = "routes![a, b, c]";
    let multi_line = "routes![\n    a,\n    b,\n    c,\n]";
    let trailing_comma = "routes![a, b, c,]";

    let expected = vec![
        handler("get", "a"),
        handler("get", "b"),
        handler("get", "c"),
    ];
    assert_eq!(scan_routes(one_line, "get"), expected);
    assert_eq!(scan_routes(multi_line, "get"), expected);
    assert_eq!(scan_routes(trailing_comma, "get"), expected);
}

#[test]
fn comments_and_blank_lines_are_ignored() {
    let scanned = scan_routes(
        "routes![\n    // data API\n    get_data::get_data,\n\n    // page routes follow\n    get_page::login, // serves index.html\n]\n",
        "get",
    );

    assert_eq!(
        scanned,
        vec![
            handler("get_data", "get_data"),
            handler("get_page", "login")
        ]
    );
}

#[test]
fn every_block_in_a_file_is_scanned() {
    let scanned = scan_routes(
        "fn first() -> Vec<Route> { routes![a] }\nfn second() -> Vec<Route> {\n    routes![\n        b,\n        c,\n    ]\n}\n",
        "post",
    );

    assert_eq!(
        scanned,
        vec![
            handler("post", "a"),
            handler("post", "b"),
            handler("post", "c")
        ]
    );
}

#[test]
fn nested_brackets_do_not_end_the_block_early() {
    // Bracket depth is tracked so an ignorable entry containing `[` cannot cut
    // the block short and hide the handlers after it.
    let scanned = scan_routes("routes![first, computed[index], second]", "get");

    assert_eq!(
        scanned,
        vec![handler("get", "first"), handler("get", "second")]
    );
}

#[test]
fn non_literal_entries_are_skipped_not_guessed() {
    // A macro argument, an index expression or a literal cannot be resolved to
    // a handler. Registering a bogus `__path_*` for it would break the build;
    // skipping it keeps the module compiling, and the entry is reported as a
    // build warning.
    let scanned = scan_routes(
        "routes![real_handler, some_macro!(), computed[index], 42, \"literal\", trailing::]",
        "get",
    );

    assert_eq!(scanned, vec![handler("get", "real_handler")]);
}

#[test]
fn unterminated_block_yields_nothing_instead_of_panicking() {
    assert!(scan_routes("routes![a, b", "get").is_empty());
    assert!(scan_routes("no routes macro here", "get").is_empty());
}

/// Guards the scanner against the modules that carry the real route tables:
/// dropping one from the scan list unmounts nothing but silently documents
/// nothing, which is the drift the contract gate reports.
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
