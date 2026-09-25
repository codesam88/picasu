//! AST-based discovery of Rocket route handlers and `routes![...]` members.
//!
//! Shared by `build.rs` and its unit tests (see `src/tests/ast_scan.rs`) so the
//! analysis that decides which handlers reach `paths(...)` is covered by
//! `cargo test`. A parsing mistake here is silent: the build emits a coverage
//! warning at most, and the affected routes simply vanish from the spec while
//! staying mounted.
//!
//! The module is pure. It takes source text plus a file path (used only to
//! build messages) and returns findings; it never touches the file system and
//! never prints. `build.rs` does the reading and renders every finding as a
//! `cargo:warning=` line. Nothing here panics — a file that does not parse
//! comes back as [`Finding::UnparsableFile`], so a broken source cannot abort
//! the build with a panic.
//!
//! Scope of the `routes![...]` discovery: every invocation that reaches the
//! AST is collected, in any position (expression, statement, item). An
//! invocation nested inside another macro's token stream never becomes an AST
//! node and is therefore not visible here — the same limitation any
//! source-level pass has.

use std::path::Path;

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::Visit;
use syn::{Attribute, File, ItemFn, LitStr, Macro, Meta};

/// Rocket HTTP-method attributes that mark a function as a route handler.
const VERB_ATTRIBUTES: [&str; 7] = ["get", "post", "put", "delete", "patch", "head", "options"];

/// One handler reference from a `routes![...]` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerRef {
    /// Module path relative to the scanned router file, e.g. `get_page` for
    /// `get_page::login`. Unqualified entries resolve to the group prefix.
    pub module_path: String,
    /// Bare handler name, e.g. `login`.
    pub handler: String,
}

/// A function carrying a Rocket verb attribute — a route handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteHandler {
    /// Function name, e.g. `get_data`.
    pub name: String,
    /// The Rocket attribute name, e.g. `get`.
    pub verb: String,
    /// First string literal in the Rocket attribute — the declared URI, e.g.
    /// `/get/get-data?<start>&<end>`. `None` when the attribute declares no
    /// string literal.
    pub rocket_path: Option<String>,
    /// Whether *this function* carries `#[utoipa::path(...)]`. The previous
    /// check was file-scoped and credited a sibling's annotation.
    pub annotated: bool,
    /// The `path = "..."` argument of this function's `#[utoipa::path]`
    /// attribute, `None` when the attribute omits it.
    pub openapi_path: Option<String>,
}

impl RouteHandler {
    /// The [`Finding::PathMismatch`] for this handler when the Rocket URI and
    /// the `utoipa::path` declaration disagree; `None` when they agree, when
    /// either side declares no path, or when either path is not a plain string
    /// literal (nothing to compare).
    ///
    /// The Rocket URI is rendered in spec form with
    /// `super::route_path::to_spec_path` — the same translation the
    /// mounted-route parity gate uses, so the two comparisons cannot drift
    /// apart. The translation is called directly rather than passed in as a
    /// parameter: a caller that could supply its own translation could
    /// silently neutralise the check. The including file must declare
    /// `route_path` as a sibling module (`build.rs` and `tests/ast_scan.rs`
    /// both do).
    pub fn path_mismatch(&self, file: &Path) -> Option<Finding> {
        let rocket_path = self.rocket_path.as_ref()?;
        let openapi_path = self.openapi_path.as_ref()?;
        let spec_path = super::route_path::to_spec_path(rocket_path);
        if spec_path == *openapi_path {
            return None;
        }
        Some(Finding::PathMismatch {
            handler: self.name.clone(),
            verb: self.verb.clone(),
            rocket_path: rocket_path.clone(),
            openapi_path: openapi_path.clone(),
            file: file.display().to_string(),
        })
    }
}

