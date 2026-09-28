//! Consistency between the annotated handlers in source and the operations in
//! the committed `OpenAPI` document.
//!
//! Two of the failures this catches cannot be seen from either end of the
//! existing pipeline. The build script only asks whether a registered handler
//! carries an annotation, and the backend contract tests compare Rocket's
//! *runtime* route table with the spec — but an annotation can sit on a route
//! attribute and still declare a verb the route does not serve. The route is
//! mounted, the spec documents something else, and every other check passes.
//!
//! The path half of that local comparison is gone: every path disagreement
//! converges on a finding elsewhere (recorded at the deletion site in
//! `check_registered`'s body), and dropping it is what left this crate with no
//! Rocket-to-OpenAPI path translation to maintain.
//!
//! The comparison is done on source so it needs no compiled artifact and stays
//! cheap enough to run on every `just openapi-check`. What it cannot answer —
//! whether the route table Rocket mounts really is the one in `routes![]` — is
//! left to the mounted-route parity tests in the backend, which are the only
//! place feature-aware runtime inspection is possible.

use std::collections::{BTreeMap, BTreeSet};

use crate::finding::Finding;
use crate::handlers::{Handler, HttpMethod};
use crate::modules::{SourceUnit, handler_module_path};
use crate::scan_source;

/// One operation declared by an `OpenAPI` document.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SpecOperation<'a> {
    /// Method, from the path item's key.
    pub method: HttpMethod,
    /// Path template, e.g. `/get/metadata/{asset_id}`.
    pub path: &'a str,
    /// The operation's `operationId`, when it declares one.
    pub operation_id: Option<&'a str>,
    /// The response status codes the operation declares, ascending.
    pub responses: Vec<u16>,
    /// Whether the operation carries a non-empty `security` requirement.
    pub secured: bool,
    /// The subject tags the operation declares, in document order. Empty when it
    /// declares none, which [`crate::check_tags`] reports.
    pub tags: Vec<&'a str>,
    /// The parameters the operation declares, in document order. Empty when it
    /// declares none, which is not the same as a route that binds none: a Rocket
    /// URI declares its path and query parameters itself, and every one of them
    /// has to be declared here as well.
    pub parameters: Vec<SpecParameter<'a>>,
    /// The request body the operation declares, or `None` when it declares no
    /// `requestBody` at all. `Some` with an untyped schema is how an annotation
    /// saying `request_body = Value` reaches the document.
    pub request_body: Option<RequestBody<'a>>,
}

/// Where a declared parameter is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParameterLocation {
    /// `in: path`: substituted into the path template. Always required, because
    /// a request that omits a path segment does not address the operation.
    Path,
    /// `in: query`: read from the query string, and optional exactly when the
    /// handler binds it as an `Option`.
    Query,
    /// `in: header`, `in: cookie`, or a location this crate does not know. Read
    /// as one value rather than dropped, so a malformed entry is visible in the
    /// parameter list instead of quietly shrinking it — and outside the
    /// parameter rules, which compare the route's own bindings.
    Other,
}

impl ParameterLocation {
    /// The location an `in = "..."` value names.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "path" => Self::Path,
            "query" => Self::Query,
            _ => Self::Other,
        }
    }
}

/// One entry of an operation's `parameters` array.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SpecParameter<'a> {
    /// The parameter's name, which is also the name Rocket binds its segment or
    /// query key to.
    pub name: &'a str,
    /// Where the value is read from.
    pub location: ParameterLocation,
    /// The declared `required` flag, `false` when the key is absent. An absent
    /// flag on a path parameter is not a valid path parameter: `OpenAPI` requires
    /// `required: true` there, and a generator that reads the flag would tell a
    /// caller the segment is optional.
    pub required: bool,
}

