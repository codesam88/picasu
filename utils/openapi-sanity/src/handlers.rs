//! Rocket route attributes and per-function `#[utoipa::path]` annotations.

use proc_macro2::{TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Attribute, ItemFn, LitStr, Meta};

use crate::finding::Finding;

/// HTTP method a Rocket route attribute declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HttpMethod {
    /// `GET`
    Get,
    /// `POST`
    Post,
    /// `PUT`
    Put,
    /// `DELETE`
    Delete,
    /// `PATCH`
    Patch,
    /// `HEAD`
    Head,
    /// `OPTIONS`
    Options,
}

impl HttpMethod {
    /// The method a Rocket route attribute name refers to, ignoring any
    /// qualifier so that `#[get(..)]` and `#[rocket::get(..)]` are one route.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "get" => Some(Self::Get),
            "post" => Some(Self::Post),
            "put" => Some(Self::Put),
            "delete" => Some(Self::Delete),
            "patch" => Some(Self::Patch),
            "head" => Some(Self::Head),
            "options" => Some(Self::Options),
            _ => None,
        }
    }

    /// The canonical lower-case verb, as Rocket and the `OpenAPI` document spell
    /// it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Post => "post",
            Self::Put => "put",
            Self::Delete => "delete",
            Self::Patch => "patch",
            Self::Head => "head",
            Self::Options => "options",
        }
    }
}

/// A function declaring a Rocket route, a `#[utoipa::path]` operation, or both.
///
/// Both facts come from this function's own attributes. A file-level view cannot
/// tell which annotation belongs to which handler, and crediting a sibling's
/// annotation registers a route under another operation's metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handler {
    /// Function name, as `routes![...]` would refer to it.
    pub name: String,
    /// 1-based line of the function name.
    pub line: usize,
    /// Method from this function's own Rocket route attribute.
    pub method: Option<HttpMethod>,
    /// URI from this function's own Rocket route attribute, verbatim — Rocket
    /// form, including `<param..>` segments and any query string. Use
    /// [`crate::to_spec_path`] to compare it with a documented path.
    pub uri: Option<String>,
    /// Whether this function carries a `#[utoipa::path]` annotation.
    pub annotated: bool,
    /// The `path = "..."` literal of that annotation, when it declares one.
    pub spec_path: Option<String>,
}

/// Route and annotation facts declared by the functions in a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HandlerScan {
    /// Handlers in source order.
    pub handlers: Vec<Handler>,
    /// Malformed attributes plus syntax errors.
    pub findings: Vec<Finding>,
}

pub(crate) fn scan(file: &syn::File, label: &str) -> HandlerScan {
    let mut collector = Collector {
        label,
        scan: HandlerScan::default(),
    };
    collector.visit_file(file);
    collector.scan
}

/// Collects the route-declaring functions of one file, in source order.
struct Collector<'a> {
    label: &'a str,
    scan: HandlerScan,
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        if let Some(handler) = describe(item, self.label, &mut self.scan.findings) {
            self.scan.handlers.push(handler);
        }
        // Recurse, so a handler declared inside another function is still found.
        visit::visit_item_fn(self, item);
    }
}

/// Read the route attributes and `OpenAPI` annotation of one function.
///
/// Functions declaring neither a route nor an annotation are not part of the
/// contract — helpers, guards and conversions — and are left out of the result
/// rather than reported as missing an annotation.
fn describe(item: &ItemFn, label: &str, findings: &mut Vec<Finding>) -> Option<Handler> {
    let name = item.sig.ident.to_string();
    let mut method = None;
    let mut uri = None;
    let mut annotated = false;
    let mut spec_path = None;

    for attr in &item.attrs {
        if let Some(found) = route_method(attr) {
            method = Some(found);
            uri = route_uri(attr, label, &name, findings);
        } else if is_utoipa_path(attr) {
            annotated = true;
            spec_path = utoipa_spec_path(attr);
        }
    }

    if method.is_none() && !annotated {
        return None;
    }

    Some(Handler {
        name,
        line: line_of_ident(&item.sig.ident),
        method,
        uri,
        annotated,
        spec_path,
    })
}

/// The method a Rocket route attribute declares, or `None` for any other
/// attribute.
fn route_method(attr: &Attribute) -> Option<HttpMethod> {
    let name = attr.path().segments.last()?.ident.to_string();
    HttpMethod::from_name(&name)
}

/// The URI literal of a Rocket route attribute.
///
/// Rocket takes the URI as the attribute's first positional argument, so an
/// argument that is not a string literal is reported rather than guessed at.
fn route_uri(
    attr: &Attribute,
    label: &str,
    name: &str,
    findings: &mut Vec<Finding>,
) -> Option<String> {
    // Rocket takes the URI as the attribute's first positional argument, so an
    // argument that is not a string literal is reported rather than guessed at.
    let first = match &attr.meta {
        Meta::List(list) => list.tokens.clone().into_iter().next(),
        Meta::Path(_) | Meta::NameValue(_) => None,
    };
    let Some(token) = first else {
        findings.push(Finding::at(
            label,
            attr.span(),
            format!("{name}: route attribute declares no URI argument"),
        ));
        return None;
    };

    let span = token.span();
    if let Some(uri) = string_literal(token) {
        return Some(uri);
    }
    findings.push(Finding::at(
        label,
        span,
        format!("{name}: route URI is not a string literal"),
    ));
    None
}

/// Whether an attribute is `#[utoipa::path]`.
///
/// The annotation is always crate-qualified, and a bare `#[path = "..."]` is the
/// built-in module attribute — reading it as an `OpenAPI` operation would make
/// every `#[path = "…"] mod` look documented.
fn is_utoipa_path(attr: &Attribute) -> bool {
    let mut segments = attr.path().segments.iter();
    matches!(
        (segments.next(), segments.next(), segments.next()),
        (Some(utoipa), Some(path), None) if utoipa.ident == "utoipa" && path.ident == "path"
    )
}

/// The `path = "..."` literal of a `#[utoipa::path]` attribute.
///
/// Read from the attribute's own top-level tokens: `params(..)` and
/// `responses(..)` are nested token trees, so an identifier inside one cannot be
/// mistaken for the operation's path key. An annotation without the key yields
/// `None` rather than a finding — utoipa may derive the path itself, and whether
/// that is acceptable is a contract decision, not a parse error.
fn utoipa_spec_path(attr: &Attribute) -> Option<String> {
    let Meta::List(list) = &attr.meta else {
        return None;
    };

    let mut tokens = list.tokens.clone().into_iter().peekable();
    while let Some(token) = tokens.next() {
        let is_key = matches!(&token, TokenTree::Ident(ident) if ident == "path");
        let is_assigned =
            matches!(tokens.peek(), Some(TokenTree::Punct(punct)) if punct.as_char() == '=');
        if !is_key || !is_assigned {
            continue;
        }
        let _ = tokens.next();
        if let Some(value) = tokens.next().and_then(string_literal) {
            return Some(value);
        }
    }

    None
}

/// The value of a string-literal token, or `None` for any other token.
fn string_literal(token: TokenTree) -> Option<String> {
    syn::parse2::<LitStr>(TokenStream::from(token))
        .ok()
        .map(|literal| literal.value())
}

/// The 1-based line a definition name is on, or 1 when the input carried no
/// span.
fn line_of_ident(ident: &syn::Ident) -> usize {
    let line = ident.span().start().line;
    if line > 0 { line } else { 1 }
}
