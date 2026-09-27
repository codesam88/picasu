//! Rocket route discovery and per-function `#[utoipa::path]` attribution.
//!
//! These cover the half of the contract gate that runs on source rather than on
//! a mounted route table: which HTTP verb and URI a handler declares, and
//! whether *that* function carries an `OpenAPI` annotation. Crediting a sibling
//! function's annotation is the failure this file guards against, because it
//! registers a route under someone else's metadata and no test notices until
//! the spec is read.

use openapi_sanity::{HandlerScan, HttpMethod, scan_handlers, scan_source};

fn handler<'a>(scan: &'a HandlerScan, name: &str) -> &'a openapi_sanity::Handler {
    scan.handlers
        .iter()
        .find(|handler| handler.name == name)
        .unwrap_or_else(|| panic!("no handler named `{name}` in {:?}", scan.handlers))
}

#[test]
fn route_attribute_yields_method_and_uri() {
    let scan = scan_handlers(
        "src/router/get/get_data.rs",
        r#"
        #[get("/get/get-data", format = "json", data = "<_body>")]
        pub async fn get_data(_body: String) {}
        "#,
    );

    let get_data = handler(&scan, "get_data");
    assert_eq!(get_data.method, Some(HttpMethod::Get));
    assert_eq!(get_data.uri.as_deref(), Some("/get/get-data"));
    assert!(!get_data.annotated);
    assert_eq!(get_data.spec_path, None);
}

#[test]
fn every_rocket_route_verb_is_recognized() {
    for method in [
        HttpMethod::Get,
        HttpMethod::Post,
        HttpMethod::Put,
        HttpMethod::Delete,
        HttpMethod::Patch,
        HttpMethod::Head,
        HttpMethod::Options,
    ] {
        let source = format!(
            "#[{method_attr}(\"/x\")]\npub async fn handler() {{}}",
            method_attr = method.as_str()
        );
        let scan = scan_handlers("src/router/x.rs", &source);

        assert_eq!(
            handler(&scan, "handler").method,
            Some(method),
            "verb {method:?} not recognized"
        );
    }
}

#[test]
fn a_qualified_route_attribute_is_the_same_route() {
    // `#[rocket::get]` and `#[get]` declare the same route; the qualifier is
    // import syntax, not a different contract.
    let scan = scan_handlers(
        "src/router/get/get_page.rs",
        "#[rocket::get(\"/albums/view/<_path..>\")]\npub async fn albums_view() {}\n",
    );

    let albums_view = handler(&scan, "albums_view");
    assert_eq!(albums_view.method, Some(HttpMethod::Get));
    assert_eq!(albums_view.uri.as_deref(), Some("/albums/view/<_path..>"));
}

#[test]
fn a_route_uri_with_parameters_is_kept_verbatim() {
    let scan = scan_handlers(
        "src/router/get/get_prefetch.rs",
        "#[get(\"/get/prefetch?<locate>\")]\npub async fn prefetch() {}\n",
    );

    assert_eq!(
        handler(&scan, "prefetch").uri.as_deref(),
        Some("/get/prefetch?<locate>")
    );
}

#[test]
fn a_route_attribute_without_a_uri_literal_is_reported() {
    let scan = scan_handlers(
        "src/router/get/get_data.rs",
        "#[get(URI)]\npub async fn get_data() {}\n",
    );

    let get_data = handler(&scan, "get_data");
    assert_eq!(get_data.method, Some(HttpMethod::Get));
    assert_eq!(get_data.uri, None);
    assert_eq!(scan.findings.len(), 1);
    assert_eq!(scan.findings[0].line, Some(1));
    assert!(
        scan.findings[0].message.contains("get_data"),
        "the diagnostic must name the handler, got: {}",
        scan.findings[0].message
    );
}

#[test]
fn an_annotated_handler_reports_its_own_spec_path() {
    let scan = scan_handlers(
        "src/router/post/authenticate.rs",
        r#"
        #[utoipa::path(
            post,
            path = "/post/authenticate",
            tag = "auth",
            responses((status = 200, description = "ok", body = String))
        )]
        #[post("/post/authenticate", format = "json", data = "<data>")]
        pub async fn authenticate(data: String) {}
        "#,
    );

    let authenticate = handler(&scan, "authenticate");
    assert!(authenticate.annotated);
    assert_eq!(
        authenticate.spec_path.as_deref(),
        Some("/post/authenticate")
    );
    assert_eq!(authenticate.method, Some(HttpMethod::Post));
    assert!(scan.findings.is_empty());
}

