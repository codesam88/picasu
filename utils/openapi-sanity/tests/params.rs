//! The parameter gate: one test per way a handler's declared input disagrees with
//! the document's.
//!
//! Every rule is driven from a *conforming* baseline and the failing shape is one
//! edit away from it, which is what makes a rule that stopped reporting — or
//! started reporting for a tree it should pass — a test failure rather than a
//! quieter gate. The `clean/` tree is the shared baseline: its `/get/metadata/
//! {asset_id}` and `?<path>` are the two shapes P1 and P2 have to get right
//! before they can say anything about a drift.
//!
//! The remaining rules are driven from trees written here, because no checked-in
//! fixture carries a request body: adding one to `clean/` would mean adding an
//! operation to `CLEAN_POLICY` and a guard to the tree, which changes the auth
//! gate's baseline for a rule the auth gate does not own. The trees are written to
//! `CARGO_TARGET_TMPDIR` and are the only place in the crate where a handler's
//! `data = "<x>"` argument and a document's `components/schemas` exist.
//!
//! # The trees are one group
//!
//! Every tree is `write/mod.rs` plus `write/write.rs`, so a `routes![...]` entry
//! resolves against the file that declares the handler and a finding's label is
//! the same shape in every test. Each handler is on a line the test states: the
//! finding anchors at the function, and the `write/write.rs` snippets below say
//! which line that is by their shape.

use std::path::Path;

use openapi_sanity::{
    ArgKind, BodySchema, ParameterLocation, SchemaIndex, check_params, route_query_bindings,
    scan_source, schema_index, spec_operations, spec_placeholders,
};

mod support;

use support::{Fixture, TEST_PREFIX, param_findings_against, write_tree};

/// A router tree and document that agree about paths, queries, bodies and ids.
const CLEAN: &str = "clean";

/// The prefix the public artifact omits on purpose, as the gate is run in the
/// repository.
const REPOSITORY_PREFIX: &str = "/get/test/";

/// The route table of the four-handler trees: a named JSON body, a multipart
/// form body, a handler taking arbitrary JSON, and a route with no body at all.
///
/// The four shapes are the ones P3 has to tell apart, and the two that need a
/// document half — a form body and an arbitrary-JSON body — are the two ways
/// utoipa renders `request_body = Value`.
const ROUTE_TABLE: &str = r"pub mod write;

pub fn generate_write_routes() -> Vec<Route> {
    routes![
        write::create_album,
        write::upload,
        write::import_archive,
        write::get_album,
    ]
}
";

/// The route table of the single-handler trees. Three of the four P1/P2/P4 shapes
/// are about one route, and a table registering handlers the tree does not define
/// would put `check_contract`'s "declared in no scanned source file" finding in the
/// middle of their reports.
const ONE_ROUTE_TABLE: &str = r"pub mod write;

pub fn generate_write_routes() -> Vec<Route> {
    routes![
        write::get_album,
    ]
}
";

/// The four handlers in the conforming shape. `create_album` is on line 5,
/// `upload` on 9, `import_archive` on 13 and `get_album` on 17.
const DECLARING_HANDLERS: &str = r#"use rocket::post;

#[utoipa::path(post, path = "/write/create", tag = "albums", request_body = CreateAlbum)]
#[post("/write/create", format = "json", data = "<album>")]
pub fn create_album(_auth: GuardAuth, album: Json<CreateAlbum>) {}

#[utoipa::path(post, path = "/write/upload", tag = "upload", request_body = Value)]
#[post("/write/upload", data = "<form>")]
pub fn upload(_auth: GuardAuth, form: Result<Form<UploadForm<'_>>, Errors<'_>>) {}

#[utoipa::path(post, path = "/write/import", tag = "albums", request_body = Value)]
#[post("/write/import", format = "json", data = "<raw>")]
pub fn import_archive(_auth: GuardAuth, raw: Json<serde_json::Value>) {}

#[utoipa::path(get, path = "/write/albums/{album_id}", tag = "albums")]
#[get("/write/albums/<album_id>")]
pub fn get_album(_auth: GuardAuth, album_id: u32) {}
"#;

/// The document those four handlers produce: one named component schema, the
/// multipart media type for the form body, an untyped body for the handler that
/// takes arbitrary JSON, and no body for the route that takes none.
const DECLARING_DOCUMENT: &str = r##"{
  "openapi": "3.1.0",
  "paths": {
    "/write/albums/{album_id}": {
      "get": {
        "operationId": "get_album",
        "tags": ["albums"],
        "parameters": [
          { "in": "path", "name": "album_id", "required": true, "schema": { "type": "integer" } }
        ],
        "responses": { "200": { "description": "Album" } }
      }
    },
    "/write/create": {
      "post": {
        "operationId": "create_album",
        "tags": ["albums"],
        "requestBody": {
          "required": true,
          "content": {
            "application/json": { "schema": { "$ref": "#/components/schemas/CreateAlbum" } }
          }
        },
        "responses": { "200": { "description": "Created" } }
      }
    },
    "/write/import": {
      "post": {
        "operationId": "import_archive",
        "tags": ["albums"],
        "requestBody": {
          "required": true,
          "content": { "application/json": { "schema": {} } }
        },
        "responses": { "200": { "description": "Imported" } }
      }
    },
    "/write/upload": {
      "post": {
        "operationId": "upload",
        "tags": ["upload"],
        "requestBody": {
          "required": true,
          "content": {
            "multipart/form-data": { "schema": {} }
          }
        },
        "responses": { "200": { "description": "Uploaded" } }
      }
    }
  },
  "components": {
    "schemas": {
      "CreateAlbum": { "type": "object" }
    }
  }
}
"##;

// ── A conforming tree ─────────────────────────────────────────────────────────

#[test]
fn the_clean_tree_documents_every_parameter_its_routes_bind() {
    assert_eq!(
        Fixture::load(CLEAN).param_findings(&[TEST_PREFIX]),
        Vec::<String>::new(),
        "a tree whose path and query parameters are declared with the right `required` \
         flag must not be reported"
    );
}

