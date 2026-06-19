use std::fmt::Write;
use std::path::Path;

fn main() {
    println!("cargo::rerun-if-changed=tests/scenarios");
    println!("cargo::rerun-if-changed=tests/scenarios/selftest");

    let out_dir = std::env::var("OUT_DIR").unwrap();

    let mut lines = String::new();

    let manifest_dir: std::path::PathBuf = std::env::var("CARGO_MANIFEST_DIR").map_or_else(
        |_| std::env::current_dir().unwrap(),
        std::path::PathBuf::from,
    );

    let backend_dir = manifest_dir.join("tests/scenarios");
    let mut entries: Vec<_> = std::fs::read_dir(&backend_dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", backend_dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
        })
        .collect();
    entries.sort();

    for path in &entries {
        let name = path.file_stem().unwrap().to_str().unwrap();
        let _ = writeln!(
            lines,
            r#"#[test]
fn backend_{name}() {{
    run_backend_scenario("{name}");
}}
"#,
        );
    }

    let selftest_dir = manifest_dir.join("tests/scenarios/selftest");
    let mut entries: Vec<_> = std::fs::read_dir(&selftest_dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", selftest_dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
        })
        .collect();
    entries.sort();

    for path in &entries {
        let name = path.file_stem().unwrap().to_str().unwrap();
        let _ = writeln!(
            lines,
            r#"#[test]
fn selftest_{name}() {{
    run_selftest_scenario("{name}");
}}
"#,
        );
    }

    let out_path = Path::new(&out_dir).join("scenarios.rs");
    std::fs::write(&out_path, &lines)
        .unwrap_or_else(|e| panic!("write {}: {e}", out_path.display()));
}
