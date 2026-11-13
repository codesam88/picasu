//! No-op attribute macros used to tag backend functions with the operation
//! and event identifiers tracked by `utils/quint/operations-check.mjs`.
//!
//! Both macros return the annotated item unchanged. They exist so the
//! implementation-tracking layer can be scanned without affecting
//! compilation, OpenAPI generation or Rocket route registration.

extern crate proc_macro;

use proc_macro::TokenStream;

/// Tag a function with a tracked operation id (e.g. `#[quint_annotations::operation("OP-3")]`).
#[proc_macro_attribute]
pub fn operation(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Tag a function with a tracked event name (e.g. `#[quint_annotations::event("API_DELETE")]`).
#[proc_macro_attribute]
pub fn event(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
