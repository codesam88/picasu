//! The one Rocket-to-OpenAPI path translation in the repository.

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
    let path = uri.split('?').next().unwrap_or(uri);
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else {
            // Not a segment declaration; keep the remainder verbatim.
            out.push_str(&rest[start..]);
            return out;
        };
        out.push('{');
        out.push_str(after[..end].trim_end_matches('.').trim_start_matches('_'));
        out.push('}');
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}