/// The type an operation declares for its request body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BodySchema<'a> {
    /// A `$ref` to a component schema: the document names a type, and the name is
    /// the one the developer would have to write to satisfy it.
    Named(&'a str),
    /// A JSON Schema primitive `type`, which is what utoipa writes for
    /// `request_body = String` and the like. Held as the document spells it,
    /// because a `type` is a JSON Schema keyword rather than a component name.
    Primitive(&'a str),
    /// A schema carrying neither a `$ref` nor a `type`, which is how utoipa
    /// renders `request_body = Value`: the document states nothing about the
    /// shape of the body, and a generator reading it produces `any`.
    Untyped,
}

/// The request body an operation declares.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RequestBody<'a> {
    /// The declared schema. A `requestBody` with no `content` at all is read as
    /// [`BodySchema::Untyped`]: the key is there, and what it describes is not.
    pub schema: BodySchema<'a>,
    /// The media types the body may arrive as, in document order. Read as a set
    /// for the rules, and from the first entry for the schema, which is the one
    /// utoipa emits.
    pub content_types: Vec<&'a str>,
}

/// The component schemas a document defines and the ones its `$ref`s name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaIndex<'a> {
    /// Every name under `components/schemas`.
    pub defined: BTreeSet<&'a str>,
    /// Every name a `$ref` anywhere in the document points at, including refs
    /// inside another component schema. A schema referenced only by another
    /// unreferenced schema is still referenced, which is what makes the rule
    /// about the whole document rather than about each operation.
    pub referenced: BTreeSet<&'a str>,
}

/// The JSON pointer prefix a component schema is addressed by.
const SCHEMA_REF_PREFIX: &str = "#/components/schemas/";

impl SpecOperation<'_> {
    /// Whether the operation states that an unauthorized caller is answered
    /// `401`, either by naming the response or by declaring a security
    /// requirement.
    ///
    /// Both are accepted because they state the same thing at different levels:
    /// a `security` entry says who may call the operation, a `401` response says
    /// what happens when they may not. A document that uses neither leaves a
    /// guarded operation with an undocumented rejection.
    #[must_use]
    pub fn documents_rejection(&self) -> bool {
        self.responses.contains(&401) || self.secured
    }
}

/// The operations of an `OpenAPI` document, sorted by path and then method.
///
/// Derived from `paths` plus the HTTP-method keys of each path item. A path item
/// may also carry non-operation keys (`parameters`, `summary`, `servers`,
/// `$ref`); those are skipped, so a path item that declares an operation under
/// a key this crate does not know is read as a document with fewer operations
/// than it has. An operation without an `operationId` is reported with
/// `operation_id: None` rather than dropped.
///
/// # Panics
/// Panics if `document` is not a JSON object with a `paths` object. A document
/// that does not parse is a broken input, not a document with no operations, so
/// it is rejected loudly instead of read as an empty contract.
#[must_use]
pub fn spec_operations(document: &serde_json::Value) -> Vec<SpecOperation<'_>> {
    let paths = document
        .get("paths")
        .and_then(serde_json::Value::as_object)
        .expect("OpenAPI document has no `paths` object");

    let mut operations: Vec<SpecOperation<'_>> = paths
        .iter()
        .filter_map(|(path, item)| {
            let item = item.as_object()?;
            Some(item.iter().filter_map(|(method, operation)| {
                let method = HttpMethod::from_name(method)?;
                Some(SpecOperation {
                    method,
                    path,
                    operation_id: operation.get("operationId").and_then(|id| id.as_str()),
                    responses: declared_statuses(operation),
                    secured: declares_security(operation),
                    tags: declared_tags(operation),
                    parameters: declared_parameters(operation),
                    request_body: declared_request_body(operation),
                })
            }))
        })
        .flatten()
        .collect();
    operations.sort_unstable();
    operations
}

/// The numeric response status codes an operation declares, ascending.
///
/// A key that is not a number — `default`, or a range such as `4XX` — is not a
/// status code and is left out rather than read as one.
fn declared_statuses(operation: &serde_json::Value) -> Vec<u16> {
    let mut statuses: Vec<u16> = operation
        .get("responses")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|responses| responses.keys())
        .filter_map(|status| status.parse().ok())
        .collect();
    statuses.sort_unstable();
    statuses.dedup();
    statuses
}

