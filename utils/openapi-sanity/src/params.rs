//! The parameters, request bodies and operation ids a handler declares against
//! the ones the document declares.
//!
//! Everything else in this crate compares the *shape* of the contract: a handler
//! against its annotation, an operation against the document, a tag against the
//! taxonomy, a guard against the auth policy. Those all hold whatever the
//! operation takes as input. This holds what it takes, and the gap it closes is
//! not hypothetical: of the sixty-one public operations in `backend/openapi.json`
//! when it was measured, ten declared no path parameter at all, eleven query
//! parameters were bound by a route and documented by none, and two multipart
//! handlers declared a JSON body of no named type. Every one of those reads as a
//! complete operation in the generated reference, and every one of them makes the
//! reference wrong for a caller trying to build a request.
//!
//! # Five rules, one per way an input can be described twice
//!
//! | Rule | Finding | Drift it catches |
//! | --- | --- | --- |
//! | P1 | a path placeholder, a route segment and an `in: path` parameter that are not the same set | an undocumented segment, or a parameter no route binds |
//! | P2 | a `?<a>&<b>` name and an `in: query` parameter that are not the same set, or a `required` that disagrees with `Option<T>` | a query parameter nobody can see, and one a caller may not omit |
//! | P3 | a `data = "<x>"` argument with no `requestBody`, a `requestBody` with no argument, and a declared type that is not the type the argument binds | a body documented as `Value` on a typed handler, and a typed body on a `Value` handler |
//! | P4 | an `operationId` that is not the handler's name | an id a generated client cannot derive from the source |
//! | P5 | a `$ref` to a component schema the document does not define, and a defined schema nothing references | a dangling reference, and a schema kept alive by nothing |
//!
//! # What a rule is not allowed to claim
//!
//! A handler whose annotation disagrees with its own route attribute is skipped.
//! The document inherits that disagreement from the annotation, so its placeholders
//! describe a route the handler does not serve, and every P1 finding would be a
//! second wording of the local finding [`crate::check_contract`] already made.
//!
//! An operation the document omits is skipped too, for the same reason: a handler
//! with no operation in the document has nothing to compare a parameter against,
//! and its absence is one finding of its own.
//!
//! P5 is the one rule with no handler side, and it reads the whole document
//! rather than the compared contract — a schema is either named by the document or
//! it is not, whichever path names it. It follows the duplicate-`operationId` rule
//! in [`crate::check_contract`] in that: the exclusions describe the published
//! contract, and a component is not part of it.
//!
//! # The document is read here, not passed in
//!
//! P1–P4 read the operations, P5 reads `components/schemas`, and only a document
//! carries both. The check therefore takes the parsed document rather than a
//! `&[SpecOperation]` derived from somewhere else: passing both would let a caller
//! compare a handler against one document and report a schema from another.
//!
//! # Every finding names the fix
//!
//! The messages here are longer than the rest of the crate's because a parameter
//! drift has a specific remedy — add this `in: path` entry, name this schema,
//! bind this argument as an `Option` — and "the operation and the handler disagree"
//! would leave the reader to derive it. Which of the two spellings is the wrong one
//! is not decided: the route and the annotation are both reviewed code, and a
//! finding that named a culprit would be a claim the analyzer cannot support.

use std::collections::{BTreeMap, BTreeSet};

use crate::contract::{
    BodySchema, HandlerKey, ParameterLocation, RequestBody, SchemaIndex, SpecOperation,
    SpecParameter, declared_operation, identity, is_excluded, read_sources, schema_index,
    spec_operations,
};
use crate::finding::Finding;
use crate::handlers::{ArgKind, Handler, HandlerArg, HttpMethod};
use crate::modules::SourceUnit;
use crate::path::{route_query_bindings, route_segments, spec_placeholders};

/// The media type Rocket's form wrapper reads a multipart body as, and the one
/// that makes such a body describable.
const MULTIPART: &str = "multipart/form-data";

