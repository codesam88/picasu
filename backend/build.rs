use std::collections::HashMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};

// Shared with the unit tests in `src/tests/ast_scan.rs`: the analysis decides
// which handlers reach `paths(...)`, and a parsing mistake there silently
// drops routes from the spec. The module is pure — it returns findings and
// this script prints them as `cargo:warning=` lines.
#[path = "build/ast_scan.rs"]
mod ast_scan;

// The single Rocket→OpenAPI path translation, also shared with the
// mounted-route parity gate in `src/tests/openapi_contract.rs`: both compare
// the same two strings, so they must use the same implementation. `ast_scan`
// calls it as `super::route_path::to_spec_path`, which is why this
// declaration must exist next to the `ast_scan` module.
#[path = "build/route_path.rs"]
mod route_path;

use ast_scan::{HandlersScan, scan_handlers, scan_routes};

#[derive(Clone)]
struct Route {
    group_prefix: String,
    module_path: String,
    handler: String,
}

#[allow(clippy::cast_precision_loss)]
fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let backend_root = Path::new(&manifest_dir);
    let router_root = backend_root.join("src").join("router");

    println!("cargo:rerun-if-changed=src/router");
    println!("cargo:rerun-if-changed=src/lib.rs");

    let all_routes = collect_all_routes(&router_root);
    let mut annotated: Vec<Route> = Vec::new();
    let mut missing: Vec<&Route> = Vec::new();
    // Defining files are parsed once; their path findings are printed once.
    let mut handler_scans: HashMap<PathBuf, HandlersScan> = HashMap::new();

    for r in &all_routes {
        let source = route_source(r, backend_root);
        if !handler_scans.contains_key(&source) {
            let scan = match std::fs::read_to_string(&source) {
                Ok(content) => scan_handlers(&content, &source),
                // An I/O problem is not a missing annotation: say so, then
                // fall back — the route reports as missing below, as before.
                Err(err) => {
                    println!("cargo:warning=cannot read {}: {err}", source.display());
                    HandlersScan::default()
                }
            };
            for finding in &scan.findings {
                println!("cargo:warning={}", finding.message());
            }
            // Attribute path agreement: compare exactly the two strings the
            // parity gate compares — the shared translation of the Rocket
            // URI against the annotation's declared path.
            for handler in &scan.handlers {
                if let Some(finding) = handler.path_mismatch(&source) {
                    println!("cargo:warning={}", finding.message());
                }
            }
            handler_scans.insert(source.clone(), scan);
        }
        let scan = handler_scans
            .get(&source)
            .expect("handler scan inserted in the iteration above");
        // The annotation gate is per function: only *this* handler's own
        // `#[utoipa::path]` counts, never a sibling's in the same file.
        // `candidate` prefers the annotated one when a `#[cfg(test)]`
        // duplicate shares the name, so the real route is not dropped.
        match scan.candidate(&r.handler) {
            Some(handler) if handler.annotated => annotated.push(Route {
                group_prefix: r.group_prefix.clone(),
                module_path: r.module_path.clone(),
                handler: r.handler.clone(),
            }),
            _ => missing.push(r),
        }
    }

    for r in &missing {
        println!(
            "cargo:warning=missing #[utoipa::path] annotation on {}::{}",
            r.module_path, r.handler
        );
    }

    if !all_routes.is_empty() {
        let annotated_count = annotated.len();
        let total = all_routes.len();
        if annotated_count != total {
            let coverage_pct = (annotated_count as f64 / total as f64) * 100.0;
            println!(
                "cargo:warning=utoipa annotation coverage: {coverage_pct:.1}% ({annotated_count}/{total})"
            );
        }
    }

    generate_openapi_rs(&annotated, &backend_root.join("src").join("openapi.rs"));
    generate_scenarios_rs(&annotated, backend_root);
}

/// Response components declared in `src/openapi_components.rs` and referenced
/// from `#[utoipa::path]` annotations. Registered in the generated `ApiDoc`.
const RESPONSE_COMPONENTS: &[&str] = &["Unauthorized"];

fn generate_openapi_rs(annotated: &[Route], dest: &Path) {
    let group_order: [&str; 6] = ["get", "post", "put", "delete", "fairing", "auth"];
    let mut annotated = annotated.to_vec();
    annotated.sort_by(|a, b| {
        let a_grp = group_order
            .iter()
            .position(|&g| g == a.group_prefix)
            .unwrap_or(99);
        let b_grp = group_order
            .iter()
            .position(|&g| g == b.group_prefix)
            .unwrap_or(99);
        a_grp
            .cmp(&b_grp)
            .then_with(|| a.module_path.cmp(&b.module_path))
            .then_with(|| a.handler.cmp(&b.handler))
    });

    let mut out = String::new();
    macro_rules! wln {
        ($($arg:tt)*) => {
            let _ = writeln!(out, $($arg)*);
        };
    }

    wln!("// Auto-generated by build.rs. Do not edit manually.");
    wln!("use utoipa::OpenApi;");
    wln!();

    // Shared response components referenced by the `#[utoipa::path]`
    // annotations below (e.g. `(status = 401, response = Unauthorized)`). One
    // list drives the import and the `components(responses(...))` registration,
    // so adding a component cannot leave the generated file with an import that
    // is registered under a different name.
    for component in RESPONSE_COMPONENTS {
        wln!("use crate::openapi_components::{component};");
    }

    for r in &annotated {
        let import_mod = if r.module_path == r.group_prefix {
            format!("crate::router::{g}", g = r.group_prefix)
        } else {
            format!(
                "crate::router::{g}::{m}",
                g = r.group_prefix,
                m = r.module_path
            )
        };
        wln!(
            "use {import_mod}::__path_{handler};",
            import_mod = import_mod,
            handler = r.handler
        );
    }

    wln!();
    wln!("#[derive(OpenApi)]");
    wln!("#[openapi(");
    wln!("    paths(");

    let mut current_group = String::new();
    for r in &annotated {
        let section = match r.group_prefix.as_str() {
            "get" => "// ── GET routes",
            "post" => "// ── POST routes",
            "put" => "// ── PUT routes",
            "delete" => "// ── DELETE routes",
            "fairing" => "// ── FAIRING routes",
            "auth" => "// ── auth routes (router/auth.rs)",
            _ => "",
        };
        if section != current_group {
            if !current_group.is_empty() {
                wln!();
            }
            wln!("        {section}");
            current_group = section.to_string();
        }
        wln!("        {},", r.handler);
    }

    wln!("    ),");
    wln!("    components(");
    wln!("        responses({})", RESPONSE_COMPONENTS.join(", "));
    wln!("    ),");
    wln!(")]");
    wln!("pub struct ApiDoc;");
    wln!();
    wln!("/// # Panics");
    wln!("/// Panics if `ApiDoc::openapi().to_json()` fails.");
    wln!("#[must_use]");
    wln!("pub fn generate_json() -> String {{");
    wln!("    ApiDoc::openapi()");
    wln!("        .to_json()");
    wln!("        .expect(\"OpenAPI serialization failed\")");
    wln!("}}");

    std::fs::write(dest, out.as_bytes()).unwrap_or_else(|e| {
        panic!("failed to write {}: {e}", dest.display());
    });
    // Do NOT rerun on dest — it's generated, would cause rebuild loop.
}

