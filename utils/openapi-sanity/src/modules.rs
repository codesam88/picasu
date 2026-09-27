//! Which router files make up the documented contract, and how a `routes![]`
//! entry finds the file that defines the handler it names.
//!
//! A handler is part of the contract only if a `routes![...]` block registers it
//! *and* its own function carries a `#[utoipa::path]` annotation. Which files
//! declare those route tables is a property of the application, not of this
//! crate, but three consumers need the same answer — the build script that
//! generates `openapi.rs`, the contract gate, and the crate's own tests — so the
//! list is declared once here instead of once per consumer.

/// The router modules whose `routes![...]` blocks are part of the documented
/// contract, as `(group prefix, path relative to the router root)`.
///
/// `router/auth.rs` is in the list because it mounts the token renewal routes
/// through `generate_fairing_routes()`; without it its annotated handlers never
/// reach `paths(...)` and the routes are mounted but undocumented. A module in
/// this list whose file no longer exists is dead configuration, so a consumer
/// that resolves the list against a real tree reports it instead of quietly
/// scanning one module fewer.
pub const SCANNED_MODULES: &[(&str, &str)] = &[
    ("get", "get/mod.rs"),
    ("post", "post/mod.rs"),
    ("put", "put/mod.rs"),
    ("delete", "delete.rs"),
    ("auth", "auth.rs"),
];

/// One loaded router source file, as the contract checks consume it.
///
/// A `routes![...]` block and the functions it registers usually live in
/// different files — `get/mod.rs` registers `get_page::login`, which is defined
/// in `get/get_page.rs` — so the two facts are carried separately: the group a
/// file belongs to, and the module its own functions form. The pair is what a
/// `routes![...]` entry has to match to name a handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceUnit<'a> {
    label: &'a str,
    group_prefix: &'a str,
    module_path: &'a str,
    source: &'a str,
}

impl<'a> SourceUnit<'a> {
    /// A unit for a router-relative file, with the group and module taken from
    /// its path rather than from the caller, so a unit cannot disagree with the
    /// layout it claims to describe.
    ///
    /// `get/mod.rs` is the route table of group `get` and defines the group's
    /// own module, `get/get_page.rs` defines module `get_page` of the same
    /// group, and a group-root file such as `delete.rs` defines the group itself
    /// — which is what makes its unqualified `routes![delete_data]` entries
    /// resolve to it.
    #[must_use]
    pub fn for_relative_path(label: &'a str, relative_path: &'a str, source: &'a str) -> Self {
        let (group_prefix, module_path) = group_and_module(relative_path);
        Self {
            label,
            group_prefix,
            module_path,
            source,
        }
    }

    /// Diagnostic label of the file. Never a path this crate has opened: it is
    /// copied into whatever findings the checks produce.
    #[must_use]
    pub fn label(&self) -> &'a str {
        self.label
    }

    /// Group the file belongs to, which is the `group_prefix` unqualified
    /// `routes![...]` entries resolve against.
    #[must_use]
    pub fn group_prefix(&self) -> &'a str {
        self.group_prefix
    }

    /// Module the file's own functions form, as a `routes![...]` entry would
    /// qualify them.
    #[must_use]
    pub fn module_path(&self) -> &'a str {
        self.module_path
    }

    /// File contents, parsed as a complete file.
    #[must_use]
    pub fn source(&self) -> &'a str {
        self.source
    }
}

/// The group and module a router-relative path defines.
fn group_and_module(relative_path: &str) -> (&str, &str) {
    let path = relative_path.strip_suffix(".rs").unwrap_or(relative_path);
    match path.split_once('/') {
        // `get/mod.rs` is the group's route table; the module it declares is
        // the group itself, named by the unqualified entries it registers.
        Some((group, "mod" | "")) => (group, group),
        // `delete.rs` is a group-root file: it defines the group's own module.
        None => (path, path),
        Some((group, module)) => (group, module),
    }
}

/// The router-relative file defining the handler a `routes![...]` entry names.
///
/// A `routes![...]` entry qualifies a handler by module, and a module is a
/// file: the same mapping the build script uses to emit `__path_*` imports has
/// to be the one a contract check resolves, or the check reports handlers the
/// build script never registered.
#[must_use]
pub fn handler_module_path(group_prefix: &str, module_path: &str) -> String {
    if module_path == group_prefix {
        format!("{group_prefix}.rs")
    } else {
        format!("{group_prefix}/{module_path}.rs")
    }
}