/// The JSON Schema primitive types, as the Rust spelling a handler argument would
/// have to use to match one.
///
/// A `type` keyword names a JSON Schema primitive rather than a component schema,
/// and the two spellings of the same thing differ: `request_body = String` reaches
/// the document as `{"type": "string"}`. Comparing them verbatim would report every
/// primitive body in the API as a mismatch, so the primitives are mapped once.
/// Stated assumption: a handler binds an integer body as `i64` and a
/// floating-point one as `f64`, which is what the standard library's conversions
/// produce and what this codebase's handlers use.
const PRIMITIVE_TYPES: &[(&str, &str)] = &[
    ("boolean", "bool"),
    ("integer", "i64"),
    ("number", "f64"),
    ("string", "String"),
];

/// A `(method, path)` pair, the identity an operation is compared by.
type Operation = (HttpMethod, String);

/// Every way a handler's declared input disagrees with the document's.
///
/// `units` and `excluded_prefixes` mean what they mean in [`crate::check_contract`]:
/// `units` must hold the route tables *and* the handlers they register, and the
/// excluded operations are outside the compared contract in both directions, so
/// the test-only surface the public artifact strips is not asked to document
/// parameters it does not publish.
///
/// Findings are sorted by file, line and message, so two runs over the same inputs
/// report the same things in the same order.
#[must_use]
pub fn check_params(
    units: &[SourceUnit<'_>],
    spec_label: &str,
    document: &serde_json::Value,
    excluded_prefixes: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let (declarations, mut registrations) = read_sources(units, &mut findings);
    registrations.sort();

    let spec = spec_operations(document);
    let documented: BTreeMap<Operation, &SpecOperation<'_>> = spec
        .iter()
        .filter(|operation| !is_excluded(operation.path, excluded_prefixes))
        .map(|operation| ((operation.method, operation.path.to_string()), operation))
        .collect();

    let mut seen: BTreeSet<&HandlerKey> = BTreeSet::new();
    for registration in &registrations {
        // A handler registered twice declares its contract once, and the duplicate
        // registration is `check_contract`'s finding; comparing the same signature
        // against the document twice would report its drift twice with it.
        if !seen.insert(&registration.key) {
            continue;
        }
        let Some(declaration) = declarations.get(&registration.key) else {
            // A registered handler no scanned file declares has no signature to
            // read; `check_contract` names the missing declaration.
            continue;
        };
        check_handler(
            &declaration.label,
            &identity(&registration.key),
            &declaration.handler,
            &documented,
            excluded_prefixes,
            &mut findings,
        );
    }

    check_schemas(spec_label, &schema_index(document), &mut findings);

    findings.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.message.cmp(&b.message))
    });
    findings
}

/// The four source-side rules, for one registered handler.
fn check_handler(
    label: &str,
    identity: &str,
    handler: &Handler,
    documented: &BTreeMap<Operation, &SpecOperation<'_>>,
    excluded_prefixes: &[&str],
    findings: &mut Vec<Finding>,
) {
    if !handler.annotated {
        // A handler with no annotation declares no operation, and its absence from
        // the document is `check_contract`'s finding.
        return;
    }
    let Some(operation) = declared_operation(handler) else {
        return;
    };
    if !operation.agrees_with_route {
        // The annotation already disagrees with the route, so the document's
        // placeholders describe a route this handler does not serve.
        return;
    }
    if is_excluded(&operation.path, excluded_prefixes) {
        return;
    }
    let Some(documented) = documented.get(&(operation.method, operation.path.clone())) else {
        // The operation the document does not declare, which is
        // `check_contract`'s finding; there is nothing here to compare against.
        return;
    };

    check_path_parameters(label, identity, handler, documented, findings);
    check_query_parameters(label, identity, handler, documented, findings);
    check_request_body(label, identity, handler, documented, findings);
    check_operation_id(label, identity, handler, documented, findings);
}