/// Whether an operation declares a security requirement, which is where a
/// document states who may call it instead of naming the rejection status.
fn declares_security(operation: &serde_json::Value) -> bool {
    operation
        .get("security")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|requirements| !requirements.is_empty())
}

/// The subject tags an operation declares, in document order.
///
/// An entry that is not a string names no subject, so it is left out rather than
/// guessed at, and an operation whose `tags` is not an array at all is read as
/// carrying none — which [`crate::check_tags`] reports. The document is generated
/// from `tag = "..."` literals, so neither shape is reachable from the generator
/// and reading them leniently costs nothing.
fn declared_tags(operation: &serde_json::Value) -> Vec<&str> {
    operation
        .get("tags")
        .and_then(serde_json::Value::as_array)
        .map(|tags| tags.iter().filter_map(serde_json::Value::as_str).collect())
        .unwrap_or_default()
}

/// The parameters an operation declares, in document order.
///
/// An entry with no `name` names no parameter and is left out; one with no `in`
/// names no location and is read as [`ParameterLocation::Other`] rather than
/// dropped, so a malformed entry shows up in the list instead of quietly
/// shrinking it. Neither shape is reachable from the generator, which writes both
/// keys from `params((..))`, so reading them leniently costs nothing.
fn declared_parameters(operation: &serde_json::Value) -> Vec<SpecParameter<'_>> {
    operation
        .get("parameters")
        .and_then(serde_json::Value::as_array)
        .map(|parameters| {
            parameters
                .iter()
                .filter_map(|parameter| {
                    Some(SpecParameter {
                        name: parameter.get("name")?.as_str()?,
                        location: ParameterLocation::from_name(
                            parameter
                                .get("in")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default(),
                        ),
                        required: parameter
                            .get("required")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The request body an operation declares, or `None` when it declares none.
///
/// A body spread over more than one media type is read through the first, which
/// is the only shape utoipa emits; the media types themselves are all kept,
/// because whether a handler's form body can arrive at all is a question about
/// them rather than about the schema.
fn declared_request_body(operation: &serde_json::Value) -> Option<RequestBody<'_>> {
    let body = operation.get("requestBody")?;
    let content = body.get("content").and_then(serde_json::Value::as_object);
    Some(content.map_or(
        RequestBody {
            schema: BodySchema::Untyped,
            content_types: Vec::new(),
        },
        |content| {
            let schema = content
                .values()
                .next()
                .and_then(|media_type| media_type.get("schema"))
                .map_or(BodySchema::Untyped, declared_body_schema);
            RequestBody {
                schema,
                content_types: content.keys().map(String::as_str).collect(),
            }
        },
    ))
}

/// The type a `requestBody`'s schema declares.
///
/// A `$ref` names a component schema, and its last segment is the name; anything
/// else names a type only if it carries a `type` keyword. A schema with neither
/// says nothing about the body's shape, which is the document's way of saying
/// `Value`.
fn declared_body_schema(schema: &serde_json::Value) -> BodySchema<'_> {
    if let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) {
        return BodySchema::Named(
            reference
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
                .unwrap_or(reference),
        );
    }
    schema
        .get("type")
        .and_then(serde_json::Value::as_str)
        .map_or(BodySchema::Untyped, BodySchema::Primitive)
}

/// The component schemas a document defines, and the ones its `$ref`s name.
///
/// Both directions of the same question, so both are read from one walk of the
/// document: what is under `components/schemas`, and what every `$ref` in the
/// document — in an operation, in a response, inside another schema — points at.
/// A `$ref` to a component kind other than a schema is not collected, because
/// `defined` holds schema names and comparing the two sets is what the rule does.
///
/// No reference to the exclusions a caller passes to the checks: a schema is
/// either named by the document or it is not, whatever the path it is named from.
#[must_use]
pub fn schema_index(document: &serde_json::Value) -> SchemaIndex<'_> {
    let mut index = SchemaIndex::default();
    if let Some(schemas) = document
        .pointer("/components/schemas")
        .and_then(serde_json::Value::as_object)
    {
        index.defined.extend(schemas.keys().map(String::as_str));
    }
    collect_schema_refs(document, &mut index);
    index
}

/// Every component-schema `$ref` anywhere below `value`.
fn collect_schema_refs<'a>(value: &'a serde_json::Value, index: &mut SchemaIndex<'a>) {
    match value {
        serde_json::Value::Object(members) => {
            for (key, member) in members {
                if key == "$ref"
                    && let Some(reference) = member.as_str()
                    && let Some(schema) = reference.strip_prefix(SCHEMA_REF_PREFIX)
                {
                    index.referenced.insert(schema);
                }
                collect_schema_refs(member, index);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_schema_refs(item, index);
            }
        }
        _ => {}
    }
}

/// Every way a set of router source files and an `OpenAPI` document disagree.
///
/// `units` must hold the files the `routes![...]` blocks live in *and* the files
/// defining the handlers they register; [`referenced_handler_files`] says which
/// extra ones a caller has to read to get there. A `routes![...]` entry naming
/// a function no unit declares is reported rather than skipped, because skipping
/// it would shrink the compared contract without saying so.
///
/// `excluded_prefixes` removes operations from the comparison in both
/// directions. The public artifact deliberately omits the test-only probe
/// surface, which source still declares; without the exclusion every such
/// handler is reported as missing from the spec. The list is a parameter rather
/// than a constant so the rule stays with whoever owns the artifact, and a
/// caller that forgets it sees the omission as a finding.
///
/// Findings are sorted by file, line and message, so two runs over the same
/// inputs report the same things in the same order.
#[must_use]
pub fn check_contract(
    units: &[SourceUnit<'_>],
    spec_label: &str,
    spec: &[SpecOperation<'_>],
    excluded_prefixes: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let (declarations, mut registrations) = read_sources(units, &mut findings);
    registrations.sort();

    let documented: BTreeSet<Operation> = spec
        .iter()
        .filter(|operation| !is_excluded(operation.path, excluded_prefixes))
        .map(|operation| (operation.method, operation.path.to_string()))
        .collect();

    let mut declared: BTreeSet<Operation> = BTreeSet::new();
    let mut cursor = 0;
    while let Some(first) = registrations.get(cursor) {
        // The first registration is the handler's contract declaration; every
        // further one mounts a second route for an operation that exists once,
        // and the generated `paths(...)` would list it twice.
        let duplicates: Vec<&Registration> = registrations[cursor + 1..]
            .iter()
            .take_while(|entry| entry.key == first.key)
            .collect();
        for duplicate in &duplicates {
            findings.push(Finding::on_line(
                &duplicate.label,
                duplicate.line,
                format!(
                    "{}: registered in routes![] more than once (first at {}:{})",
                    identity(&first.key),
                    first.label,
                    first.line
                ),
            ));
        }

        check_registered(
            first,
            &declarations,
            &documented,
            excluded_prefixes,
            &mut declared,
            &mut findings,
        );
        cursor += 1 + duplicates.len();
    }

    check_spec_coverage(spec_label, &documented, &declared, &mut findings);
    check_operation_ids(spec_label, spec, &mut findings);

    findings.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.message.cmp(&b.message))
    });
    findings
}