/// File that declares `route`'s handler: `router/<group>.rs` for an
/// unqualified entry (its module resolves to the group), otherwise
/// `router/<group>/<module>.rs`.
fn route_source(route: &Route, backend_root: &Path) -> PathBuf {
    if route.module_path == route.group_prefix {
        backend_root
            .join("src")
            .join("router")
            .join(format!("{}.rs", route.group_prefix))
    } else {
        backend_root
            .join("src")
            .join("router")
            .join(&route.group_prefix)
            .join(format!("{}.rs", route.module_path))
    }
}

fn collect_all_routes(router_root: &Path) -> Vec<Route> {
    // Every entry must name a file under `router/`: an entry that cannot be
    // read is reported as `cargo:warning=cannot read …` below rather than
    // skipped silently (there is no `fairing/` module — the fairing-style
    // renewal routes live in `auth.rs`, so no such entry exists).
    let mod_entries = [
        ("get", "get/mod.rs"),
        ("post", "post/mod.rs"),
        ("put", "put/mod.rs"),
        ("delete", "delete.rs"),
        // `auth.rs` mounts the token renewal routes through
        // `generate_fairing_routes()`. It has to be scanned here as well,
        // otherwise its annotated handlers never reach `paths(...)` and the
        // routes are mounted but undocumented.
        ("auth", "auth.rs"),
    ];

    let mut routes = Vec::new();

    for (group_prefix, rel_path) in &mod_entries {
        let path = router_root.join(rel_path);
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                println!("cargo:warning=cannot read {}: {err}", path.display());
                continue;
            }
        };

        let scan = scan_routes(&content, group_prefix, &path);
        for finding in &scan.findings {
            println!("cargo:warning={}", finding.message());
        }
        for handler in scan.handlers {
            routes.push(Route {
                group_prefix: group_prefix.to_string(),
                module_path: handler.module_path,
                handler: handler.handler,
            });
        }
    }

    routes
}

fn generate_scenarios_rs(_annotated: &[Route], backend_root: &Path) {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let test_dir = backend_root.join("tests").join("scenarios");
    let selftest_dir = test_dir.join("selftest");

    println!("cargo:rerun-if-changed={}", test_dir.display());

    let mut out = String::new();
    macro_rules! wln {
        ($($arg:tt)*) => {
            let _ = writeln!(out, $($arg)*);
        };
    }

    wln!("// Auto-generated by build.rs. Do not edit manually.");
    wln!();

    // Generate test for each YAML scenario
    let mut entries: Vec<_> = std::fs::read_dir(&test_dir)
        .unwrap_or_else(|e| panic!("read scenarios dir {}: {e}", test_dir.display()))
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "yaml"))
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in &entries {
        let path = entry.path();
        let stem = path.file_stem().unwrap().to_str().unwrap();
        let test_name = format!("scenario_{stem}");
        wln!("#[test]");
        wln!("fn {test_name}() {{");
        wln!("    run_backend_scenario(\"{stem}\");");
        wln!("}}");
        wln!();
        println!("cargo:rerun-if-changed={}", path.display());
    }

    // Generate test for each selftest YAML
    if selftest_dir.exists() {
        println!("cargo:rerun-if-changed={}", selftest_dir.display());
        let mut selftest_entries: Vec<_> = std::fs::read_dir(&selftest_dir)
            .unwrap_or_else(|e| panic!("read selftest dir {}: {e}", selftest_dir.display()))
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "yaml"))
            .collect();
        selftest_entries.sort_by_key(std::fs::DirEntry::file_name);

        for entry in &selftest_entries {
            let path = entry.path();
            let stem = path.file_stem().unwrap().to_str().unwrap();
            let test_name = format!("selftest_{stem}");
            wln!("#[test]");
            wln!("fn {test_name}() {{");
            wln!("    run_selftest_scenario(\"{stem}\");");
            wln!("}}");
            wln!();
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    let dest = Path::new(&out_dir).join("scenarios.rs");
    std::fs::write(&dest, out.as_bytes())
        .unwrap_or_else(|e| panic!("write {}: {e}", dest.display()));
}
