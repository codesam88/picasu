//! The one Rocket-to-OpenAPI path translation in the repository.
//!
//! A Rocket URI and an `OpenAPI` path template name the same value three ways, and
//! every consumer of this module needs a different one:
//!
//! | spelling | shape | who needs it |
//! | --- | --- | --- |
//! | binding name | `_path`, `asset_id` | the handler's own parameter list — Rocket binds `<_path..>` to a parameter called `_path` |
//! | template name | `path`, `asset_id` | a documented path and the `<segment>` names a route serves |
//! | path template | `/albums/view/{path}` | the committed document |
//!
//! All three are read from one scan of the URI, so a route's segments, the
//! document's placeholders and the handler's arguments cannot disagree about a
//! name the analyzer only half understands. What is *not* here is a rule: this
//! module translates, and [`crate::check_params`] decides whether two spellings
//! of the same name ought to be the same value.

/// Rewrite a Rocket route URI to `OpenAPI` path-template form.
///
/// Rocket declares segments as `<name>` / `<name..>`; `OpenAPI` uses `{name}`.
/// A
/// leading underscore in a Rocket segment name (`<_path..>`, used to avoid a
/// clash with the handler name) has no `OpenAPI` counterpart and is dropped.
/// The
/// query part is dropped because it is documented per parameter, not in the
/// path.
///
/// Mounted-route parity compares a runtime route table against a generated
/// document, so both sides have to agree on this translation. It lives here so
/// that a second, drifting copy cannot be introduced by a new consumer.
#[must_use]
pub fn to_spec_path(uri: &str) -> String {
    let path = path_of(uri);
    let scan = scan(path);
    let mut out = String::with_capacity(path.len());
    for segment in &scan.segments {
        out.push_str(segment.prefix);
        out.push('{');
        out.push_str(segment.template);
        out.push('}');
    }
    out.push_str(scan.trailing);
    out
}

/// The names a Rocket URI's `<segment>` declarations bind parameters by, in the
/// order they appear.
///
/// The raw spelling, not the template one: Rocket matches `<_path..>` against a
/// handler parameter called `_path`, and the underscore the developer added to
/// dodge a name clash is part of that name. [`route_segments`] is the same list
/// as a documented path spells it.
#[must_use]
pub fn route_bindings(uri: &str) -> Vec<String> {
    scan(path_of(uri))
        .segments
        .iter()
        .map(|segment| segment.binding.to_string())
        .collect()
}

/// The names a Rocket URI's `<segment>` declarations have once
/// [`to_spec_path`] has rewritten them, in the order they appear.
///
/// What a path template's `{name}` placeholders ought to be, read off the route
/// rather than off the document, so the two halves of the comparison cannot
/// disagree about which side was translated.
#[must_use]
pub fn route_segments(uri: &str) -> Vec<String> {
    scan(path_of(uri))
        .segments
        .iter()
        .map(|segment| segment.template.to_string())
        .collect()
}

/// The names a Rocket URI's query part declares, in the order they appear.
///
/// `?<timestamp>&<start>` binds two parameters, and they are the only place a
/// query parameter is declared: the path says nothing about them, and the
/// document has to declare each one again. Nothing is normalized here beyond the
/// zero-or-more dots, because a query parameter has no template spelling.
#[must_use]
pub fn route_query_bindings(uri: &str) -> Vec<String> {
    let Some((_, query)) = uri.split_once('?') else {
        return Vec::new();
    };
    query
        .split('&')
        .flat_map(|part| {
            scan(part)
                .segments
                .iter()
                .map(|segment| segment.binding.to_string())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The `{name}` placeholders a documented path template declares, in the order
/// they appear.
///
/// The document-side counterpart of [`route_segments`]. A template is not a
/// Rocket URI and is not run through [`to_spec_path`]: reading `{name}` out of
/// the committed document is what tells a placeholder the route does not serve
/// from one it does.
#[must_use]
pub fn spec_placeholders(path: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        rest = &rest[start + 1..];
        // An unterminated placeholder is not a declaration; the remainder has no
        // name to report, so the scan stops rather than inventing one.
        let Some(end) = rest.find('}') else {
            break;
        };
        names.push(rest[..end].to_string());
        rest = &rest[end + 1..];
    }
    names
}

/// The path part of a URI: everything before the query separator.
///
/// Rocket keeps the query in the same string as the path, and the two are
/// documented in different places, so every reader of a URI splits it here.
fn path_of(uri: &str) -> &str {
    uri.split('?').next().unwrap_or(uri)
}

/// One `<segment>` declaration and the two spellings of its name.
struct Segment<'a> {
    /// Literal path text before this declaration.
    prefix: &'a str,
    /// The name Rocket binds a parameter by.
    binding: &'a str,
    /// The name a path template spells the same value as.
    template: &'a str,
}

/// The declarations of one URI part, plus the text following the last complete
/// one.
struct Scan<'a> {
    segments: Vec<Segment<'a>>,
    /// The text after the last complete `<...>` declaration, which is the whole
    /// input when there is none. A malformed URI with an unterminated `<` is
    /// passed through verbatim rather than silently dropped, so a malformed URI
    /// stays visible in the comparison.
    trailing: &'a str,
}

/// Read every complete `<segment>` declaration of a URI part.
///
/// Shared by the three readers above: a route's binding names, its template names
/// and the path [`to_spec_path`] builds all come from this one walk, so a change
/// to what counts as a declaration cannot reach one of them alone.
fn scan(path: &str) -> Scan<'_> {
    let mut segments = Vec::new();
    let mut rest = path;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else {
            // The whole of the remainder, literal prefix before the `<` included:
            // a malformed URI is passed through verbatim rather than dropped up to
            // the point it broke.
            return Scan {
                segments,
                trailing: rest,
            };
        };
        let binding = after[..end].trim_end_matches('.');
        segments.push(Segment {
            prefix: &rest[..start],
            binding,
            template: binding.trim_start_matches('_'),
        });
        rest = &after[end + 1..];
    }
    Scan {
        segments,
        trailing: rest,
    }
}
