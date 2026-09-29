//! Static analysis of Rocket routes and `#[utoipa::path]` annotations.
//!
//! # What this answers
//!
//! The public contract of a Rocket + utoipa service is decided in two places that
//! are not the runtime route table: a `routes![...]` block says which routes
//! exist, and a `#[utoipa::path]` annotation says which of them are documented.
//! This crate reads both from source, so a build script, a test and a CLI can ask
//! the same questions and get the same answers.
//!
//! ```no_run
//! let scan = openapi_sanity::scan_source(
//!     "src/router/get/mod.rs",
//!     "pub fn routes() { routes![get_page::login] }",
//!     "get",
//! );
//! assert_eq!(scan.routes[0].handler, "login");
//! ```
//!
//! # Shape of the API
//!
//! [`scan_source`] parses a file once and answers both questions;
//! [`scan_routes`] and [`scan_handlers`] parse it for one of them. Results are
//! plain owned types ([`RouteScan`], [`HandlerScan`], [`SourceScan`]) — no `syn`
//! type is part of the surface, so the parser can be replaced without touching a
//! caller.
//!
//! # Diagnostics instead of failures
//!
//! Nothing here reads the filesystem, prints, or panics — except
//! [`spec_operations`], which rejects a document that is not a `paths` object,
//! since a document that does not parse is a broken input rather than a
//! contract with no operations. A file label is passed in and copied into the
//! findings it produces, a malformed `routes![]` entry is reported rather than
//! guessed at, and a syntax error becomes a [`Finding`] with a file and line.
//! What to do with a finding — a build warning, a gate failure, a CLI
//! diagnostic — belongs to the caller.
//!
//! # Comparing source with a spec
//!
//! [`check_contract`] is the one place that compares the two views with each
//! other: a handler registered in `routes![]` against its own annotation, and
//! the operations source declares against the operations an `OpenAPI` document
//! lists. [`SCANNED_MODULES`] is the list of router files the contract is read
//! from, shared with the build script so the two cannot disagree about which
//! files make up the API, and [`referenced_handler_files`] tells a caller which
//! further files it has to read to resolve the handlers those modules register.
//!
//! # Who may call an operation
//!
//! The two views say nothing about authentication, and that is the one property of
//! a public API a consumer cannot read off the spec: an operation documenting a
//! `401` and serving an anonymous caller is indistinguishable from one enforcing
//! its guard. [`check_auth`] closes that gap against [`AUTH_POLICY`], a table with
//! one entry per documented operation naming the guards its handler must declare
//! or stating why the operation is deliberately open. [`GuardClass`] is the
//! vocabulary, [`KNOWN_GUARDS`] the single place a guard type enters it.
//!
//! # What the reference groups by
//!
//! [`check_tags`] holds the document to the subject taxonomy in [`KNOWN_TAGS`],
//! and keeps the reserved `pages` tag on the SPA page routes and off everything
//! else. A tag is a documentation grouping, so it says nothing about
//! authentication — the two are separate policies, and neither is derived from
//! the other.
//!
//! # What an operation takes
//!
//! The other rules compare the *shape* of the contract; [`check_params`]
//! compares what a handler binds against what the document declares it takes —
//! the path, query and form parameters a Rocket URI binds by name, the request
//! body a `data = "<x>"` argument receives, and the `operationId` a generated
//! client calls the operation by. It is the one group with no policy table,
//! because there is nothing to classify: a route either declares a parameter and
//! the document redeclares it, or the two disagree and the document is wrong.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod auth;
mod contract;
mod finding;
mod guards;
mod handlers;
mod modules;
mod params;
mod path;
mod routes;
mod tags;

pub use auth::{AUTH_POLICY, AuthRule, Unauthenticated, check_auth};
pub use contract::{
    BodySchema, ParameterLocation, RequestBody, SchemaIndex, SpecOperation, SpecParameter,
    check_contract, referenced_handler_files, schema_index, spec_operations,
};
pub use finding::Finding;
pub use guards::{Discard, Enforcement, GuardBinding, GuardClass, KNOWN_GUARDS};
pub use handlers::{ArgKind, Handler, HandlerArg, HandlerScan, HttpMethod};
pub use modules::{SCANNED_MODULES, SourceUnit, handler_module_path};
pub use params::check_params;
pub use path::{route_bindings, route_query_bindings, route_segments, spec_placeholders};
pub use routes::{HandlerRef, RouteScan};
pub use tags::{KNOWN_TAGS, check_tags};

/// Handler references and route facts found in one source file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceScan {
    /// Handler references from every `routes![...]` block, in source order.
    pub routes: Vec<HandlerRef>,
    /// Functions declaring a route, a `#[utoipa::path]` operation, or both.
    pub handlers: Vec<Handler>,
    /// Malformed input found in the file; empty when it parsed cleanly.
    pub findings: Vec<Finding>,
}

/// Scan one source file for both the `routes![...]` blocks it declares and the
/// route/annotation facts of its handler functions.
///
/// The file is parsed once, which is what this exists for next to
/// [`scan_routes`] and [`scan_handlers`]: a caller comparing the two views pays
/// for one parse, and a syntax error is reported once instead of twice.
#[must_use]
pub fn scan_source(file: &str, source: &str, group_prefix: &str) -> SourceScan {
    match parse(file, source) {
        Ok(ast) => {
            let routes = routes::scan(&ast, file, group_prefix);
            let handlers = handlers::scan(&ast, file);
            SourceScan {
                findings: routes
                    .findings
                    .into_iter()
                    .chain(handlers.findings)
                    .collect(),
                routes: routes.handlers,
                handlers: handlers.handlers,
            }
        }
        Err(finding) => SourceScan {
            findings: vec![finding],
            ..SourceScan::default()
        },
    }
}

/// Handler references registered by every `routes![...]` block in `source`.
///
/// Entries are comma-separated, not line-separated, so a single-line
/// `routes![a, b]` and its multi-line form yield the same handlers. Unqualified
/// entries resolve against `group_prefix`: `routes![delete_data]` scanned with
/// `"delete"` yields the reference `delete::delete_data`. An entry that is not a
/// plain `ident` or `path::ident` is reported in
/// [`RouteScan::findings`] rather than resolved by guesswork.
#[must_use]
pub fn scan_routes(file: &str, source: &str, group_prefix: &str) -> RouteScan {
    match parse(file, source) {
        Ok(ast) => routes::scan(&ast, file, group_prefix),
        Err(finding) => RouteScan {
            findings: vec![finding],
            ..RouteScan::default()
        },
    }
}

/// Rocket route attributes and `#[utoipa::path]` annotations declared by the
/// functions in `source`.
///
/// Only functions that declare a route or an annotation are reported; helpers,
/// guards and conversions are not part of the contract.
#[must_use]
pub fn scan_handlers(file: &str, source: &str) -> HandlerScan {
    match parse(file, source) {
        Ok(ast) => handlers::scan(&ast, file),
        Err(finding) => HandlerScan {
            findings: vec![finding],
            ..HandlerScan::default()
        },
    }
}

/// Parse one source file, turning a syntax error into a finding.
///
/// A malformed file must not take the build down, and it must not be silently
/// read as "this file declares nothing": the caller gets a diagnostic with the
/// file and the line to act on.
fn parse(file: &str, source: &str) -> Result<syn::File, Finding> {
    syn::parse_file(source).map_err(|error| {
        let line = error.span().start().line;
        Finding {
            file: file.to_string(),
            line: (line > 0).then_some(line),
            message: error.to_string(),
        }
    })
}
