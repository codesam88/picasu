//! The Rocket-to-OpenAPI path translation, owned by the backend.
//!
//! A Rocket URI and an `OpenAPI` path template name the same value three ways:
//!
//! | spelling | shape | who needs it |
//! | --- | --- | --- |
//! | binding name | `path`, `asset_id` | the handler's own parameter list — Rocket binds `<path..>` to a parameter called `path`, and requires the two names to match |
//! | template name | `path`, `asset_id` | a documented path and the `<segment>` names a route serves |
//! | path template | `/albums/view/{path}` | the committed document |
//!
//! [`to_spec_path`] turns the first spelling into the third. The consumers that
//! need it are the backend's own: the mounted-route parity tests and the
//! `--check-openapi` route-set check, which both compare a runtime route table
//! against a generated document and therefore have to agree on one translation.
//! Nothing outside the backend translates a URI, so this crate is the
//! translation's single home.

/// Rewrite a Rocket route URI to `OpenAPI` path-template form.
///
/// Rocket declares segments as `<name>` / `<name..>`; `OpenAPI` uses `{name}`.
/// The segment name is carried through verbatim: utoipa derives the documented
/// parameter from the same route string, so rewriting it here would make the two
/// sides disagree. The query part is dropped because it is documented per
/// parameter, not in the path.
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

/// The path part of a URI: everything before the query separator.
///
/// Rocket keeps the query in the same string as the path, and the two are
/// documented in different places, so every reader of a URI splits it here.
fn path_of(uri: &str) -> &str {
    uri.split('?').next().unwrap_or(uri)
}

/// One `<segment>` declaration and the template spelling of its name.
struct Segment<'a> {
    /// Literal path text before this declaration.
    prefix: &'a str,
    /// The name a path template spells the bound parameter as.
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
/// The template spelling [`to_spec_path`] writes comes from this walk, so a
/// change to what counts as a declaration cannot reach the translation without
/// reaching the walk itself.
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
            template: binding,
        });
        rest = &after[end + 1..];
    }
    Scan {
        segments,
        trailing: rest,
    }
}