/// The router-relative files defining the handlers a set of route-table files
/// register.
///
/// A `routes![...]` block names handler modules the block's own file does not
/// define, so a caller that read only the scanned module list is missing most of
/// the handlers and every one of them would be reported as undeclared.
#[must_use]
pub fn referenced_handler_files(units: &[SourceUnit<'_>]) -> BTreeSet<String> {
    let mut files = BTreeSet::new();
    for unit in units {
        let scan = scan_source(unit.label(), unit.source(), unit.group_prefix());
        for route in scan.routes {
            files.insert(handler_module_path(unit.group_prefix(), &route.module_path));
        }
    }
    files
}

/// A handler identity: the group a file belongs to, the module a `routes![]`
/// entry qualifies it with, and its function name.
pub(crate) type HandlerKey = (String, String, String);

/// A `(method, path)` pair, the identity an operation is compared by.
type Operation = (HttpMethod, String);

/// A route-declaring function, with the file it was read from.
pub(crate) struct Declaration {
    pub(crate) label: String,
    pub(crate) handler: Handler,
}

/// One `routes![...]` entry, with the file and line it was registered at.
///
/// Ordered by identity and then by where it was registered, so the first entry
/// of a run of equal keys is the handler's declaration and the rest are the
/// duplicates — without depending on the order the files were read in.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Registration {
    pub(crate) key: HandlerKey,
    pub(crate) label: String,
    pub(crate) line: usize,
}