/// A problem found while scanning a source file. Pure data: `build.rs`
/// renders each finding as a `cargo:warning=` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// A `routes![]` entry that is not a plain `handler` or `module::handler`
    /// path and therefore cannot be resolved to a function.
    UnparsableRoutesEntry {
        /// The entry rendered from its token stream, e.g. `some_macro ! ()`.
        entry: String,
    },
    /// A handler whose Rocket attribute and `#[utoipa::path(path = "...")]`
    /// declare different paths once the Rocket URI is translated with the
    /// shared `route_path::to_spec_path`.
    PathMismatch {
        /// Function the disagreement was found on.
        handler: String,
        /// Rocket verb attribute the URI came from, e.g. `get`.
        verb: String,
        /// URI declared by the Rocket attribute.
        rocket_path: String,
        /// Path declared by the `utoipa::path` attribute.
        openapi_path: String,
        /// File the handler is declared in (message context only).
        file: String,
    },
    /// The source file could not be parsed; it was skipped entirely.
    UnparsableFile {
        /// File that failed to parse (message context only).
        file: String,
        /// Parser error as reported by `syn`.
        error: String,
    },
}

impl Finding {
    /// The message `build.rs` prints after `cargo:warning=`.
    pub fn message(&self) -> String {
        match self {
            Finding::UnparsableRoutesEntry { entry } => {
                format!("ignoring unparsable routes![] entry: {entry}")
            }
            Finding::PathMismatch {
                handler,
                verb,
                rocket_path,
                openapi_path,
                file,
            } => format!(
                "utoipa path mismatch on {handler} in {file}: #[{verb}] declares \
                 \"{rocket_path}\" but utoipa::path declares \"{openapi_path}\""
            ),
            Finding::UnparsableFile { file, error } => format!("failed to parse {file}: {error}"),
        }
    }
}

/// Outcome of scanning one source file for `routes![...]` blocks.
#[derive(Debug, Default)]
pub struct RoutesScan {
    /// Handlers referenced by `routes![...]` blocks, in source order.
    pub handlers: Vec<HandlerRef>,
    /// Entries — or the whole file — that could not be understood.
    pub findings: Vec<Finding>,
}

/// Outcome of scanning one source file for Rocket route handlers.
#[derive(Debug, Default)]
pub struct HandlersScan {
    /// Functions carrying a Rocket verb attribute, in source order.
    pub handlers: Vec<RouteHandler>,
    /// Path disagreements and unparsable files.
    pub findings: Vec<Finding>,
}

impl HandlersScan {
    /// The function to gate a mounted `name` on: prefer an annotated
    /// candidate, else the first one in source order.
    ///
    /// Several functions can share a name — a `#[cfg(test)]` duplicate or a
    /// test-module helper declared before the real handler, for example (the
    /// scan does not evaluate `cfg`s). First-in-source order would pick the
    /// duplicate and, finding no annotation, drop the real route from the
    /// spec; the annotated candidate is the one that defines the contract.
    pub fn candidate(&self, name: &str) -> Option<&RouteHandler> {
        self.handlers
            .iter()
            .find(|handler| handler.name == name && handler.annotated)
            .or_else(|| self.handlers.iter().find(|handler| handler.name == name))
    }
}

/// Extract the handlers referenced by the `routes![...]` invocations in
/// `content` — every invocation that reaches the AST (see the module docs for
/// the macro-nesting limitation).
///
/// The macro bodies come from the file AST and are split on token-stream
/// commas, not lines, so the single-line `routes![a, b]` form and its
/// multi-line form yield the same entries and comments inside the block are
/// handled by the tokenizer. `group_prefix` resolves unqualified entries
/// (`renew_hash_token` in the `auth` group becomes `auth::renew_hash_token`).
///
/// An entry that is not a plain `ident` or `module::ident` path is reported as
/// [`Finding::UnparsableRoutesEntry`] instead of being guessed at: a wrong
/// guess would register a nonexistent `__path_*` import. A file that does not
/// parse yields a [`Finding::UnparsableFile`] and no handlers; nothing panics.
pub fn scan_routes(content: &str, group_prefix: &str, file: &Path) -> RoutesScan {
    let mut scan = RoutesScan::default();
    let ast = match parse(content, file) {
        Ok(ast) => ast,
        Err(finding) => {
            scan.findings.push(finding);
            return scan;
        }
    };

    let mut collector = RouteMacroCollector::default();
    collector.visit_file(&ast);

    for block in collector.blocks {
        for entry in split_entries(&block) {
            if entry.is_empty() {
                continue; // trailing or repeated comma
            }
            match entry_path(&entry) {
                Some(path) => scan.handlers.push(resolve(&path, group_prefix)),
                None => scan.findings.push(Finding::UnparsableRoutesEntry {
                    entry: TokenStream::from_iter(entry).to_string(),
                }),
            }
        }
    }
    scan
}