/// P1: the template's placeholders, the route's segments and the declared
/// `in: path` parameters are the same set.
///
/// Three names for one value, and each of the three can be the odd one out: a
/// template placeholder the route does not serve, a route segment the document
/// does not name, and a declared parameter no route binds. A path parameter is
/// also required by `OpenAPI` whatever the handler does with it, so a
/// `required: false` there is a fourth way the same value is described twice.
///
/// Anchored at the handler: the route is what declares the segment, and the fix
/// belongs in the annotation that has to redeclare it.
fn check_path_parameters(
    label: &str,
    identity: &str,
    handler: &Handler,
    operation: &SpecOperation<'_>,
    findings: &mut Vec<Finding>,
) {
    let Some(uri) = handler.uri.as_deref() else {
        return;
    };
    let segments = route_segments(uri);
    let bound = to_set(&segments);
    let placeholders = spec_placeholders(operation.path);
    let template = to_set(&placeholders);
    let declared = declared_names(operation, ParameterLocation::Path);

    for name in template.difference(&bound) {
        findings.push(Finding::on_line(
            label,
            handler.line,
            format!(
                "{identity}: the documented path binds `{name}` but the route serves no such \
                 segment"
            ),
        ));
    }
    for name in bound.difference(&template) {
        findings.push(Finding::on_line(
            label,
            handler.line,
            format!(
                "{identity}: the route serves a `{name}` segment but the documented path binds \
                 no such placeholder"
            ),
        ));
    }
    for name in bound.difference(&declared) {
        findings.push(Finding::on_line(
            label,
            handler.line,
            format!(
                "{identity}: the route binds path parameter `{name}` but the operation \
                 declares no `in: path` parameter by that name"
            ),
        ));
    }
    for name in declared.difference(&bound) {
        findings.push(Finding::on_line(
            label,
            handler.line,
            format!(
                "{identity}: the operation declares path parameter `{name}` but the route \
                 binds no such segment"
            ),
        ));
    }
    for parameter in declared_parameters(operation, ParameterLocation::Path)
        .filter(|parameter| !parameter.required)
    {
        findings.push(Finding::on_line(
            label,
            handler.line,
            format!(
                "{identity}: path parameter `{}` is bound by the route and cannot be \
                 optional, but the operation declares `required: false`",
                parameter.name
            ),
        ));
    }
}

/// P2: the route's `?<a>&<b>` names and the declared `in: query` parameters are
/// the same set, and each one's `required` agrees with `Option<T>`.
///
/// A query parameter is bound by the route whether or not the document mentions
/// it, so an undocumented one is invisible in the reference while still deciding
/// what a caller may omit. The `required` half is the other direction of the same
/// drift: Rocket refuses a request missing a `?<name>` unless the argument is an
/// `Option`, so a document that says otherwise describes a request the route
/// rejects.
fn check_query_parameters(
    label: &str,
    identity: &str,
    handler: &Handler,
    operation: &SpecOperation<'_>,
    findings: &mut Vec<Finding>,
) {
    let Some(uri) = handler.uri.as_deref() else {
        return;
    };
    let query = route_query_bindings(uri);
    let bound = to_set(&query);
    let declared = declared_names(operation, ParameterLocation::Query);

    for name in bound.difference(&declared) {
        findings.push(Finding::on_line(
            label,
            handler.line,
            format!(
                "{identity}: the route binds query parameter `{name}` but the operation \
                 declares no `in: query` parameter by that name"
            ),
        ));
    }
    for name in declared.difference(&bound) {
        findings.push(Finding::on_line(
            label,
            handler.line,
            format!(
                "{identity}: the operation declares query parameter `{name}` but the route \
                 binds no such query segment"
            ),
        ));
    }

    for parameter in declared_parameters(operation, ParameterLocation::Query) {
        if !bound.contains(parameter.name) {
            continue;
        }
        // A route that binds a query parameter the handler does not declare as an
        // argument does not compile, so an argument that is not found here means
        // the signature is one this analyzer cannot read: there is no `Option<T>`
        // to compare, and saying otherwise would be a guess.
        let Some(argument) = plain_argument(handler, parameter.name) else {
            continue;
        };
        if parameter.required == argument.optional {
            findings.push(Finding::on_line(
                label,
                handler.line,
                mismatch_required(identity, parameter.name, argument.optional),
            ));
        }
    }
}