#[test]
fn an_annotated_handler_reports_its_own_annotation_verb() {
    // The verb of the annotation decides which operation utoipa registers the
    // handler under, so it is read separately from the route attribute: they are
    // the same in every correct handler and disagree in exactly the case the
    // contract gate has to notice.
    let scan = scan_handlers(
        "src/router/post/authenticate.rs",
        r#"
        #[utoipa::path(post, path = "/post/authenticate", tag = "auth")]
        #[post("/post/authenticate", format = "json", data = "<data>")]
        pub async fn authenticate(data: String) {}
        "#,
    );

    let authenticate = handler(&scan, "authenticate");
    assert_eq!(authenticate.spec_method, Some(HttpMethod::Post));
    assert_eq!(authenticate.method, Some(HttpMethod::Post));
    assert!(scan.findings.is_empty());
}

#[test]
fn the_annotation_verb_is_found_wherever_it_sits() {
    // utoipa takes the verb as a bare identifier among the annotation's
    // top-level tokens, and an identifier that names no verb must not end the
    // search — `params(..)` precedes the verb in a readable annotation.
    let scan = scan_handlers(
        "src/router/get/get_data.rs",
        r#"
        #[utoipa::path(
            params(("path" = String, Path)),
            get,
            path = "/get/get-data",
            responses((status = 200, description = "ok"))
        )]
        #[get("/get/get-data")]
        pub async fn get_data() {}
        "#,
    );

    assert_eq!(
        handler(&scan, "get_data").spec_method,
        Some(HttpMethod::Get)
    );
}

#[test]
fn a_verb_inside_a_nested_group_is_not_the_annotation_verb() {
    // `responses(..)` and `params(..)` are token groups, so an identifier inside
    // one cannot register the operation under a verb nobody declared.
    let scan = scan_handlers(
        "src/router/get/get_data.rs",
        r#"
        #[utoipa::path(
            path = "/get/get-data",
            responses((status = 200, description = "post")),
            params(("delete" = String, Query))
        )]
        #[get("/get/get-data")]
        pub async fn get_data() {}
        "#,
    );

    let get_data = handler(&scan, "get_data");
    assert!(get_data.annotated);
    assert_eq!(get_data.spec_method, None);
    assert_eq!(get_data.spec_path.as_deref(), Some("/get/get-data"));
    assert!(scan.findings.is_empty());
}

#[test]
fn an_unannotated_handler_declares_no_annotation_verb() {
    let scan = scan_handlers(
        "src/router/get/get_page.rs",
        "#[get(\"/setting\")]\npub async fn setting() {}\n",
    );

    let setting = handler(&scan, "setting");
    assert!(!setting.annotated);
    assert_eq!(setting.spec_method, None);
    assert_eq!(setting.spec_path, None);
}

#[test]
fn an_annotation_is_not_credited_to_a_sibling_function() {
    // The two functions live in one file; only one of them is annotated. A
    // file-level "does this module mention utoipa::path" check would register
    // the plain route under the annotated one's metadata.
    let scan = scan_handlers(
        "src/router/get/get_page.rs",
        r#"
        #[utoipa::path(get, path = "/login", tag = "pages")]
        #[get("/login")]
        pub async fn login() {}

        #[get("/setting")]
        pub async fn setting() {}
        "#,
    );

    assert!(handler(&scan, "login").annotated);
    assert_eq!(handler(&scan, "login").spec_path.as_deref(), Some("/login"));

    let setting = handler(&scan, "setting");
    assert!(!setting.annotated, "a sibling's annotation was credited");
    assert_eq!(setting.spec_path, None);
    assert_eq!(setting.uri.as_deref(), Some("/setting"));
}

