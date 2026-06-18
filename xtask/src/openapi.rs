use std::fmt::Write;
use std::path::Path;

const PAGE_MODULES: &[&str] = &["get_page", "random"];

const BACKEND_SRC: &str = "gallery-backend/src";

/// Route handler identified by its group, module, and function name.
#[derive(Debug)]
struct Route {
    group_prefix: String,
    module_path: String,
    handler: String,
}

// ── Coverage check (fast, no compilation) ─────────────────────────────────────

/// Structured coverage result.
#[derive(Debug)]
pub struct CoverageReport {
    pub total: usize,
    pub page_routes: Vec<String>,
    pub data_routes: usize,
    pub annotated: Vec<String>,
    pub missing: Vec<String>,
}

/// Run coverage check and return structured results.
pub fn check_coverage(router_root: &Path, backend_root: &Path) -> CoverageReport {
    let all_routes = collect_all_routes(router_root);

    let mut page_routes = Vec::new();
    let mut data_routes = Vec::new();

    for r in &all_routes {
        if PAGE_MODULES.contains(&r.module_path.as_str()) {
            page_routes.push(format!("{}::{}", r.module_path, r.handler));
        } else {
            data_routes.push(r);
        }
    }

    let annotated: Vec<String> = data_routes
        .iter()
        .filter(|r| has_annotation(r, backend_root))
        .map(|r| format!("{}::{}", r.module_path, r.handler))
        .collect();

    let missing: Vec<String> = data_routes
        .iter()
        .filter(|r| !has_annotation(r, backend_root))
        .map(|r| format!("{}::{}", r.module_path, r.handler))
        .collect();

    CoverageReport {
        total: all_routes.len(),
        page_routes,
        data_routes: data_routes.len(),
        annotated,
        missing,
    }
}

/// Run coverage check on the real codebase, print report, exit on failure.
pub fn run_coverage() {
    let router_root = Path::new("gallery-backend/src/router");
    let backend_root = Path::new(BACKEND_SRC);
    let report = check_coverage(router_root, backend_root);

    let coverage_pct = if report.data_routes > 0 {
        (report.annotated.len() as f64 / report.data_routes as f64) * 100.0
    } else {
        0.0
    };

    println!("=== OpenAPI Annotation Coverage ===\n");
    println!("Total registered routes:     {:>3}", report.total);
    println!(
        "Page routes (excluded):      {:>3}",
        report.page_routes.len()
    );
    println!("Data API routes:             {:>3}", report.data_routes);
    println!("Annotated:                   {:>3}", report.annotated.len());
    println!("Missing:                     {:>2}", report.missing.len());
    println!("Coverage:                    {:>5.1}%\n", coverage_pct);

    if !report.missing.is_empty() {
        println!("Missing annotations ({}):", report.missing.len());
        for label in &report.missing {
            println!("  - {}", label);
        }
        println!();
    }

    println!("Annotated ({}):", report.annotated.len());
    for label in &report.annotated {
        println!("  ✓ {}", label);
    }

    if !report.page_routes.is_empty() {
        println!("\nPage routes ({}):", report.page_routes.len());
        for label in &report.page_routes {
            println!("  · {}", label);
        }
    }

    if !report.missing.is_empty() {
        eprintln!(
            "\nError: {} data API routes are missing utoipa annotations.",
            report.missing.len()
        );
        std::process::exit(1);
    }

    println!("\nAll data API routes are annotated ✓");
}

// ── Generator ─────────────────────────────────────────────────────────────────