/// The diagnostic for a `required` flag that disagrees with the bound argument.
fn mismatch_required(identity: &str, name: &str, optional: bool) -> String {
    if optional {
        format!(
            "{identity}: the operation declares query parameter `{name}` as `required: true` \
             but the handler binds it as an `Option`"
        )
    } else {
        format!(
            "{identity}: the operation declares query parameter `{name}` as `required: false` \
             but the handler binds it as a required argument"
        )
    }
}

/// P3: a `data = "<x>"` argument exists exactly when the operation declares a
/// request body, and the declared type is the type the argument binds.
///
/// The two halves fail independently. A handler with a body and an annotation
/// without one documents an operation no caller can find a payload for; an
/// annotation with a body and a handler without one documents a payload the route
/// never reads. Once both are there, the declared schema has to name the type —
/// and `request_body = Value` is a finding on a typed handler in either
/// direction, because `Value` tells a generator nothing about the body it has to
/// send.
///
/// A form body is the one case where naming the type is not enough: Rocket's
/// `Form<T>` is filled from a form-encoded or multipart request, so the media type
/// is part of the claim. The rule accepts a document that declares
/// `multipart/form-data`; naming the inner type under any other media type
/// describes a body a caller does not know how to send, and that finding names the
/// media type the document did declare.
fn check_request_body(
    label: &str,
    identity: &str,
    handler: &Handler,
    operation: &SpecOperation<'_>,
    findings: &mut Vec<Finding>,
) {
    let bound = handler
        .args
        .iter()
        .find(|argument| argument.kind == ArgKind::Data);

    match (bound, operation.request_body.as_ref()) {
        (None, None) => {}
        (Some(argument), None) => findings.push(Finding::on_line(
            label,
            handler.line,
            format!(
                "{identity}: the route binds its body to `{}` but the operation declares no \
                 request body",
                argument.name
            ),
        )),
        (None, Some(_)) => findings.push(Finding::on_line(
            label,
            handler.line,
            format!(
                "{identity}: the operation declares a request body but the route binds no \
                 `data = \"<…>\"` argument"
            ),
        )),
        (Some(argument), Some(declared)) => {
            if let Some(drift) = body_drift(argument, declared) {
                findings.push(Finding::on_line(
                    label,
                    handler.line,
                    format!("{identity}: {drift}"),
                ));
            }
        }
    }
}

/// The finding a request body earns, or `None` when the document and the handler
/// describe the same body.
fn body_drift(argument: &HandlerArg, declared: &RequestBody<'_>) -> Option<String> {
    let observed = argument
        .body_type
        .as_deref()
        .unwrap_or("a type this analyzer cannot name");
    let documented = documented_body_name(&declared.schema);

    if argument.multipart {
        if declared.content_types.contains(&MULTIPART) {
            return None;
        }
        if matches!(declared.schema, BodySchema::Named(name) if name == observed) {
            // The schema names the type, so the only thing missing is how to send
            // the body: the finding is about the media type, not the schema.
            let media = declared
                .content_types
                .first()
                .copied()
                .unwrap_or("no media type");
            return Some(format!(
                "the operation describes the `Form` body as `{media}` but the route binds it \
                 as `{observed}` — declare the body `{MULTIPART}`"
            ));
        }
        return Some(format!(
            "the operation declares request body `{documented}` but the route binds a `Form` \
             body to `{observed}` — name the schema `{observed}` or declare the body \
             `{MULTIPART}`"
        ));
    }

    (documented != observed).then(|| {
        format!(
            "the operation declares request body `{documented}` but the route binds its body \
             to `{observed}`"
        )
    })
}

