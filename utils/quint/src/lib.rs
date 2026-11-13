//! Proc-macro crate that validates operation and event marker ids at
//! expansion time.
//!
//! `#[quint::operation("OP-N")]` checks `OP-N` against the `id` field of
//! `pipeline/operations.json`; `#[quint::event("NAME")]` checks `NAME`
//! against the `name` field of `pipeline/events.json`. An unknown id
//! emits `compile_error!`. The macros otherwise return the annotated item
//! unchanged.
//!
//! What this does not enforce (rung 1+): that the marker sits on a real
//! item rather than a comment/string mention, that a valid-but-wrong id
//! maps to the right function, or aspect/order drift between model and code.
//! `utils/quint/operations-check.mjs` stays as the catalog-side backstop.

extern crate proc_macro;

use proc_macro::TokenStream;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

static OPS: OnceLock<HashSet<String>> = OnceLock::new();
static EVENTS: OnceLock<HashSet<String>> = OnceLock::new();

fn catalog_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("pipeline")
        .join(file)
}

fn load_ids(file: &str, array_key: &str, id_field: &str) -> HashSet<String> {
    let path = catalog_path(file);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("quint: cannot read {}: {}", path.display(), e));
    let value: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("quint: cannot parse {}: {}", path.display(), e));
    let mut set = HashSet::new();
    if let Some(arr) = value.get(array_key).and_then(|v| v.as_array()) {
        for item in arr {
            if let Some(id) = item.get(id_field).and_then(|v| v.as_str()) {
                set.insert(id.to_string());
            }
        }
    }
    set
}

fn ops_ids() -> &'static HashSet<String> {
    OPS.get_or_init(|| load_ids("operations.json", "operations", "id"))
}

fn events_ids() -> &'static HashSet<String> {
    EVENTS.get_or_init(|| load_ids("events.json", "events", "name"))
}

fn parse_id(attr: &TokenStream) -> String {
    let s = attr.to_string();
    let t = s.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

fn validate(attr: TokenStream, item: TokenStream, is_op: bool) -> TokenStream {
    let ids = if is_op { ops_ids() } else { events_ids() };
    let catalog = if is_op {
        "pipeline/operations.json"
    } else {
        "pipeline/events.json"
    };
    let id = parse_id(&attr);
    if !ids.contains(&id) {
        let msg = format!("unknown id `{}` (not found in {})", id, catalog);
        let escaped = msg.replace('\\', "\\\\").replace('"', "\\\"");
        let err_src = format!("compile_error!(\"{}\");", escaped);
        let mut out: TokenStream = err_src.parse().unwrap_or_else(|_| TokenStream::new());
        out.extend(item);
        return out;
    }
    item
}

#[proc_macro_attribute]
pub fn operation(attr: TokenStream, item: TokenStream) -> TokenStream {
    validate(attr, item, true)
}

#[proc_macro_attribute]
pub fn event(attr: TokenStream, item: TokenStream) -> TokenStream {
    validate(attr, item, false)
}