/// The operation a handler's annotation declares for it.
pub(crate) struct DeclaredOperation {
    pub(crate) method: HttpMethod,
    pub(crate) path: String,
    /// Whether the annotation and the route attribute it sits on declare the
    /// same method. A handler that disagrees with itself is already reported, so
    /// a caller comparing it with the document does not restate the cause. Path
    /// agreement is not part of it: the route-attr↔annotation path rule was
    /// dropped as redundant, and every path disagreement now converges on a
    /// finding elsewhere (see the note at that rule's deletion site).
    pub(crate) agrees_with_route: bool,
}

/// The operation a route-declaring function documents.
///
/// The document is generated from the annotation, so the annotation is the
/// source declaration of the operation — including when it disagrees with the
/// route, which is why a disagreeing handler still occupies its operation in
/// the document instead of looking undeclared. The route's URI is not a
/// fallback: every annotation in the repository names its `path`, and the
/// route-path half of `agrees_with_route` went with the dropped path rule, so
/// this crate translates no Rocket URI. `None` when the annotation names no
/// path, or neither the annotation nor the route names a verb.
pub(crate) fn declared_operation(handler: &Handler) -> Option<DeclaredOperation> {
    let annotated_path = handler.spec_path.as_deref()?;
    let same_method = handler.method.is_none() || handler.method == handler.spec_method;
    let method = handler.spec_method.or(handler.method)?;

    Some(DeclaredOperation {
        method,
        path: annotated_path.to_string(),
        agrees_with_route: same_method,
    })
}