/// The type name a declared body schema states, as the Rust spelling a handler
/// argument would have to use to match it.
///
/// An untyped schema names `Value`, which is what utoipa writes for
/// `request_body = Value` and `request_body = serde_json::Value`: the document
/// states that the body is arbitrary JSON, and a handler binding `Json<Value>`
/// states the same thing.
fn documented_body_name(schema: &BodySchema<'_>) -> String {
    match schema {
        BodySchema::Named(name) => (*name).to_string(),
        BodySchema::Primitive(primitive) => PRIMITIVE_TYPES
            .iter()
            .find(|(json, _)| *json == *primitive)
            .map_or_else(|| (*primitive).to_string(), |(_, rust)| (*rust).to_string()),
        BodySchema::Untyped => "Value".to_string(),
    }
}

/// P4: the documented `operationId` is the handler's name.
///
/// utoipa's default, and the reason a generated client can call an operation by
/// the name in the source. An id nobody derived is the one thing in the document
/// that cannot be checked against anything else, so it is compared against the
/// function it documents rather than trusted: a rename in either place, or an
/// annotation that set an id by hand, moves the name a caller has to use.
///
/// Stability of the id across reviewed changes is deliberately not this rule's
/// job — a diff of the document is what shows a rename nobody meant, and a rule
/// here would only know that the name had changed.
fn check_operation_id(
    label: &str,
    identity: &str,
    handler: &Handler,
    operation: &SpecOperation<'_>,
    findings: &mut Vec<Finding>,
) {
    match operation.operation_id {
        None => findings.push(Finding::on_line(
            label,
            handler.line,
            format!(
                "{identity}: the operation declares no operationId, so it is documented under \
                 no name a generated client could call"
            ),
        )),
        Some(id) if id != handler.name => findings.push(Finding::on_line(
            label,
            handler.line,
            format!(
                "{identity}: the operation declares operationId `{id}` but the handler is \
                 named `{}`",
                handler.name
            ),
        )),
        Some(_) => {}
    }
}

/// P5: every `$ref` resolves to a defined component schema, and every defined
/// component schema is referenced.
///
/// Both directions, because either one alone is satisfied by a document whose
/// schemas and operations have drifted apart: a `$ref` to a name the document does
/// not define breaks every generator that resolves it, and a schema nothing
/// references is one that has stopped being part of the contract while still
/// occupying the document — `FileEntry` is registered as a component and named by
/// no operation.
///
/// Document-shaped, like the duplicate-`operationId` rule: an anonymous schema has
/// no line, and the whole document is read rather than the compared contract.
fn check_schemas(spec_label: &str, index: &SchemaIndex<'_>, findings: &mut Vec<Finding>) {
    for name in index.referenced.difference(&index.defined) {
        findings.push(Finding {
            file: spec_label.to_string(),
            line: None,
            message: format!(
                "`$ref` to the component schema `{name}`, which the document does not define"
            ),
        });
    }
    for name in index.defined.difference(&index.referenced) {
        findings.push(Finding {
            file: spec_label.to_string(),
            line: None,
            message: format!("component schema `{name}` is defined but nothing references it"),
        });
    }
}

/// The parameters an operation declares at one location, in document order.
fn declared_parameters<'a>(
    operation: &'a SpecOperation<'a>,
    location: ParameterLocation,
) -> impl Iterator<Item = &'a SpecParameter<'a>> {
    operation
        .parameters
        .iter()
        .filter(move |parameter| parameter.location == location)
}

/// The names an operation declares at one location.
fn declared_names<'a>(
    operation: &'a SpecOperation<'a>,
    location: ParameterLocation,
) -> BTreeSet<&'a str> {
    declared_parameters(operation, location)
        .map(|parameter| parameter.name)
        .collect()
}

/// The plain argument a route segment or query key binds, if the signature
/// declares one.
///
/// A guard is not the argument: a guard of the same name would answer before the
/// body runs, and reading its `Option` as the query parameter's optionality would
/// answer a policy question with a contract one.
fn plain_argument<'a>(handler: &'a Handler, name: &str) -> Option<&'a HandlerArg> {
    handler
        .args
        .iter()
        .find(|argument| argument.kind == ArgKind::Plain && argument.name == name)
}

/// A list of names as a set, for the comparisons above.
fn to_set(names: &[String]) -> BTreeSet<&str> {
    names.iter().map(String::as_str).collect()
}