/// Discover the route handlers declared in `content`.
///
/// A function carrying a `#[get(...)]`, `#[post(...)]`, `#[put(...)]`,
/// `#[delete(...)]`, `#[patch(...)]`, `#[head(...)]` or `#[options(...)]`
/// attribute is a route handler; the verb is the attribute name and
/// `rocket_path` the first string literal in it (the declared URI).
///
/// `annotated` reports whether *that function* carries `#[utoipa::path(...)]`.
/// The previous file-scoped check (`file contains "utoipa::path"`) credited a
/// sibling's annotation to every function in the file; the real failure only
/// surfaced later as a missing `__path_*` import rather than a warning.
///
/// When both the Rocket attribute and `#[utoipa::path(path = "...")]` declare
/// a path, [`RouteHandler::path_mismatch`] reports their disagreement as a
/// [`Finding::PathMismatch`] after applying the shared
/// `super::route_path::to_spec_path` translation — the same translation the
/// mounted-route parity gate uses, so the two comparisons cannot disagree.
/// No comparison is attempted when either path is absent or not a plain
/// string literal.
///
/// A file that does not parse yields a [`Finding::UnparsableFile`] and no
/// handlers; nothing panics.
pub fn scan_handlers(content: &str, file: &Path) -> HandlersScan {
    let mut scan = HandlersScan::default();
    let ast = match parse(content, file) {
        Ok(ast) => ast,
        Err(finding) => {
            scan.findings.push(finding);
            return scan;
        }
    };

    let mut collector = FunctionCollector::default();
    collector.visit_file(&ast);

    for function in &collector.functions {
        let Some((verb, verb_name)) = rocket_verb(&function.attrs) else {
            continue;
        };
        let handler = RouteHandler {
            name: function.sig.ident.to_string(),
            verb: verb_name.to_string(),
            rocket_path: first_string_literal(verb),
            annotated: function.attrs.iter().any(is_utoipa_path),
            openapi_path: function
                .attrs
                .iter()
                .find(|attr| is_utoipa_path(attr))
                .and_then(utoipa_path_argument),
        };
        scan.handlers.push(handler);
    }
    scan
}

/// Parse `content`; on failure produce the finding instead of panicking —
/// `build.rs` must not abort the build over a source it cannot parse.
fn parse(content: &str, file: &Path) -> Result<File, Finding> {
    syn::parse_file(content).map_err(|error| Finding::UnparsableFile {
        file: file.display().to_string(),
        error: error.to_string(),
    })
}

/// Collects every `routes![...]` macro token stream in a file. A single
/// `visit_macro` override covers every position the parser can put a macro
/// in — expression, statement, item — so each invocation is collected exactly
/// once.
#[derive(Default)]
struct RouteMacroCollector {
    blocks: Vec<TokenStream>,
}

impl<'ast> Visit<'ast> for RouteMacroCollector {
    fn visit_macro(&mut self, node: &'ast Macro) {
        collect_route_macro(&mut self.blocks, &node.path, &node.tokens);
        syn::visit::visit_macro(self, node);
    }
}

/// Push a macro's token stream when it is a `routes![...]` invocation; any
/// path whose last segment is `routes` counts (`routes!`, `rocket::routes!`).
fn collect_route_macro(blocks: &mut Vec<TokenStream>, path: &syn::Path, tokens: &TokenStream) {
    if path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "routes")
    {
        blocks.push(tokens.clone());
    }
}

/// Collects every function item in a file, in source order, so attributes can
/// be inspected per function rather than per file.
#[derive(Default)]
struct FunctionCollector {
    functions: Vec<ItemFn>,
}

impl<'ast> Visit<'ast> for FunctionCollector {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        self.functions.push(node.clone());
        syn::visit::visit_item_fn(self, node);
    }
}