/// The route-declaring functions and the `routes![]` entries of every unit, plus
/// the diagnostics their input produced.
///
/// Each file is parsed once for both views, which is what [`scan_source`]
/// exists for, and a syntax error in a file is reported once instead of once per
/// view. Shared with [`crate::check_auth`], which reads the same declarations
/// through the same parse.
pub(crate) fn read_sources(
    units: &[SourceUnit<'_>],
    findings: &mut Vec<Finding>,
) -> (BTreeMap<HandlerKey, Declaration>, Vec<Registration>) {
    let mut declarations = BTreeMap::new();
    let mut registrations = Vec::new();
    for unit in units {
        let scan = scan_source(unit.label(), unit.source(), unit.group_prefix());
        findings.extend(scan.findings);
        for handler in scan.handlers {
            declarations
                .entry((
                    unit.group_prefix().to_string(),
                    unit.module_path().to_string(),
                    handler.name.clone(),
                ))
                // Two files claiming one module is a layout error rather than
                // two declarations; the first one read is the one reported.
                .or_insert(Declaration {
                    label: unit.label().to_string(),
                    handler,
                });
        }
        for route in scan.routes {
            registrations.push(Registration {
                key: (
                    unit.group_prefix().to_string(),
                    route.module_path,
                    route.handler,
                ),
                label: unit.label().to_string(),
                line: route.line,
            });
        }
    }
    (declarations, registrations)
}

/// One registered handler: the annotation it carries, and the operation the
/// source declares for it.
fn check_registered(
    registration: &Registration,
    declarations: &BTreeMap<HandlerKey, Declaration>,
    documented: &BTreeSet<Operation>,
    excluded_prefixes: &[&str],
    declared: &mut BTreeSet<Operation>,
    findings: &mut Vec<Finding>,
) {
    let identity = identity(&registration.key);
    let Some(declaration) = declarations.get(&registration.key) else {
        findings.push(Finding::on_line(
            &registration.label,
            registration.line,
            format!("{identity}: registered in routes![] but declared in no scanned source file"),
        ));
        return;
    };
    let handler = &declaration.handler;
    if !handler.annotated {
        findings.push(Finding::on_line(
            &declaration.label,
            handler.line,
            format!(
                "{identity}: registered in routes![] but the function carries no \
                 #[utoipa::path] annotation"
            ),
        ));
        return;
    }

    // The route-attr↔annotation path comparison that used to be here is
    // redundant, not merely dropped: every path disagreement converges on a
    // finding elsewhere. The mounted route's own path missing from the document
    // surfaces at `--check-openapi` as mounted-but-absent from the spec (the
    // load-bearing direction), the annotation's path missing from it is the
    // "declared in source but absent from the spec" finding below, and a handler
    // registered under a path no route serves is the route's own registration
    // checks. That comparison was also the last whole-path Rocket-to-OpenAPI
    // translation in this crate, which is what allowed the translation to leave
    // it. The method comparison stays: it needs no translation.
    if let (Some(route), Some(annotated)) = (handler.method, handler.spec_method)
        && route != annotated
    {
        findings.push(Finding::on_line(
            &declaration.label,
            handler.line,
            format!(
                "{identity}: the route declares {} but its #[utoipa::path] declares {}",
                verb(route),
                verb(annotated)
            ),
        ));
    }

    // A declaration that disagrees with its own route is not reported against
    // the document: the document inherits the disagreement from the annotation,
    // so the finding above names the cause and a second one about the document
    // would only restate it.
    let Some(operation) = declared_operation(handler) else {
        return;
    };
    if is_excluded(&operation.path, excluded_prefixes) {
        return;
    }
    if operation.agrees_with_route
        && !documented.contains(&(operation.method, operation.path.clone()))
    {
        findings.push(Finding::on_line(
            &declaration.label,
            handler.line,
            format!(
                "{identity}: {} {} is declared in source but absent from the spec",
                verb(operation.method),
                operation.path
            ),
        ));
    }
    declared.insert((operation.method, operation.path));
}

/// Spec operations that no scanned route declares.
fn check_spec_coverage(
    spec_label: &str,
    documented: &BTreeSet<Operation>,
    declared: &BTreeSet<Operation>,
    findings: &mut Vec<Finding>,
) {
    for (method, path) in documented.difference(declared) {
        findings.push(Finding {
            file: spec_label.to_string(),
            line: None,
            message: format!(
                "{} {path} is in the spec but no scanned route declares it",
                verb(*method)
            ),
        });
    }
}

/// `operationId`s claimed by more than one operation. Generated clients derive
/// method names from them, so a collision silently merges two endpoints.
fn check_operation_ids(spec_label: &str, spec: &[SpecOperation<'_>], findings: &mut Vec<Finding>) {
    let mut owners: BTreeMap<&str, Vec<&SpecOperation<'_>>> = BTreeMap::new();
    for operation in spec {
        if let Some(id) = operation.operation_id {
            owners.entry(id).or_default().push(operation);
        }
    }

    for (id, claimants) in owners {
        if claimants.len() < 2 {
            continue;
        }
        let mut claims: Vec<String> = claimants
            .iter()
            .map(|operation| format!("{} {}", verb(operation.method), operation.path))
            .collect();
        claims.sort_unstable();
        findings.push(Finding {
            file: spec_label.to_string(),
            line: None,
            message: format!(
                "duplicate operationId `{id}` claimed by {}",
                claims.join(", ")
            ),
        });
    }
}

/// The rendered identity of a handler, as `routes![]` would qualify it.
pub(crate) fn identity(key: &HandlerKey) -> String {
    format!("{}::{}", key.1, key.2)
}

/// Whether an operation is deliberately outside the compared contract.
pub(crate) fn is_excluded(path: &str, excluded_prefixes: &[&str]) -> bool {
    excluded_prefixes
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

/// A method as the spec and the diagnostics spell it.
pub(crate) fn verb(method: HttpMethod) -> String {
    method.as_str().to_ascii_uppercase()
}
