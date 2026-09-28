//! Rocket route attributes, per-function `#[utoipa::path]` annotations and the
//! parameters a handler binds its request by.

use proc_macro2::{TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Attribute, FnArg, ItemFn, LitStr, Meta, Pat, Type};

use crate::finding::Finding;
use crate::guards::{self, Discard, Enforcement, GuardBinding};

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
    /// The verb of that annotation, when it declares one. utoipa accepts the
    /// verb anywhere among the annotation's top-level tokens, and it is what
    /// decides which operation the annotation is registered under.
    pub spec_method: Option<HttpMethod>,
    /// The request guards the parameter list declares, in signature order. A
    /// handler that declares none is reachable without any guard.
    pub guards: Vec<GuardBinding>,
    /// Every parameter the signature binds, in signature order.
    ///
    /// The guards above say what a parameter *enforces*; this says what Rocket
    /// hands it, which is the question the parameter rules ask: a request body
    /// reaches the handler through one named argument, and a path or query
    /// parameter through an argument of the same name as the segment that declares
    /// it.
    pub args: Vec<HandlerArg>,
}

/// What Rocket hands one handler argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArgKind {
    /// A request guard. Rocket runs it before the body, and the argument says
    /// nothing about the request's path, query or body.
    Guard,
    /// The argument the route attribute's `data = "<name>"` binds the request
    /// body to. A handler without one is a handler without a body, whatever its
    /// other parameters are.
    Data,
    /// Every other argument, which Rocket binds by the name it declares: a path
    /// segment, a query parameter or a form field.
    Plain,
}

/// One parameter of a handler's signature.
///
/// The name is the whole of the binding: Rocket matches a `<segment>` and a
/// `?<name>` against an argument of that name, so an argument and the route
/// declaration it answers are two spellings of one fact and are compared as such.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerArg {
    /// The name the handler binds the argument to.
    pub name: String,
    /// What Rocket hands the argument.
    pub kind: ArgKind,
    /// 1-based line of the parameter in the signature, so a finding about one
    /// points at the parameter rather than at the function.
    pub line: usize,
    /// Whether the declared type is an `Option`, which is the only thing that
    /// makes a query parameter optional: Rocket binds a missing `?<name>` to
    /// `None` and refuses the request outright without one.
    pub optional: bool,
    /// The type the request body is known by, after unwrapping the `Result`,
    /// `Option` and `Json`/`Form` wrappers down to the type the developer named.
    /// `None` for a guard — a guard carries no body — and for a type this
    /// analyzer cannot name, such as a reference to a slice.
    pub body_type: Option<String>,
    /// Whether the argument is a `Form<..>` wrapper, which Rocket fills from a
    /// form-encoded or multipart request rather than from a JSON body.
    pub multipart: bool,
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
/// rather than reported as missing an annotation. Their parameters are still
/// read, so a discarded guard is reported against the handler that owns it rather
/// than against whatever route the helper feeds.
fn describe(item: &ItemFn, label: &str, findings: &mut Vec<Finding>) -> Option<Handler> {
    let name = item.sig.ident.to_string();
    let mut method = None;
    let mut uri = None;
    let mut data = None;
    let mut annotated = false;
    let mut spec_path = None;
    let mut spec_method = None;

    for attr in &item.attrs {
        if let Some(found) = route_method(attr) {
            method = Some(found);
            uri = route_uri(attr, label, &name, findings);
            data = route_data_binding(attr);
        } else if is_utoipa_path(attr) {
            annotated = true;
            spec_path = utoipa_spec_path(attr);
            spec_method = utoipa_spec_method(attr);
        }
    }

    let guards = guards::bindings(item);
    report_discarded_guards(label, &name, &guards, findings);

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
        spec_method,
        guards,
        args: arguments(item, data.as_deref()),
    })
}

/// Every parameter of a handler's signature, in signature order.
///
/// A receiver and a parameter that is not a plain name bind nothing a rule can
/// compare, so they are left out: `self` is not a request, and a destructured
/// pattern has no name for a route to match against.
fn arguments(item: &ItemFn, data: Option<&str>) -> Vec<HandlerArg> {
    item.sig
        .inputs
        .iter()
        .filter_map(|input| argument(input, data))
        .collect()
}

/// One parameter, with what Rocket hands it and the type its body is known by.
fn argument(input: &FnArg, data: Option<&str>) -> Option<HandlerArg> {
    let FnArg::Typed(typed) = input else {
        return None;
    };
    let Pat::Ident(name) = typed.pat.as_ref() else {
        return None;
    };

    let line = line_of_ident(&name.ident);
    let name = name.ident.to_string();
    let body = body_type(&typed.ty);
    let kind = if guards::is_guard(&typed.ty) {
        ArgKind::Guard
    } else if data == Some(name.as_str()) {
        ArgKind::Data
    } else {
        ArgKind::Plain
    };

    Some(HandlerArg {
        line,
        name,
        kind,
        optional: body.optional,
        // A guard is not a body, and naming its type here would read as a claim
        // about one.
        body_type: if kind == ArgKind::Guard {
            None
        } else {
            body.name
        },
        multipart: body.multipart,
    })
}