#[test]
fn a_nested_path_key_is_not_mistaken_for_the_operation_path() {
    // `params(...)` and `responses(...)` are token groups, so an identifier
    // inside them cannot be read as the operation's own `path = "..."`.
    let scan = scan_handlers(
        "src/router/get/get_data.rs",
        r#"
        #[utoipa::path(
            get,
            tag = "timeline",
            params(("path" = String, Path, description = "A path")),
            responses((status = 200, description = "ok"))
        )]
        #[get("/get/get-data")]
        pub async fn get_data() {}
        "#,
    );

    let get_data = handler(&scan, "get_data");
    assert!(get_data.annotated);
    assert_eq!(
        get_data.spec_path, None,
        "the nested `path` was read as the operation path"
    );
    assert!(scan.findings.is_empty());
}

#[test]
fn an_annotated_handler_without_a_path_is_annotated_without_a_spec_path() {
    // utoipa may derive the path itself, so the absence of `path = "..."` is
    // reported as "no literal" and not as a malformed annotation.
    let scan = scan_handlers(
        "src/router/get/get_data.rs",
        "#[utoipa::path(get, tag = \"timeline\")]\n#[get(\"/get/get-data\")]\npub async fn get_data() {}\n",
    );

    let get_data = handler(&scan, "get_data");
    assert!(get_data.annotated);
    assert_eq!(get_data.spec_path, None);
    assert!(scan.findings.is_empty());
}

#[test]
fn only_a_crate_qualified_utoipa_annotation_counts() {
    // `#[path = "..."]` is the built-in module attribute, so the annotation is
    // matched on both path segments: a last-segment-only match would read a
    // `#[path]` on anything as a documented operation.
    let scan = scan_handlers(
        "src/router/get/mod.rs",
        "#[path = \"get_page.rs\"]\npub mod pages;\n",
    );
    assert!(scan.handlers.is_empty(), "{:?}", scan.handlers);
    assert!(scan.findings.is_empty());

    let scan = scan_handlers(
        "src/router/get/get_page.rs",
        "#[path = \"other.rs\"]\npub fn handler() {}\n",
    );
    assert!(scan.handlers.is_empty(), "{:?}", scan.handlers);
    assert!(scan.findings.is_empty());
}

#[test]
fn a_function_with_neither_a_route_nor_an_annotation_is_not_a_handler() {
    // Helpers, `FromRequest` impls and the like are not part of the contract,
    // so they do not appear in the inventory.
    let scan = scan_handlers(
        "src/router/get/get_data.rs",
        "pub(crate) fn helper() -> usize { 1 }\n\nfn other() {}\n",
    );

    assert!(scan.handlers.is_empty(), "{:?}", scan.handlers);
    assert!(scan.findings.is_empty());
}

#[test]
fn unparsable_source_is_reported_instead_of_panicking() {
    let scan = scan_handlers("src/router/get/get_data.rs", "pub async fn get_data( {");

    assert!(scan.handlers.is_empty());
    assert_eq!(scan.findings.len(), 1);
    assert_eq!(scan.findings[0].file, "src/router/get/get_data.rs");
    assert_eq!(scan.findings[0].line, Some(1));
}

#[test]
fn a_handler_line_is_stable() {
    // The line of the function name, not of its attributes: the diagnostic
    // points at the item being reported.
    let scan = scan_handlers(
        "src/router/get/get_page.rs",
        "\n\n#[get(\"/login\")]\npub async fn login() {}\n",
    );

    assert_eq!(handler(&scan, "login").line, 4);
}

#[test]
fn scan_source_answers_both_questions_in_one_pass() {
    let source = r#"
    pub fn generate_get_routes() -> Vec<Route> {
        routes![
            get_page::login,
        ]
    }

    #[utoipa::path(get, path = "/login", tag = "pages")]
    #[get("/login")]
    pub async fn login() {}
    "#;

    let scan = scan_source("src/router/get/get_page.rs", source, "get");

    assert_eq!(scan.routes.len(), 1);
    assert_eq!(scan.routes[0].handler, "login");
    assert_eq!(scan.routes[0].module_path, "get_page");
    assert_eq!(scan.handlers.len(), 1);
    assert!(scan.handlers[0].annotated);
    assert!(scan.findings.is_empty());
}

#[test]
fn scan_source_reports_a_parse_error_once() {
    let scan = scan_source("src/router/get/mod.rs", "routes![a, b", "get");

    assert!(scan.routes.is_empty());
    assert!(scan.handlers.is_empty());
    assert_eq!(scan.findings.len(), 1);
}
