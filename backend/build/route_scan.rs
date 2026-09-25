//! Extraction of handler names from `routes![...]` blocks.
//!
//! Shared by `build.rs` and its unit tests (see `src/tests/route_scan.rs`) so the
//! scanner that decides which handlers reach `paths(...)` is covered by
//! `cargo test`. A parsing mistake here is silent: the build emits a coverage
//! warning at most, and the affected routes simply vanish from the spec while
//! staying mounted. That is exactly how `POST /post/renew-hash-token` and
//! `POST /post/renew-timestamp-token` went undocumented — the single-line
//! `routes![a, b]` form was parsed line by line, yielding one handler named
//! `a, b`.

/// One handler reference from a `routes![...]` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerRef {
    /// Module path relative to `router/`, e.g. `get_page` for `get_page::login`.
    pub module_path: String,
    /// Bare handler name, e.g. `login`.
    pub handler: String,
}

/// Extract every handler referenced by a `routes![...]` block in `content`.
///
/// Entries are comma-separated, not line-separated, so a single-line
/// `routes![a, b]` and its multi-line form yield the same handlers. Entries are
/// resolved against `group_prefix` when unqualified. Malformed entries (anything
/// that is not `ident` or `path::ident`) are skipped, so a non-literal argument
/// cannot be mistaken for a handler and silently register a bogus `__path_*`.
pub fn scan_routes(content: &str, group_prefix: &str) -> Vec<HandlerRef> {
    let mut handlers = Vec::new();
    let mut search_start = 0usize;

    while let Some(routes_start) = content[search_start..].find("routes![") {
        let body_start = search_start + routes_start + "routes![".len();
        let rest = &content[body_start..];
        let Some(end) = find_matching_bracket(rest) else {
            break;
        };

        for entry in strip_comments(&rest[..end]).split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            match parse_entry(entry, group_prefix) {
                Some(handler) => handlers.push(handler),
                // A `routes![]` entry that is not a plain handler reference is
                // reported rather than guessed at.
                None => println!("cargo:warning=ignoring unparsable routes![] entry: {entry}"),
            }
        }
        search_start = body_start + end + 1;
    }

    handlers
}

fn strip_comments(block: &str) -> String {
    block
        .lines()
        .map(|line| match line.find("//") {
            Some(pos) => &line[..pos],
            None => line,
        })
        .collect::<Vec<&str>>()
        .join("\n")
}

fn parse_entry(entry: &str, group_prefix: &str) -> Option<HandlerRef> {
    // Every `::`-separated segment must be a plain identifier. This rejects
    // macro invocations, literals, indexing and arithmetic — an entry that
    // cannot name a handler must not be turned into a `__path_*` import.
    if !entry.split("::").all(is_identifier) {
        return None;
    }
    match entry.rfind("::") {
        Some(pos) => Some(HandlerRef {
            module_path: entry[..pos].to_string(),
            handler: entry[pos + 2..].to_string(),
        }),
        None => Some(HandlerRef {
            module_path: group_prefix.to_string(),
            handler: entry.to_string(),
        }),
    }
}

fn is_identifier(segment: &str) -> bool {
    let mut chars = segment.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Byte offset of the `]` closing a `routes![` block body, skipping nested
/// brackets.
fn find_matching_bracket(s: &str) -> Option<usize> {
    let mut depth = 0u32;
    for (i, ch) in s.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}