#[test]
fn the_clean_fixture_exercises_a_path_and_a_query_parameter() {
    // Without both shapes on the baseline, a rule that compared nothing at all
    // would pass the silence above.
    let data = Fixture::load(CLEAN).read("get/data.rs");

    assert!(
        data.contains("#[get(\"/get/metadata/<asset_id>\")]") && data.contains("asset_id: &str"),
        "the clean fixture binds a path segment to an argument of the same name"
    );
    assert!(
        data.contains("#[get(\"/get/path-completion?<path>\")]")
            && data.contains("path: Option<String>"),
        "the clean fixture binds an optional query parameter"
    );
}

#[test]
fn a_declared_body_shape_is_reported_nothing() {
    // The baseline for P3 and P5: a named schema, a form body satisfied by its
    // media type, a handler binding arbitrary JSON against an untyped schema, and a
    // route with no body. Every wrapper P3 unwraps and every `required` flag P1/P2
    // read is exercised here in the conforming direction, so a rule that misreads
    // one of them fails on the baseline rather than only on drift.
    let tree = four_handler_tree("params-conforming", DECLARING_HANDLERS, DECLARING_DOCUMENT);

    assert_eq!(tree.param_findings(&[]), Vec::<String>::new());
}

#[test]
fn the_conforming_tree_reports_nothing_from_any_of_the_five_rules() {
    // The same silence with the rules' own claims stated: no path or query
    // finding, no body finding, no operation-id finding and no schema finding, so
    // a rule that reported for the wrong reason is not hiding behind a rule that
    // reported for the right one.
    let tree = four_handler_tree(
        "params-conforming-claims",
        DECLARING_HANDLERS,
        DECLARING_DOCUMENT,
    );

    for (rule, expected) in [
        ("path", 0),
        ("query", 0),
        ("body", 0),
        ("operationId", 0),
        ("schema", 0),
    ] {
        let count = tree
            .param_findings(&[])
            .iter()
            .filter(|finding| mentions(finding, rule))
            .count();
        assert_eq!(count, expected, "the {rule} rules are quiet: {count}");
    }
}

// ── P1: path parameters ───────────────────────────────────────────────────────

#[test]
fn reports_a_path_segment_the_operation_does_not_declare() {
    // The state ten of the repository's operations are in: a segment the route
    // binds, a document that names the operation and none of its parameters, and
    // nothing else wrong.
    let tree = one_handler_tree(
        "params-undocumented-path",
        ROUTE_WITH_PATH,
        DOCUMENT_WITHOUT_PATH_PARAMETER,
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![format!(
            "{}:3: write::get_album: the route binds path parameter `album_id` but the \
             operation declares no `in: path` parameter by that name",
            tree.label("write/write.rs")
        )]
    );
}

