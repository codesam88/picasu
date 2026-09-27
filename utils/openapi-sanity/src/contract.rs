//! Consistency between the annotated handlers in source and the operations in
//! the committed `OpenAPI` document.
//!
//! Two of the failures this catches cannot be seen from either end of the
//! existing pipeline. The build script only asks whether a registered handler
//! carries an annotation, and the backend contract tests compare Rocket's
//! *runtime* route table with the spec — but an annotation can sit on a route
//! attribute and still describe a different path or a different verb. The route
//! is mounted, the spec documents something else, and every other check passes.
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
use crate::path::to_spec_path;
use crate::scan_source;

/// One operation declared by an `OpenAPI` document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SpecOperation<'a> {
    /// Method, from the path item's key.
    pub method: HttpMethod,
    /// Path template, e.g. `/get/metadata/{asset_id}`.
    pub path: &'a str,
    /// The operation's `operationId`, when it declares one.
    pub operation_id: Option<&'a str>,
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
                })
            }))
        })
        .flatten()
        .collect();
    operations.sort_unstable();
    operations
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
type HandlerKey = (String, String, String);

/// A `(method, path)` pair, the identity an operation is compared by.
type Operation = (HttpMethod, String);

/// A route-declaring function, with the file it was read from.
struct Declaration {
    label: String,
    handler: Handler,
}

/// One `routes![...]` entry, with the file and line it was registered at.
///
/// Ordered by identity and then by where it was registered, so the first entry
/// of a run of equal keys is the handler's declaration and the rest are the
/// duplicates — without depending on the order the files were read in.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Registration {
    key: HandlerKey,
    label: String,
    line: usize,
}

/// The route-declaring functions and the `routes![]` entries of every unit, plus
/// the diagnostics their input produced.
///
/// Each file is parsed once for both views, which is what [`scan_source`]
/// exists for, and a syntax error in a file is reported once instead of once per
/// view.
fn read_sources(
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

    let route_path = handler.uri.as_deref().map(to_spec_path);
    let mut agrees_with_route = true;
    if let (Some(route), Some(annotated)) = (route_path.as_deref(), handler.spec_path.as_deref())
        && route != annotated
    {
        agrees_with_route = false;
        findings.push(Finding::on_line(
            &declaration.label,
            handler.line,
            format!(
                "{identity}: the route serves {route} but its #[utoipa::path] declares \
                 {annotated}"
            ),
        ));
    }
    if let (Some(route), Some(annotated)) = (handler.method, handler.spec_method)
        && route != annotated
    {
        agrees_with_route = false;
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

    // utoipa derives a path from the route when the annotation omits one, so
    // the annotation wins where it speaks and the route fills the silence. The
    // document is generated from the annotation, so the annotation is also what
    // counts as the source declaration of the operation — including when it
    // disagrees with the route, which is why a disagreeing handler still
    // occupies its operation in the document instead of looking undeclared.
    let method = handler.spec_method.or(handler.method);
    let path = handler.spec_path.as_deref().or(route_path.as_deref());
    let (Some(method), Some(path)) = (method, path) else {
        return;
    };
    if is_excluded(path, excluded_prefixes) {
        return;
    }

    // A declaration that disagrees with its own route is not reported against
    // the document: the document inherits the disagreement from the annotation,
    // so the finding above names the cause and a second one about the document
    // would only restate it.
    if agrees_with_route && !documented.contains(&(method, path.to_string())) {
        findings.push(Finding::on_line(
            &declaration.label,
            handler.line,
            format!(
                "{identity}: {} {path} is declared in source but absent from the spec",
                verb(method)
            ),
        ));
    }
    declared.insert((method, path.to_string()));
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
fn identity(key: &HandlerKey) -> String {
    format!("{}::{}", key.1, key.2)
}

/// Whether an operation is deliberately outside the compared contract.
fn is_excluded(path: &str, excluded_prefixes: &[&str]) -> bool {
    excluded_prefixes
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

/// A method as the spec and the diagnostics spell it.
fn verb(method: HttpMethod) -> String {
    method.as_str().to_ascii_uppercase()
}
