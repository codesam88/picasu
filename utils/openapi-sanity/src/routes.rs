//! Handler references registered by Rocket `routes![...]` blocks.

use proc_macro2::{Spacing, TokenStream, TokenTree};
use syn::visit::Visit;

use crate::finding::Finding;

/// One handler reference from a `routes![...]` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerRef {
    /// Module path relative to the scanned group, e.g. `get_page` for
    /// `get_page::login`. An unqualified entry carries the group prefix.
    pub module_path: String,
    /// Bare handler name, e.g. `login`.
    pub handler: String,
}

/// Handler references and diagnostics from the `routes![...]` blocks in a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteScan {
    /// References in source order; `routes![a, b]` yields two of them.
    pub handlers: Vec<HandlerRef>,
    /// Entries that do not name a handler, plus syntax errors.
    pub findings: Vec<Finding>,
}

pub(crate) fn scan(file: &syn::File, label: &str, group_prefix: &str) -> RouteScan {
    let mut collector = Collector {
        label,
        group_prefix,
        scan: RouteScan::default(),
    };
    collector.visit_file(file);
    collector.scan
}

/// Collects the `routes!` invocations of one file, in source order.
struct Collector<'a> {
    label: &'a str,
    group_prefix: &'a str,
    scan: RouteScan,
}

impl<'ast> Visit<'ast> for Collector<'_> {
    /// Only `routes!` invocations are of interest, and a macro's token stream is
    /// opaque to the visitor, so not recursing into one loses nothing.
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        // The macro name is matched on its last segment, so a `rocket::routes!`
        // block registers routes exactly like a local one.
        if mac
            .path
            .segments
            .last()
            .is_some_and(|last| last.ident == "routes")
        {
            self.collect(mac);
        }
    }
}

impl Collector<'_> {
    fn collect(&mut self, mac: &syn::Macro) {
        // The block's tokens are read by value; the macro itself is borrowed and
        // may be visited again by a caller that walks a wider tree.
        for entry in split_entries(mac.tokens.clone()) {
            let line = entry.line;
            let rendered = entry.tokens.to_string();
            match parse_entry(entry.tokens, self.group_prefix) {
                Some(handler) => self.scan.handlers.push(handler),
                // A `routes![]` entry that is not a plain handler reference is
                // reported rather than guessed at: turning it into a
                // `__path_*` import would either break the build or, worse,
                // register a route under a name nobody declared.
                None => self.scan.findings.push(Finding::on_line(
                    self.label,
                    line,
                    format!("ignoring unparsable routes![] entry: `{rendered}`"),
                )),
            }
        }
    }
}

/// One comma-separated `routes![]` entry: its tokens and the line it starts on.
struct Entry {
    tokens: TokenStream,
    line: usize,
}

/// Split a `routes!` body on its top-level commas.
///
/// Commas inside a nested token tree — `(..)`, `[..]`, `{..}` — belong to that
/// tree, so an entry containing one cannot swallow the handlers after it. A
/// trailing comma and an empty block contribute no entry.
fn split_entries(tokens: TokenStream) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut entry = EntryBuilder::default();

    for token in tokens {
        if is_separator(&token) {
            if let Some(finished) = entry.take() {
                entries.push(finished);
            }
        } else {
            entry.push(token);
        }
    }
    if let Some(finished) = entry.take() {
        entries.push(finished);
    }

    entries
}

/// A top-level comma. A joint one (`,,`) belongs to its neighbour instead.
fn is_separator(token: &TokenTree) -> bool {
    matches!(token, TokenTree::Punct(punct)
        if punct.as_char() == ',' && punct.spacing() == Spacing::Alone)
}

/// Accumulates the tokens of one entry and the line its first token is on.
#[derive(Default)]
struct EntryBuilder {
    tokens: TokenStream,
    line: Option<usize>,
}

impl EntryBuilder {
    fn push(&mut self, token: TokenTree) {
        if self.line.is_none() {
            self.line = line_of(&token);
        }
        self.tokens.extend(Some(token));
    }

    /// Finish the entry, or `None` when no token was collected.
    fn take(&mut self) -> Option<Entry> {
        let line = self.line?;
        let tokens = std::mem::take(&mut self.tokens);
        self.line = None;
        Some(Entry { tokens, line })
    }
}

/// The 1-based line a token starts on. A group's span covers its delimiters, so
/// an entry starting with one is reported on the line of the opening delimiter.
fn line_of(token: &TokenTree) -> Option<usize> {
    let line = match token {
        TokenTree::Group(group) => group.span_open(),
        other => other.span(),
    }
    .start()
    .line;
    (line > 0).then_some(line)
}

/// Resolve one `routes![]` entry to a handler reference.
///
/// Every `::`-separated segment must be a plain identifier, which rejects macro
/// calls, literals, indexing and arithmetic. Unqualified entries resolve against
/// `group_prefix`, so `routes![delete_data]` scanned for the `delete` group
/// becomes `delete::delete_data`.
fn parse_entry(tokens: TokenStream, group_prefix: &str) -> Option<HandlerRef> {
    let mut segments: Vec<String> = Vec::new();
    let mut tokens = tokens.into_iter();

    loop {
        match tokens.next() {
            // A raw identifier (`r#type`) is not a handler name: the `__path_*`
            // item it would have to name does not exist.
            Some(TokenTree::Ident(ident)) if !ident.to_string().starts_with("r#") => {
                segments.push(ident.to_string());
            }
            _ => return None,
        }
        match tokens.next() {
            None => break,
            // `::` is a joint colon followed by a standalone one. Both checks
            // matter: they reject `a: b` and `a:::b`, which the text form of this
            // parser also refused.
            Some(TokenTree::Punct(first))
                if first.as_char() == ':' && first.spacing() == Spacing::Joint => {}
            _ => return None,
        }
        match tokens.next() {
            Some(TokenTree::Punct(second))
                if second.as_char() == ':' && second.spacing() == Spacing::Alone => {}
            _ => return None,
        }
    }

    let handler = segments.pop()?;
    let module_path = if segments.is_empty() {
        group_prefix.to_string()
    } else {
        segments.join("::")
    };

    Some(HandlerRef {
        module_path,
        handler,
    })
}