#[test]
fn reports_a_declared_path_parameter_the_route_does_not_bind() {
    // The route has no segment, and the document — keyed by the same path the
    // annotation declares — carries the parameter anyway: a generator would ask
    // the caller for `album_id` on a route with nowhere to put it.
    let tree = one_handler_tree(
        "params-extra-path",
        ROUTE_WITHOUT_PATH,
        &DOCUMENT_WITH_PATH_PARAMETER
            .replace(r#""/write/albums/{album_id}""#, r#""/write/albums""#),
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![format!(
            "{}:3: write::get_album: the operation declares path parameter `album_id` but \
             the route binds no such segment",
            tree.label("write/write.rs")
        )]
    );
}

#[test]
fn reports_a_path_parameter_the_operation_declares_optional() {
    // `OpenAPI` requires `required: true` on a path parameter whatever the handler
    // does with it, so a generator reading the flag would offer a request that
    // does not address the operation.
    let tree = one_handler_tree(
        "params-optional-path",
        ROUTE_WITH_PATH,
        &DOCUMENT_WITH_PATH_PARAMETER.replace(r#""required": true"#, r#""required": false"#),
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![format!(
            "{}:3: write::get_album: path parameter `album_id` is bound by the route and \
             cannot be optional, but the operation declares `required: false`",
            tree.label("write/write.rs")
        )]
    );
}

#[test]
fn a_path_parameter_with_its_required_flag_is_not_reported() {
    // The conforming direction of the rule above, so it is a disagreement with the
    // flag and not "a path parameter is always reported".
    let tree = one_handler_tree(
        "params-required-path",
        ROUTE_WITH_PATH,
        DOCUMENT_WITH_PATH_PARAMETER,
    );

    assert_eq!(tree.param_findings(&[]), Vec::<String>::new());
}

#[test]
fn a_path_parameter_and_its_segment_named_differently_are_both_reported() {
    // The two spellings disagree, so neither side matches the other: the route
    // binds `album_id` and the document declares `id`. One finding per odd name
    // out, because a reader has to look at both to know which is wrong.
    let tree = one_handler_tree(
        "params-renamed-path",
        ROUTE_WITH_PATH,
        &DOCUMENT_WITH_PATH_PARAMETER.replace(r#""name": "album_id""#, r#""name": "id""#),
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![
            format!(
                "{}:3: write::get_album: the operation declares path parameter `id` but the \
                 route binds no such segment",
                tree.label("write/write.rs")
            ),
            format!(
                "{}:3: write::get_album: the route binds path parameter `album_id` but the \
                 operation declares no `in: path` parameter by that name",
                tree.label("write/write.rs")
            ),
        ],
        "the two names are reported one each, sorted, and neither is reported twice"
    );
}

#[test]
fn an_unterminated_placeholder_is_not_read_as_a_name() {
    // A template with a `{` and no `}` declares no parameter, and the reader does
    // not invent one from the remainder — otherwise a hand-edited document would
    // report a parameter nobody wrote down. Asserted at the reader itself: a
    // document path that differs from the annotation's never reaches P1, because
    // the lookup that feeds it is keyed by the annotation and the disagreement
    // belongs to `check_contract`.
    assert_eq!(
        spec_placeholders("/write/albums/{album_id"),
        Vec::<String>::new(),
        "the remainder after an unterminated `{{` is not a declaration"
    );
    assert_eq!(
        spec_placeholders("/write/albums/{album_id}/cover/{cover"),
        vec!["album_id".to_string()],
        "placeholders read before the unterminated one are kept"
    );
}

// ── P2: query parameters ──────────────────────────────────────────────────────

#[test]
fn reports_a_query_parameter_the_operation_does_not_declare() {
    let tree = one_handler_tree(
        "params-undocumented-query",
        ROUTE_WITH_OPTIONAL_QUERY,
        DOCUMENT_WITHOUT_QUERY_PARAMETER,
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![format!(
            "{}:3: write::get_album: the route binds query parameter `since` but the \
             operation declares no `in: query` parameter by that name",
            tree.label("write/write.rs")
        )]
    );
}

#[test]
fn reports_a_declared_query_parameter_the_route_does_not_bind() {
    // The two names disagree, so neither side matches the other — the same shape
    // as the path rule above: one finding per odd name out.
    let tree = one_handler_tree(
        "params-extra-query",
        ROUTE_WITH_OPTIONAL_QUERY,
        &DOCUMENT_WITH_OPTIONAL_QUERY.replace(r#""name": "since""#, r#""name": "until""#),
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![
            format!(
                "{}:3: write::get_album: the operation declares query parameter `until` but \
                 the route binds no such query segment",
                tree.label("write/write.rs")
            ),
            format!(
                "{}:3: write::get_album: the route binds query parameter `since` but the \
                 operation declares no `in: query` parameter by that name",
                tree.label("write/write.rs")
            ),
        ],
        "the two names are reported one each, sorted, and neither is reported twice"
    );
}

#[test]
fn reports_an_optional_query_parameter_declared_required() {
    // The direction that breaks a caller: the document says the parameter is
    // mandatory, so a generated client refuses to make the request without it,
    // while Rocket would have accepted one.
    let tree = one_handler_tree(
        "params-required-optional-query",
        ROUTE_WITH_OPTIONAL_QUERY,
        &DOCUMENT_WITH_OPTIONAL_QUERY.replace(r#""required": false"#, r#""required": true"#),
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![format!(
            "{}:3: write::get_album: the operation declares query parameter `since` as \
             `required: true` but the handler binds it as an `Option`",
            tree.label("write/write.rs")
        )]
    );
}

#[test]
fn reports_a_required_query_parameter_declared_optional() {
    // The other direction: the document offers a request the route answers `422`
    // for, because Rocket refuses a missing `?<name>` the handler does not make
    // optional.
    let tree = one_handler_tree(
        "params-optional-required-query",
        ROUTE_WITH_REQUIRED_QUERY,
        DOCUMENT_WITH_OPTIONAL_QUERY,
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![format!(
            "{}:3: write::get_album: the operation declares query parameter `since` as \
             `required: false` but the handler binds it as a required argument",
            tree.label("write/write.rs")
        )]
    );
}

#[test]
fn a_query_parameter_whose_required_flag_matches_its_argument_is_not_reported() {
    // Both flags in the conforming direction: an `Option` argument against
    // `required: false` and a bare argument against `required: true`.
    for (name, handler, flag) in [
        (
            "params-optional-query-optional",
            ROUTE_WITH_OPTIONAL_QUERY,
            "false",
        ),
        (
            "params-required-query-required",
            ROUTE_WITH_REQUIRED_QUERY,
            "true",
        ),
    ] {
        let tree = one_handler_tree(
            name,
            handler,
            &DOCUMENT_WITH_OPTIONAL_QUERY
                .replace(r#""required": false"#, &format!(r#""required": {flag}"#)),
        );

        assert_eq!(tree.param_findings(&[]), Vec::<String>::new());
    }
}

#[test]
fn a_query_parameter_the_handler_does_not_bind_is_not_reported_twice() {
    // The `?<since>` the route declares with no matching argument: there is no
    // `Option<T>` to compare, so the `required` half has nothing to say. The
    // undocumented-parameter finding is the whole of it — a rule that guessed
    // would report a second line here.
    let tree = one_handler_tree(
        "params-query-without-argument",
        r#"#[utoipa::path(get, path = "/write/albums", tag = "albums")]
#[get("/write/albums?<since>")]
pub fn get_album(_auth: GuardAuth) {}
"#,
        DOCUMENT_WITHOUT_QUERY_PARAMETER,
    );

    assert_eq!(tree.param_findings(&[]).len(), 1);
}

#[test]
fn a_guard_bound_to_a_query_parameters_name_is_not_its_optionality() {
    // A guard runs before the body, so an `Option` wrapper around one says whether
    // the *authentication* is deferred, not whether the query parameter may be
    // omitted. Reading it as the latter would answer a policy question with a
    // contract one: the document declares `since` required, the handler's `since`
    // is a guard the body propagates with `?`, and no optionality claim is made
    // from either. Reported only if a guard ever starts being read as the query
    // parameter's argument.
    let tree = one_handler_tree(
        "params-guard-shadows-query",
        r#"#[utoipa::path(get, path = "/write/albums", tag = "albums")]
#[get("/write/albums?<since>")]
pub fn get_album(since: Option<GuardAuth>) {
    since?;
}
"#,
        &DOCUMENT_WITH_OPTIONAL_QUERY.replace(r#""required": false"#, r#""required": true"#),
    );

    assert_eq!(
        tree.param_findings(&[]),
        Vec::<String>::new(),
        "the guard is not the query parameter's argument, so the `required` flag has \
         nothing to compare against"
    );
}

#[test]
fn a_query_parameter_bound_through_a_struct_is_not_checked() {
    // The other signature this rule cannot read, and the one that is silent for a
    // different reason than the guard above: `?<since>` reaches the handler as a
    // field of `AlbumFilter`, so the parameter is filled by an argument named
    // `filter`. Reading the flag would mean reading the struct's definition to find
    // whether that field is an `Option`, which the source scan does not do — so the
    // rule says nothing rather than reading `required: true` as a claim about a
    // `filter` it has not resolved. The shape is held out of the repository by
    // `every_query_parameter_the_repository_binds_reaches_a_plain_argument`.
    let tree = one_handler_tree(
        "params-struct-bound-query",
        r#"#[utoipa::path(get, path = "/write/albums", tag = "albums")]
#[get("/write/albums?<since>")]
pub fn get_album(filter: AlbumFilter) {}
"#,
        &DOCUMENT_WITH_OPTIONAL_QUERY.replace(r#""required": false"#, r#""required": true"#),
    );

    assert_eq!(
        tree.param_findings(&[]),
        Vec::<String>::new(),
        "the `required` flag is a claim about an `Option<T>` on the bound argument, and \
         `AlbumFilter`'s fields are not something this analyzer reads"
    );
}

// ── P3: the request body ──────────────────────────────────────────────────────

#[test]
fn reports_a_body_the_operation_does_not_declare() {
    // The route reads a body and the document says nothing, so the operation is
    // documented as taking no payload at all. Dropping the body also orphans the
    // schema it referenced, which is P5's finding on the same edit and stays in
    // the report below.
    let tree = four_handler_tree(
        "params-undeclared-body",
        DECLARING_HANDLERS,
        &DECLARING_DOCUMENT.replace(
            r##"        "requestBody": {
          "required": true,
          "content": {
            "application/json": { "schema": { "$ref": "#/components/schemas/CreateAlbum" } }
          }
        },
"##,
            "",
        ),
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![
            format!(
                "{}: component schema `CreateAlbum` is defined but nothing references it",
                tree.label("openapi.json")
            ),
            format!(
                "{}:5: write::create_album: the route binds its body to `album` but the \
                 operation declares no request body",
                tree.label("write/write.rs")
            ),
        ]
    );
}

#[test]
fn reports_a_declared_body_the_route_does_not_bind() {
    // The other direction: a payload the reference describes and the route never
    // reads, so every request the documentation suggests is answered `422`. The
    // document gains the body — P3 compares the handler against the document, not
    // against the annotation — and the schema it names stays referenced by it.
    let tree = four_handler_tree(
        "params-unbound-body",
        DECLARING_HANDLERS,
        &DECLARING_DOCUMENT.replace(
            r#""responses": { "200": { "description": "Album" } }"#,
            r##""requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CreateAlbum" } } } },
        "responses": { "200": { "description": "Album" } }"##,
        ),
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![format!(
            "{}:17: write::get_album: the operation declares a request body but the route \
             binds no `data = \"<…>\"` argument",
            tree.label("write/write.rs")
        )]
    );
}

#[test]
fn reports_a_value_body_on_a_typed_handler() {
    // `request_body = Value` on a handler that binds `Json<CreateAlbum>`: the
    // document tells a generator the body is arbitrary JSON, so nothing about the
    // payload the handler actually reads reaches a caller. The state
    // `POST /get/prefetch` is in.
    let tree = four_handler_tree(
        "params-value-body",
        DECLARING_HANDLERS,
        &DECLARING_DOCUMENT.replace(
            r##""$ref": "#/components/schemas/CreateAlbum""##,
            r#""schema": {}"#,
        ),
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![
            format!(
                "{}: component schema `CreateAlbum` is defined but nothing references it",
                tree.label("openapi.json")
            ),
            format!(
                "{}:5: write::create_album: the operation declares request body `Value` but \
                 the route binds its body to `CreateAlbum`",
                tree.label("write/write.rs")
            ),
        ],
        "the untyped body and the schema its `$ref` used to reach, which P5 reports \
         on the same edit"
    );
}

#[test]
fn reports_a_named_body_on_a_value_handler() {
    // The mirror image, and the other direction the plan means by "`Value` bodies
    // on typed handlers": a document that names a schema for a handler that accepts
    // any JSON describes a constraint the route does not enforce.
    let tree = four_handler_tree(
        "params-named-body-on-value",
        DECLARING_HANDLERS,
        &DECLARING_DOCUMENT
            .replace(
                r#""application/json": { "schema": {} }"#,
                r##""application/json": { "schema": { "$ref": "#/components/schemas/Archive" } }"##,
            )
            .replace(
                r#""CreateAlbum": { "type": "object" }"#,
                r#""CreateAlbum": { "type": "object" },
                  "Archive": { "type": "object" }"#,
            ),
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![format!(
            "{}:13: write::import_archive: the operation declares request body `Archive` but \
             the route binds its body to `Value`",
            tree.label("write/write.rs")
        )]
    );
}

#[test]
fn reports_a_multipart_body_the_document_never_calls_multipart() {
    // The shape both upload handlers are in today: `request_body = Value`, which
    // utoipa renders as an untyped `application/json` body. A caller reading the
    // reference sends JSON; Rocket reads a form.
    let tree = four_handler_tree(
        "params-multipart-declared-as-json",
        DECLARING_HANDLERS,
        &DECLARING_DOCUMENT.replace(
            r#""multipart/form-data": { "schema": {} }"#,
            r#""application/json": { "schema": {} }"#,
        ),
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![format!(
            "{}:9: write::upload: the operation declares request body `Value` but the route \
             binds a `Form` body to `UploadForm` — name the schema `UploadForm` or declare \
             the body `multipart/form-data`",
            tree.label("write/write.rs")
        )]
    );
}

#[test]
fn a_form_body_declared_multipart_and_named_is_not_reported() {
    // The conforming form: the media type the rule reads, with the schema naming
    // the inner type beside it. The baseline tree carries the media type with an
    // untyped schema; this adds the name on top, and neither half reports.
    let tree = four_handler_tree(
        "params-multipart-named-schema",
        DECLARING_HANDLERS,
        &DECLARING_DOCUMENT
            .replace(
                r#""multipart/form-data": { "schema": {} }"#,
                r##""multipart/form-data": { "schema": { "$ref": "#/components/schemas/UploadForm" } }"##,
            )
            .replace(
                r#""CreateAlbum": { "type": "object" }"#,
                r#""CreateAlbum": { "type": "object" },
                  "UploadForm": { "type": "object" }"#,
            ),
    );

    assert_eq!(tree.param_findings(&[]), Vec::<String>::new());
}

#[test]
fn a_form_body_the_document_describes_as_json_is_still_reported() {
    // The `Form` body the rule above accepts with its media type, with the media
    // type moved to JSON while the schema still names the inner type. Naming the
    // type does not tell a caller how to send the body, so the finding names the
    // media type the document did declare rather than the schema.
    let tree = four_handler_tree(
        "params-form-schema-named-but-not-multipart",
        DECLARING_HANDLERS,
        &DECLARING_DOCUMENT
            .replace(
                r#""multipart/form-data": { "schema": {} }"#,
                r##""application/json": { "schema": { "$ref": "#/components/schemas/UploadForm" } }"##,
            )
            .replace(
                r#""CreateAlbum": { "type": "object" }"#,
                r#""CreateAlbum": { "type": "object" },
                  "UploadForm": { "type": "object" }"#,
            ),
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![format!(
            "{}:9: write::upload: the operation describes the `Form` body as \
             `application/json` but the route binds it as `UploadForm` — declare the body \
             `multipart/form-data`",
            tree.label("write/write.rs")
        )],
        "naming the inner type does not tell a caller how to send the body; the media \
         type is the half that is missing"
    );
}

#[test]
fn a_primitive_body_is_matched_by_the_rust_spelling_of_its_type() {
    // `request_body = String` reaches the document as `{"type": "string"}`, which is
    // a JSON Schema keyword rather than a component name. Comparing the two
    // verbatim would report every primitive body in the API as a mismatch, so the
    // rule maps the primitive — and this is the test that says so. The shape is
    // `POST /post/authenticate`'s.
    let tree = write_tree(
        "params-primitive-body",
        &[
            ("write/mod.rs", ROUTE_TABLE),
            (
                "write/write.rs",
                r#"use rocket::post;

#[utoipa::path(post, path = "/write/authenticate", tag = "auth", request_body = String)]
#[post("/write/authenticate", data = "<password>")]
pub fn create_album(password: Json<String>) {}
"#,
            ),
        ],
        PRIMITIVE_BODY_DOCUMENT,
    );

    assert_eq!(tree.param_findings(&[]), Vec::<String>::new());
}

#[test]
fn a_json_body_the_analyzer_cannot_name_is_not_compared_against_the_document() {
    // The types the body reader unwraps to nothing: a tuple inside `Json`, a slice,
    // and a reference to a slice. A `&` in front of a named type is unwrapped, so
    // the last row is the control — a reference is not by itself a reason to skip.
    //
    // There is no name to compare, so the rule says nothing. The string it stood in
    // for the missing name could only ever mismatch, and a finding carrying it names
    // neither the type the handler takes nor a change that would answer it.
    for (shape, body_type) in [
        ("a tuple", "Json<(u64, String)>"),
        ("a slice", "Json<&[u8]>"),
        ("a reference to a slice", "&[u8]"),
        ("a reference to a named type", "&Json<CreateAlbum>"),
    ] {
        let tree = write_tree(
            &format!(
                "params-unreadable-body-{}",
                body_type.replace(['<', '>', ' ', '&'], "")
            ),
            &[
                ("write/mod.rs", ROUTE_TABLE),
                (
                    "write/write.rs",
                    &format!(
                        r#"use rocket::post;

#[utoipa::path(post, path = "/write/create", tag = "albums", request_body = CreateAlbum)]
#[post("/write/create", format = "json", data = "<album>")]
pub fn create_album(_auth: GuardAuth, album: {body_type}) {{}}
"#
                    ),
                ),
            ],
            DECLARING_DOCUMENT,
        );

        assert_eq!(
            tree.param_findings(&[]),
            Vec::<String>::new(),
            "`{body_type}` is {shape}, and a body the analyzer cannot name is not \
             compared against the type the document declares"
        );
    }
}

#[test]
fn a_form_body_the_analyzer_cannot_name_is_not_compared_at_all() {
    // The same skip on the `Form` branch, with the media type wrong in the second
    // row so the branch that would report it is reached rather than short-circuited
    // on a media type that already agrees. The wrapper is read whatever the type
    // inside it is, so the media type is readable without the type — and is skipped
    // anyway, because every finding this rule makes about a form body names the
    // inner type. Reporting the media type alone would need a second message shape
    // for a type the analyzer cannot name, and would report the two unnamed bodies
    // differently for the sake of the wording. Asserted here so the choice is
    // visible rather than implied by the silence.
    for (declared_as, document) in [
        ("multipart", DECLARING_DOCUMENT),
        (
            "json",
            &DECLARING_DOCUMENT.replace(
                r#""multipart/form-data": { "schema": {} }"#,
                r#""application/json": { "schema": {} }"#,
            ),
        ),
    ] {
        let tree = four_handler_tree(
            &format!("params-unreadable-form-{declared_as}"),
            &DECLARING_HANDLERS.replace(
                "Result<Form<UploadForm<'_>>, Errors<'_>>",
                "Result<Form<(String, String)>, Errors<'_>>",
            ),
            document,
        );

        assert_eq!(
            tree.param_findings(&[]),
            Vec::<String>::new(),
            "the form body is declared {declared_as} and the analyzer cannot read the \
             type it carries"
        );
    }
}

// ── P4: operation ids ─────────────────────────────────────────────────────────

#[test]
fn reports_an_operation_id_that_is_not_the_handler_name() {
    let tree = one_handler_tree(
        "params-foreign-operation-id",
        ROUTE_WITHOUT_PATH,
        &DOCUMENT_WITHOUT_QUERY_PARAMETER.replace(r#""get_album""#, r#""album""#),
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![format!(
            "{}:3: write::get_album: the operation declares operationId `album` but the \
             handler is named `get_album`",
            tree.label("write/write.rs")
        )]
    );
}

#[test]
fn reports_an_operation_with_no_operation_id() {
    let tree = one_handler_tree(
        "params-missing-operation-id",
        ROUTE_WITHOUT_PATH,
        &DOCUMENT_WITHOUT_QUERY_PARAMETER.replace(r#""operationId": "get_album","#, ""),
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![format!(
            "{}:3: write::get_album: the operation declares no operationId, so it is \
             documented under no name a generated client could call",
            tree.label("write/write.rs")
        )]
    );
}

// ── P5: component schemas ─────────────────────────────────────────────────────

#[test]
fn reports_a_reference_to_a_schema_the_document_does_not_define() {
    let tree = four_handler_tree(
        "params-dangling-ref",
        DECLARING_HANDLERS,
        &DECLARING_DOCUMENT.replace(
            r##""$ref": "#/components/schemas/CreateAlbum""##,
            r##""$ref": "#/components/schemas/RemovedAlbum""##,
        ),
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![
            format!(
                "{}: `$ref` to the component schema `RemovedAlbum`, which the document does \
                 not define",
                tree.label("openapi.json")
            ),
            format!(
                "{}: component schema `CreateAlbum` is defined but nothing references it",
                tree.label("openapi.json")
            ),
            format!(
                "{}:5: write::create_album: the operation declares request body \
                 `RemovedAlbum` but the route binds its body to `CreateAlbum`",
                tree.label("write/write.rs")
            ),
        ],
        "the dangling reference, the schema its `$ref` used to keep alive, and the \
         body the now-named schema disagrees with — all three follow from one edit"
    );
}

#[test]
fn reports_a_schema_nothing_references() {
    // The shape `FileEntry` is in: registered as a component, named by no operation,
    // still occupying the document and the generated reference.
    let tree = four_handler_tree(
        "params-orphan-schema",
        DECLARING_HANDLERS,
        &DECLARING_DOCUMENT.replace(
            r#""CreateAlbum": { "type": "object" }"#,
            r#""CreateAlbum": { "type": "object" },
              "FileEntry": { "type": "object" }"#,
        ),
    );

    assert_eq!(
        tree.param_findings(&[]),
        vec![format!(
            "{}: component schema `FileEntry` is defined but nothing references it",
            tree.label("openapi.json")
        )]
    );
}

#[test]
fn a_schema_referenced_only_by_another_schema_is_not_orphaned() {
    // The definition is reached through one hop, which is still reached: the rule is
    // about the document, not about what each operation names directly.
    let tree = four_handler_tree(
        "params-indirectly-referenced-schema",
        DECLARING_HANDLERS,
        &DECLARING_DOCUMENT.replace(
            r#""CreateAlbum": { "type": "object" }"#,
            r##""CreateAlbum": { "type": "object", "properties": { "cover": { "$ref": "#/components/schemas/FileEntry" } } },
              "FileEntry": { "type": "object" }"##,
        ),
    );

    assert_eq!(tree.param_findings(&[]), Vec::<String>::new());
}

#[test]
fn a_reference_to_another_component_kind_is_not_a_schema_reference() {
    // `#/components/responses/...` is a different kind of component with its own name
    // space, and comparing the two sets would report every reused response as a
    // dangling schema.
    let document: serde_json::Value = serde_json::from_str(
        r##"{
            "paths": {
                "/a": { "get": { "operationId": "a", "responses": { "401": { "$ref": "#/components/responses/Unauthorized" } } } }
            },
            "components": { "responses": { "Unauthorized": { "description": "nope" } } }
        }"##,
    )
    .expect("the document is valid JSON");
    let index = schema_index(&document);

    assert_eq!(index, SchemaIndex::default());
    assert_eq!(
        check_params(&[], "spec", &document, &[]),
        Vec::<openapi_sanity::Finding>::new(),
        "a document with no component schemas has nothing for P5 to report"
    );
}

#[test]
fn a_document_without_components_declares_no_schemas() {
    let document = serde_json::json!({ "paths": {} });
    assert_eq!(schema_index(&document), SchemaIndex::default());
}

// ── Reading the document ──────────────────────────────────────────────────────

#[test]
fn an_unnamed_or_unlocated_parameter_is_kept_and_read_as_other() {
    // `in: header` is a real location, and `in: <something else>` is a location this
    // crate does not know. Both are read as `Other` rather than dropped, so they
    // cannot shrink the compared set without saying so.
    let document: serde_json::Value = serde_json::from_str(
        r#"{
            "paths": {
                "/a/{id}": {
                    "get": {
                        "operationId": "a",
                        "parameters": [
                            { "in": "path", "name": "id", "required": true },
                            { "in": "header", "name": "X-Picasu", "required": true },
                            { "in": "nowhere", "name": "odd" },
                            { "in": "query" }
                        ]
                    }
                }
            }
        }"#,
    )
    .expect("the document is valid JSON");
    let operations = spec_operations(&document);

    assert_eq!(
        operations[0]
            .parameters
            .iter()
            .map(|parameter| (parameter.name, parameter.location, parameter.required))
            .collect::<Vec<_>>(),
        vec![
            ("id", ParameterLocation::Path, true),
            ("X-Picasu", ParameterLocation::Other, true),
            ("odd", ParameterLocation::Other, false),
        ],
        "an entry with no name is left out and an entry with no `in` is read as \
         `Other`, so only path and query are compared with the route"
    );
}

/// One path's `requestBody` as the document states it: the schema and its media
/// types, or `None` when the operation declares no body at all.
type DeclaredBody<'a> = (&'a str, Option<(BodySchema<'a>, Vec<&'a str>)>);

#[test]
fn an_untyped_schema_and_a_primitive_are_distinguishable_from_a_named_one() {
    // The three shapes `requestBody` carries, since P3 compares each against a
    // different claim: a component name, a JSON Schema primitive, and nothing at all.
    let document: serde_json::Value = serde_json::from_str(
        r##"{
            "paths": {
                "/named": { "post": { "requestBody": { "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Named" } } } } } },
                "/primitive": { "post": { "requestBody": { "content": { "text/plain": { "schema": { "type": "string" } } } } } },
                "/untyped": { "post": { "requestBody": { "content": { "application/json": { "schema": {} } } } } },
                "/empty": { "post": { "requestBody": {} } },
                "/none": { "post": {} }
            }
        }"##,
    )
    .expect("the document is valid JSON");
    let operations = spec_operations(&document);
    let bodies: Vec<DeclaredBody<'_>> = operations
        .iter()
        .map(|operation| {
            let body = operation.request_body.as_ref().map(|body| {
                let schema = body.schema;
                let content_types = body.content_types.clone();
                (schema, content_types)
            });
            (operation.path, body)
        })
        .collect();

    assert_eq!(
        bodies,
        vec![
            ("/empty", Some((BodySchema::Untyped, Vec::new()))),
            (
                "/named",
                Some((BodySchema::Named("Named"), vec!["application/json"]))
            ),
            ("/none", None),
            (
                "/primitive",
                Some((BodySchema::Primitive("string"), vec!["text/plain"]))
            ),
            (
                "/untyped",
                Some((BodySchema::Untyped, vec!["application/json"]))
            ),
        ],
        "a `$ref` names a component, a `type` names a JSON Schema primitive, neither \
         is utoipa's rendering of `request_body = Value`, and no `requestBody` at \
         all is not a body — in the map's key order, which is how `spec_operations` \
         walks the paths"
    );
}

// ── What the rules do not claim ───────────────────────────────────────────────

#[test]
fn a_handler_whose_annotation_disagrees_with_its_route_is_left_to_the_contract_check() {
    // The document inherits the renamed path from the annotation, so its
    // placeholders describe a route the handler does not serve. P1 reporting that
    // again would restate `check_contract`'s local finding in a second vocabulary,
    // and P4 would compare an id against a document describing the rename.
    let tree = one_handler_tree(
        "params-annotation-disagrees",
        ROUTE_WITH_RENAMED_ANNOTATION,
        RENAMED_DOCUMENT,
    );

    assert_eq!(
        tree.param_findings(&[]),
        Vec::<String>::new(),
        "the disagreement is one finding of its own, and it belongs to the path and \
         method rules"
    );
    assert_eq!(
        tree.findings(&[]).len(),
        1,
        "`check_contract` is the rule that names it"
    );
}

#[test]
fn an_operation_the_document_omits_is_left_to_the_contract_check() {
    // No operation in the document means no declared parameters to compare, and the
    // omission is already one finding. P1 reporting the same handler would make an
    // absent operation produce a finding per parameter it would have had.
    let tree = one_handler_tree(
        "params-undocumented-operation",
        ROUTE_WITH_OPTIONAL_QUERY,
        r#"{
  "openapi": "3.1.0",
  "paths": {}
}
"#,
    );

    assert_eq!(tree.param_findings(&[]), Vec::<String>::new());
}

#[test]
fn a_handler_registered_twice_is_still_checked_once() {
    let tree = write_tree(
        "params-registered-twice",
        &[
            (
                "write/mod.rs",
                r"pub mod write;

pub fn generate_write_routes() -> Vec<Route> {
    routes![
        write::get_album,
        write::get_album,
    ]
}
",
            ),
            ("write/write.rs", ROUTE_WITH_OPTIONAL_QUERY),
        ],
        DOCUMENT_WITHOUT_QUERY_PARAMETER,
    );

    assert_eq!(
        tree.param_findings(&[]).len(),
        1,
        "the duplicate registration is `check_contract`'s finding; the parameter \
         finding is about the handler, which declares its contract once"
    );
}

#[test]
fn the_excluded_prefix_is_not_asked_to_document_its_parameters() {
    // The fixture's probe binds a path segment and sits under the prefix the public
    // artifact strips, so it is not asked to redeclare it. Widening the compared
    // contract to the test-only surface is the artifact owner's decision.
    let fixture = Fixture::load(CLEAN);
    assert!(fixture.read("get/probe.rs").contains("<asset_id>"));

    assert!(
        !fixture
            .param_findings(&[TEST_PREFIX])
            .iter()
            .any(|finding| finding.contains("/get/test/")),
        "an excluded operation is outside the compared contract"
    );
}

#[test]
fn the_report_does_not_depend_on_the_order_the_files_were_read_in() {
    // Ordering is what makes a gate's output reviewable in a diff, so the unit set
    // is deliberately reversed here: the same two files read in either order must
    // produce the same report.
    let tree = one_handler_tree(
        "params-order",
        ROUTE_WITH_PATH,
        DOCUMENT_WITHOUT_PATH_PARAMETER,
    );
    let forward = tree.param_findings(&[]);

    let mut units = tree.units();
    units.reverse();
    let reversed = param_findings_against(Path::new(&tree.label("openapi.json")), &units, &[]);

    assert_eq!(reversed, forward);
}

// ── The repository's own document ─────────────────────────────────────────────

/// The repository's own document, as the CLI will see it once `check_params` is
/// wired in: every registered handler's path, query and body declarations agree
/// with `backend/openapi.json`, and every component schema is referenced. The
/// counts Step 4 recorded on the repository (10 undocumented path parameters, 11
/// undocumented query parameters, three `Value`/`Form` bodies, one orphaned
/// schema) were fixed in the change that produced this document; what is asserted
/// now is that they stay fixed, with the rendered findings as the failure
/// message. The rules' sensitivity to each of those failure classes is pinned by
/// the fixture tests above — this test pins the repository staying conformant.
#[test]
fn the_repository_declares_every_parameter_and_references_every_schema() {
    let repository = repository_root();
    let artifact = repository.join("backend").join("openapi.json");
    let router = Fixture::in_directory(&repository.join("backend").join("src").join("router"));
    let reported = param_findings_against(&artifact, &router.units(), &[REPOSITORY_PREFIX]);

    assert!(
        reported.is_empty(),
        "the committed document drifted from the route source:\n{reported:#?}"
    );
}

#[test]
fn every_query_parameter_the_repository_binds_reaches_a_plain_argument() {
    // The `required` half of P2 reads one thing: whether the argument Rocket binds
    // to a `?<name>` is an `Option`. No route in the repository reaches that
    // parameter any other way — a guard shares no query parameter's name, and no
    // `?<x>` is filled from a `FromForm` struct's field — so the rule has no
    // parameter it currently cannot read. Asserted rather than assumed: a route
    // added in either unreadable shape makes this fail and name the handler, instead
    // of the rule quietly checking nothing about it.
    let repository = repository_root();
    let router = Fixture::in_directory(&repository.join("backend").join("src").join("router"));
    let mut unreadable = Vec::new();

    for unit in router.units() {
        let scan = scan_source(unit.label(), unit.source(), unit.group_prefix());
        for handler in &scan.handlers {
            let Some(uri) = handler.uri.as_deref() else {
                continue;
            };
            for name in route_query_bindings(uri) {
                let plain = handler
                    .args
                    .iter()
                    .any(|argument| argument.kind == ArgKind::Plain && argument.name == name);
                if !plain {
                    unreadable.push(format!("{}:{} {}", unit.label(), handler.line, name));
                }
            }
        }
    }

    assert!(
        unreadable.is_empty(),
        "P2 checks the `required` flag only where the route binds the name to a plain \
         argument, and these do not reach one:\n{unreadable:#?}"
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

impl Fixture {
    /// Assert that the parameter gate reports exactly `expected`, a complete
    /// rendered diagnostic, so a rule that moves its anchor fails here rather than
    /// still matching on wording.
    #[allow(dead_code)]
    fn assert_param_reports(&self, expected: &str) {
        let reported = self.param_findings(&[]);

        assert!(
            reported.iter().any(|finding| finding == expected),
            "expected\n  {expected}\nfrom\n{reported:#?}"
        );
    }
}

/// Whether a finding is one of the rules named by `rule`.
///
/// A message fragment rather than a rule identifier: the crate reports what is
/// wrong, not which rule found it, and a test that wanted the identifier would
/// have to grep the same message anyway.
fn mentions(finding: &str, rule: &str) -> bool {
    finding.contains(rule)
}

/// A tree of the four-handler shape: one route table, one handler file, one
/// document.
fn four_handler_tree(name: &str, handlers: &str, document: &str) -> Fixture {
    write_tree(
        name,
        &[("write/mod.rs", ROUTE_TABLE), ("write/write.rs", handlers)],
        document,
    )
}

/// A tree of the single-handler shape. Every P1/P2/P4 rule is about one route, and
/// a table registering handlers the tree does not define would put
/// `check_contract`'s "declared in no scanned source file" finding in the middle of
/// their reports.
fn one_handler_tree(name: &str, handler: &str, document: &str) -> Fixture {
    write_tree(
        name,
        &[
            ("write/mod.rs", ONE_ROUTE_TABLE),
            ("write/write.rs", handler),
        ],
        document,
    )
}

/// A route binding a path segment, on the argument of the same name. `get_album` is
/// on line 3.
const ROUTE_WITH_PATH: &str = r#"#[utoipa::path(get, path = "/write/albums/{album_id}", tag = "albums")]
#[get("/write/albums/<album_id>")]
pub fn get_album(_auth: GuardAuth, album_id: u32) {}
"#;

/// The same route with no segment, so a declared path parameter is an extra.
const ROUTE_WITHOUT_PATH: &str = r#"#[utoipa::path(get, path = "/write/albums", tag = "albums")]
#[get("/write/albums")]
pub fn get_album(_auth: GuardAuth) {}
"#;

/// A route binding a query parameter the handler makes optional.
const ROUTE_WITH_OPTIONAL_QUERY: &str = r#"#[utoipa::path(get, path = "/write/albums", tag = "albums")]
#[get("/write/albums?<since>")]
pub fn get_album(_auth: GuardAuth, since: Option<i64>) {}
"#;

/// A route binding a query parameter the handler does *not* make optional.
const ROUTE_WITH_REQUIRED_QUERY: &str = r#"#[utoipa::path(get, path = "/write/albums", tag = "albums")]
#[get("/write/albums?<since>")]
pub fn get_album(_auth: GuardAuth, since: i64) {}
"#;

/// A handler whose annotation renames the operation, which `check_contract` owns.
const ROUTE_WITH_RENAMED_ANNOTATION: &str = r#"#[utoipa::path(get, path = "/write/renamed", tag = "albums")]
#[get("/write/albums")]
pub fn get_album(_auth: GuardAuth) {}
"#;

const PRIMITIVE_BODY_DOCUMENT: &str = r#"{
  "openapi": "3.1.0",
  "paths": {
    "/write/authenticate": {
      "post": {
        "operationId": "create_album",
        "tags": ["auth"],
        "requestBody": {
          "required": true,
          "content": { "text/plain": { "schema": { "type": "string" } } }
        },
        "responses": { "200": { "description": "Token" } }
      }
    }
  }
}
"#;

const DOCUMENT_WITHOUT_PATH_PARAMETER: &str = r#"{
  "openapi": "3.1.0",
  "paths": {
    "/write/albums/{album_id}": {
      "get": {
        "operationId": "get_album",
        "tags": ["albums"],
        "responses": { "200": { "description": "Album" } }
      }
    }
  }
}
"#;

const DOCUMENT_WITH_PATH_PARAMETER: &str = r#"{
  "openapi": "3.1.0",
  "paths": {
    "/write/albums/{album_id}": {
      "get": {
        "operationId": "get_album",
        "tags": ["albums"],
        "parameters": [
          { "in": "path", "name": "album_id", "required": true, "schema": { "type": "integer" } }
        ],
        "responses": { "200": { "description": "Album" } }
      }
    }
  }
}
"#;

const DOCUMENT_WITHOUT_QUERY_PARAMETER: &str = r#"{
  "openapi": "3.1.0",
  "paths": {
    "/write/albums": {
      "get": {
        "operationId": "get_album",
        "tags": ["albums"],
        "responses": { "200": { "description": "Albums" } }
      }
    }
  }
}
"#;

const DOCUMENT_WITH_OPTIONAL_QUERY: &str = r#"{
  "openapi": "3.1.0",
  "paths": {
    "/write/albums": {
      "get": {
        "operationId": "get_album",
        "tags": ["albums"],
        "parameters": [
          { "in": "query", "name": "since", "required": false, "schema": { "type": "integer" } }
        ],
        "responses": { "200": { "description": "Albums" } }
      }
    }
  }
}
"#;

const RENAMED_DOCUMENT: &str = r#"{
  "openapi": "3.1.0",
  "paths": {
    "/write/renamed": {
      "get": {
        "operationId": "get_album",
        "tags": ["albums"],
        "responses": { "200": { "description": "Albums" } }
      }
    }
  }
}
"#;

fn repository_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the crate sits in <repo>/utils/openapi-sanity")
        .to_path_buf()
}