/// Split a macro token stream on top-level commas. Commas inside `[...]`,
/// `(...)` or `...` groups belong to those groups and do not split.
fn split_entries(block: &TokenStream) -> Vec<Vec<TokenTree>> {
    let mut entries = Vec::new();
    let mut current = Vec::new();
    for token in block.clone() {
        match token {
            TokenTree::Punct(punct) if punct.as_char() == ',' => {
                entries.push(std::mem::take(&mut current));
            }
            other => current.push(other),
        }
    }
    entries.push(current);
    entries
}

/// Render a `routes![]` entry as `module::handler` when it is a plain path of
/// identifiers, `None` otherwise. Literals, macro calls, indexing, arithmetic
/// and a trailing `::` cannot name a handler, so they are never guessed at.
fn entry_path(entry: &[TokenTree]) -> Option<String> {
    let mut path = String::new();
    let mut index = 0;
    loop {
        match entry.get(index)? {
            TokenTree::Ident(ident) => {
                if !path.is_empty() {
                    path.push_str("::");
                }
                path.push_str(&ident.to_string());
                index += 1;
            }
            _ => return None,
        }
        match entry.get(index) {
            None => return Some(path),
            Some(TokenTree::Punct(punct)) if punct.as_char() == ':' => {
                // A separator must be a complete `::`, then another
                // identifier; a single `:` or a dangling `::` is rejected.
                match entry.get(index + 1)? {
                    TokenTree::Punct(second) if second.as_char() == ':' => index += 2,
                    _ => return None,
                }
            }
            Some(_) => return None,
        }
    }
}

/// Split `module::handler` into its parts; a bare `handler` belongs to
/// `group_prefix`.
fn resolve(path: &str, group_prefix: &str) -> HandlerRef {
    match path.rfind("::") {
        Some(position) => HandlerRef {
            module_path: path[..position].to_string(),
            handler: path[position + 2..].to_string(),
        },
        None => HandlerRef {
            module_path: group_prefix.to_string(),
            handler: path.to_string(),
        },
    }
}

/// The Rocket verb attribute on a function together with its verb name, if
/// any: `#[get(...)]` and friends, bare or qualified as `#[rocket::get(...)]`.
fn rocket_verb(attrs: &[Attribute]) -> Option<(&Attribute, &str)> {
    attrs.iter().find_map(|attr| {
        let segments = &attr.path().segments;
        let last = segments.last()?;
        let verb = VERB_ATTRIBUTES
            .into_iter()
            .find(|verb| last.ident == *verb)?;
        let qualified = segments.len() == 1
            || segments
                .first()
                .is_some_and(|first| first.ident == "rocket");
        qualified.then_some((attr, verb))
    })
}

/// Whether the attribute is `#[utoipa::path(...)]` — the annotation gate is
/// per function, never per file.
fn is_utoipa_path(attr: &Attribute) -> bool {
    let segments = &attr.path().segments;
    segments.len() == 2 && segments[0].ident == "utoipa" && segments[1].ident == "path"
}

/// First string literal at the top level of an attribute's argument list.
/// Non-string literals (`rank = 11`) are skipped, so a bare
/// `#[get(rank = 11)]` yields `None`.
fn first_string_literal(attr: &Attribute) -> Option<String> {
    let Meta::List(list) = &attr.meta else {
        return None;
    };
    for token in list.tokens.clone() {
        let TokenTree::Literal(literal) = token else {
            continue;
        };
        if let Ok(text) = syn::parse_str::<LitStr>(&literal.to_string()) {
            return Some(text.value());
        }
    }
    None
}

/// The `path = "..."` argument of a `#[utoipa::path(...)]` attribute,
/// recognised as the top-level token pattern `path = "literal"`. Nested
/// argument groups such as `responses(...)` are not searched.
fn utoipa_path_argument(attr: &Attribute) -> Option<String> {
    let Meta::List(list) = &attr.meta else {
        return None;
    };
    let tokens: Vec<TokenTree> = list.tokens.clone().into_iter().collect();
    for window in tokens.windows(3) {
        let TokenTree::Ident(ident) = &window[0] else {
            continue;
        };
        let TokenTree::Punct(punct) = &window[1] else {
            continue;
        };
        let TokenTree::Literal(literal) = &window[2] else {
            continue;
        };
        if ident == "path"
            && punct.as_char() == '='
            && let Ok(text) = syn::parse_str::<LitStr>(&literal.to_string())
        {
            return Some(text.value());
        }
    }
    None
}
