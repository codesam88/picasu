//! Reading the names of a Rocket URI's parameters and of an `OpenAPI` path
//! template.
//!
//! A Rocket URI and an `OpenAPI` path template name the same value three ways, and
//! every consumer of this module needs one of them:
//!
//! | spelling | shape | who needs it |
//! | --- | --- | --- |
//! | binding name | `_path`, `asset_id` | the handler's own parameter list — Rocket binds `<_path..>` to a parameter called `_path` |
//! | template name | `path`, `asset_id` | a documented path and the `<segment>` names a route serves |
//! | path template | `/albums/view/{path}` | the committed document |
//!
//! The readers below cover the first two spellings off one scan of the URI, so a
//! route's segments, the document's placeholders and the handler's arguments
//! cannot disagree about a name the analyzer only half understands. What is
//! *not* here is a rule or the translation between the spellings: this module
//! reads names, [`crate::check_params`] decides whether two spellings of the
//! same name ought to be the same value, and the Rocket-to-OpenAPI path
//! translation lives in the backend, whose comparisons need it.

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
        .iter()
        .map(|segment| segment.binding.to_string())
        .collect()
}

/// The names a Rocket URI's `<segment>` declarations spell as template names,
/// in the order they appear.
///
/// What a path template's `{name}` placeholders ought to be, read off the route
/// rather than off the document, so the two halves of the comparison cannot
/// disagree about which side was translated.
#[must_use]
pub fn route_segments(uri: &str) -> Vec<String> {
    scan(path_of(uri))
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
/// Rocket URI: reading `{name}` out of the committed document is what tells a
/// placeholder the route does not serve from one it does.
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
    /// The name Rocket binds a parameter by.
    binding: &'a str,
    /// The name a path template spells the same value as.
    template: &'a str,
}

/// Read every complete `<segment>` declaration of a URI part, in order.
///
/// Shared by the three readers above: a route's binding names and its template
/// names all come from this one walk, so a change to what counts as a
/// declaration cannot reach one of them alone.
fn scan(path: &str) -> Vec<Segment<'_>> {
    let mut segments = Vec::new();
    let mut rest = path;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else {
            // A malformed URI with an unterminated `<` declares nothing further:
            // the complete declarations found so far are kept rather than the
            // remainder being guessed at.
            break;
        };
        let binding = after[..end].trim_end_matches('.');
        segments.push(Segment {
            binding,
            template: binding.trim_start_matches('_'),
        });
        rest = &after[end + 1..];
    }
    segments
}