pub fn generate_openapi_rs() {
    let router_root = Path::new("gallery-backend/src/router");
    let backend_root = Path::new(BACKEND_SRC);
    let dest = Path::new("gallery-backend/src/openapi.rs");

    let all_routes = collect_all_routes(router_root);

    // Filter annotated non-page routes.
    struct AnnotatedRoute {
        group_prefix: String,
        module_path: String,
        handler: String,
    }

    let mut annotated: Vec<AnnotatedRoute> = all_routes
        .into_iter()
        .filter(|r| {
            !PAGE_MODULES.contains(&r.module_path.as_str()) && has_annotation(r, backend_root)
        })
        .map(|r| AnnotatedRoute {
            group_prefix: r.group_prefix,
            module_path: r.module_path,
            handler: r.handler,
        })
        .collect();

    // Sort by group (get, post, put, delete, fairing), then by handler name.
    let group_order: [&str; 5] = ["get", "post", "put", "delete", "fairing"];
    annotated.sort_by(|a, b| {
        let a_grp = group_order.iter().position(|&g| g == a.group_prefix).unwrap_or(99);
        let b_grp = group_order.iter().position(|&g| g == b.group_prefix).unwrap_or(99);
        a_grp
            .cmp(&b_grp)
            .then_with(|| a.module_path.cmp(&b.module_path))
            .then_with(|| a.handler.cmp(&b.handler))
    });

    // ── Build output ──────────────────────────────────────────────────
    let mut out = String::new();

    writeln!(
        out,
        "// Auto-generated by `cargo xtask openapi-gen`. Do not edit manually."
    )
    .unwrap();
    writeln!(out, "use utoipa::OpenApi;").unwrap();
    writeln!(out).unwrap();

    for r in &annotated {
        let import_mod = format!(
            "crate::router::{g}::{m}",
            g = r.group_prefix,
            m = r.module_path
        );
        writeln!(
            out,
            "use {import_mod}::__path_{handler};",
            import_mod = import_mod,
            handler = r.handler
        )
        .unwrap();
    }

    writeln!(out).unwrap();
    writeln!(out, "#[derive(OpenApi)]").unwrap();
    writeln!(out, "#[openapi(").unwrap();
    writeln!(out, "    paths(").unwrap();

    let mut current_group = String::new();
    for r in &annotated {
        let section = match r.group_prefix.as_str() {
            "get" => "// ── GET routes",
            "post" => "// ── POST routes",
            "put" => "// ── PUT routes",
            "delete" => "// ── DELETE routes",
            "fairing" => "// ── FAIRING routes",
            _ => "",
        };
        if section != current_group {
            if !current_group.is_empty() {
                writeln!(out).unwrap();
            }
            writeln!(out, "        {section}").unwrap();
            current_group = section.to_string();
        }
        writeln!(out, "        {},", r.handler).unwrap();
    }

    writeln!(out, "    ),").unwrap();
    writeln!(out, ")]").unwrap();
    writeln!(out, "pub struct ApiDoc;").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "#[must_use]").unwrap();
    writeln!(out, "pub fn generate_json() -> String {{").unwrap();
    writeln!(out, "    ApiDoc::openapi()").unwrap();
    writeln!(out, "        .to_json()").unwrap();
    writeln!(out, "        .expect(\"OpenAPI serialization failed\")").unwrap();
    writeln!(out, "}}").unwrap();

    std::fs::write(dest, out.as_bytes()).unwrap_or_else(|e| {
        eprintln!("Error writing {dest}: {e}", dest = dest.display());
        std::process::exit(1);
    });
    eprintln!("wrote {}", dest.display());
}

// ─── Route discovery ──────────────────────────────────────────────────────────

fn has_annotation(route: &Route, backend_root: &Path) -> bool {
    let source = backend_root
        .join("router")
        .join(&route.group_prefix)
        .join(format!("{}.rs", route.module_path));
    let content = match std::fs::read_to_string(&source) {
        Ok(c) => c,
        Err(_) => return false,
    };
    content.contains("utoipa::path") && content.contains(&format!("fn {}(", route.handler))
}

fn collect_all_routes(router_root: &Path) -> Vec<Route> {
    let mod_files = [
        "get/mod.rs",
        "post/mod.rs",
        "put/mod.rs",
        "delete/mod.rs",
        "fairing/mod.rs",
    ];

    let mut routes = Vec::new();

    for rel_path in &mod_files {
        let path = router_root.join(rel_path);
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Warning: could not read {}: {}", path.display(), e);
                continue;
            }
        };

        let group_prefix = rel_path.strip_suffix("/mod.rs").unwrap_or("");

        if let Some(routes_start) = content.find("routes![") {
            let start = routes_start + "routes![".len();
            let rest = &content[start..];
            if let Some(end) = find_matching_bracket(rest) {
                let block = &rest[..end];
                for line in block.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with("//") {
                        continue;
                    }
                    let entry = trimmed.strip_suffix(',').unwrap_or(trimmed).trim();
                    let entry = entry.strip_suffix(']').unwrap_or(entry).trim();
                    if entry.is_empty() {
                        continue;
                    }

                    let (mod_name, handler) = if let Some(pos) = entry.rfind("::") {
                        (entry[..pos].to_string(), entry[pos + 2..].to_string())
                    } else {
                        (group_prefix.to_string(), entry.to_string())
                    };

                    routes.push(Route {
                        group_prefix: group_prefix.to_string(),
                        module_path: mod_name,
                        handler,
                    });
                }
            }
        }
    }

    routes
}

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
