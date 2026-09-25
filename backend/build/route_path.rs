//! Canonical translation of a Rocket route URI into an `OpenAPI` path
//! template — the single implementation shared by the mounted-route parity
//! gate (`src/tests/openapi_contract.rs`) and the build-time annotation
//! check (`build.rs`).
//!
//! The two forms differ because they come from different specifications:
//! Rocket declares route parameters inline in the URI as `<name>` or
//! `<name..>` (a multi-segment range), and allows an underscore-prefixed
//! binding name (`<_path..>`, used to avoid clashing with the handler
//! argument), with optional query parameters after `?`. An `OpenAPI` path
//! template spells parameters as `{name}`, has no range form and no binding
//! names, and documents query parameters per parameter rather than in the
//! path.
//!
//! The parity gate and the build-time annotation check compare the same two
//! strings — a Rocket route URI and the `path = "..."` of an utoipa
//! annotation or the committed spec — so they must share this translation:
//! two independent implementations would eventually disagree, and one gate
//! would then accept exactly what the other rejects.

/// Rewrite a Rocket route URI to `OpenAPI` path-template form.
///
/// * Everything from the first `?` on is dropped; query parameters are
///   documented per parameter, not in the path.
/// * `<name>` and `<name..>` become `{name}`; the range marker has no
///   `OpenAPI` counterpart.
/// * A leading underscore in the parameter name is stripped, because it only
///   exists to keep the Rust binding from clashing with the handler name.
/// * An unterminated `<` leaves the remainder verbatim, so a malformed URI
///   comes back recognisable instead of panicking.
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