/// The type a request-body argument is known by, after its wrappers.
///
/// `Result<Json<T>, Errors>` and `Option<Json<T>>` both bind a body of `T`, and
/// Rocket's form wrapper is declared as `Result<Form<T>, Errors>` for the same
/// reason. The wrappers are unwrapped in the order the developer wrote them, so
/// only the innermost named type is what the document is compared against.
struct BodyType {
    /// Whether an `Option` was among the wrappers unwrapped.
    optional: bool,
    /// Whether a `Form<..>` was among the wrappers unwrapped.
    multipart: bool,
    /// The name of the innermost type, when it has one.
    name: Option<String>,
}

impl BodyType {
    /// The shape of a type that names nothing: a receiver, a reference to a
    /// primitive, a slice or a tuple.
    const fn none() -> Self {
        Self {
            optional: false,
            multipart: false,
            name: None,
        }
    }
}

/// Unwrap a body argument's type down to the type it is known by.
///
/// The wrappers are the ones this codebase's handlers use, and each is unwrapped
/// only in the position it can appear in: `Result`'s first type argument, because
/// its second is the error. `serde_json::Value` is named `Value` whatever path
/// qualifies it, so a handler taking arbitrary JSON and an annotation declaring
/// `request_body = Value` read as the same claim.
fn body_type(ty: &Type) -> BodyType {
    let mut found = BodyType::none();
    let mut current = guards::unwrap_reference(ty);
    while let Type::Path(path) = current {
        let Some(segment) = path.path.segments.last() else {
            break;
        };
        let name = segment.ident.to_string();
        let inner = first_type_argument(segment);
        match name.as_str() {
            "Result" | "GuardResult" | "Json" => {}
            "Option" => found.optional = true,
            "Form" => found.multipart = true,
            _ => {
                found.name = Some(if name == "Value" {
                    "Value".to_string()
                } else {
                    name
                });
                return found;
            }
        }
        let Some(inner) = inner else { break };
        current = inner;
    }
    found
}

/// The first type argument of a path segment, which is the wrapped type for
/// every wrapper this analyzer unwraps.
///
/// A lifetime argument — `Form<UploadForm<'_>>` — is not a type and is skipped,
/// so the wrapper's own type argument is what comes out.
fn first_type_argument(segment: &syn::PathSegment) -> Option<&Type> {
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    arguments.args.iter().find_map(|argument| match argument {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
}

/// A deferred guard binding the handler body does not act on.
///
/// A local defect rather than a policy one: the binding is there, the guard
/// exists, and the handler drops its failure, so the request proceeds as though
/// it had been authorized. Reported from the scan rather than from the auth
/// policy because it holds for any handler, listed in the route table or not.
fn report_discarded_guards(
    label: &str,
    name: &str,
    guards: &[GuardBinding],
    findings: &mut Vec<Finding>,
) {
    for binding in guards {
        let Enforcement::Discarded(discard) = binding.enforcement else {
            continue;
        };
        let cause = match discard {
            Discard::Dropped => format!("`let _ = {};` drops its failure", binding.parameter),
            Discard::Unread => format!("`{}` is never read", binding.parameter),
        };
        findings.push(Finding::on_line(
            label,
            binding.line,
            format!(
                "{name}: the deferred guard {} is bound to `{}` and never enforced — \
                 {cause}; propagate it with `?`, return it, or match on it",
                binding.class.guard_name(),
                binding.parameter
            ),
        ));
    }
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

/// The `data = "<name>"` binding of a Rocket route attribute.
///
/// Rocket names the request-body argument in the route attribute rather than in
/// the signature, so this is the only place the two are joined. A route that
/// declares no `data` key takes no body, and every argument it binds is a guard
/// or a path/query parameter.
fn route_data_binding(attr: &Attribute) -> Option<String> {
    string_key(attr, "data").map(|binding| {
        binding
            .strip_prefix('<')
            .and_then(|rest| rest.strip_suffix('>'))
            .unwrap_or(&binding)
            .to_string()
    })
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
/// An annotation without the key yields `None` rather than a finding — utoipa may
/// derive the path itself, and whether that is acceptable is a contract decision,
/// not a parse error.
fn utoipa_spec_path(attr: &Attribute) -> Option<String> {
    string_key(attr, "path")
}

/// The value of a `key = "..."` entry among an attribute's top-level tokens.
///
/// Read from the attribute's own tokens: `params(..)` and `responses(..)` are
/// nested token groups, so a `path` or a `data` inside one names nothing at this
/// level. A key that is not assigned a string literal is skipped rather than
/// guessed at, which leaves the caller's `None` to mean "declares no such key".
fn string_key(attr: &Attribute, key: &str) -> Option<String> {
    let Meta::List(list) = &attr.meta else {
        return None;
    };

    let mut tokens = list.tokens.clone().into_iter().peekable();
    while let Some(token) = tokens.next() {
        let is_key = matches!(&token, TokenTree::Ident(ident) if ident == key);
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

/// The verb of a `#[utoipa::path]` attribute, read from its own top-level
/// tokens.
///
/// utoipa takes the verb as a bare identifier that may sit anywhere in the
/// attribute, and the first identifier that names a known verb is the one it
/// registers the operation under. An identifier that names no verb is skipped
/// rather than ending the search, so `#[utoipa::path(params(..), get, ..)]`
/// still yields `Get`.
fn utoipa_spec_method(attr: &Attribute) -> Option<HttpMethod> {
    let Meta::List(list) = &attr.meta else {
        return None;
    };
    list.tokens
        .clone()
        .into_iter()
        .find_map(|token| match token {
            TokenTree::Ident(ident) => HttpMethod::from_name(&ident.to_string()),
            _ => None,
        })
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
