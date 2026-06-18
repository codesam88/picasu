mod generator;
mod plan;

use std::process::Command;

fn main() {
    let subcommand = std::env::args().nth(1);

    if matches!(subcommand.as_deref(), Some("-h" | "--help")) {
        print_help("");
        return;
    }

    match subcommand.as_deref() {
        Some("emit-openapi") => emit_openapi(),
        Some("test-backend") => {
            let just_generate = std::env::args().any(|a| a == "--generate-only");
            generator::generate_all();
            if just_generate {
                return;
            }
            let status = Command::new("cargo")
                .args([
                    "nextest",
                    "run",
                    "--package",
                    "urocissa",
                    "--",
                    "scenarios_generated",
                ])
                .current_dir(workspace_root())
                .status()
                .expect("failed to run cargo nextest for backend scenarios");
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
        Some("test-generator") => {
            generator::generate_negative_tests();
            let status = Command::new("cargo")
                .args(["nextest", "run", "--", "test_generator_generated"])
                .current_dir(workspace_root())
                .status()
                .expect("failed to run cargo nextest for negative tests");
            let out_path =
                workspace_root().join("gallery-backend/src/tests/test_generator_generated.rs");
            let empty = "// @generated — empty placeholder; re-run `cargo xtask test-generator`\n";
            let _ = std::fs::write(&out_path, empty);
            if !status.success() {
                eprintln!("negative tests FAILED — assertion machinery may not be catching errors");
                std::process::exit(1);
            }
            eprintln!("all negative tests passed (each panicked as expected)");
        }
        Some("plan") => {
            let args: Vec<String> = std::env::args().skip(2).collect();
            let mut status_filter: Option<&str> = None;
            let mut type_filter: Option<&str> = None;
            let mut priority_filter: Option<&str> = None;
            let mut area_filter: Option<&str> = None;
            let mut search_query: Option<&str> = None;
            let mut sort_keys: Vec<String> = Vec::new();
            let mut kanban = false;
            let mut check = false;

            let mut i = 0;

            while i < args.len() {
                let arg = args[i].as_str();
                if arg == "--kanban" || arg == "-k" {
                    kanban = true;
                } else if arg == "--lint" {
                    check = true;
                } else if arg == "--format" {
                    plan::normalize_all();
                    return;
                } else if arg == "-h" || arg == "--help" {
                    print_help("plan");
                    return;
                } else if arg == "--search" || arg == "-q" {
                    if i + 1 < args.len() {
                        i += 1;
                        search_query = Some(&args[i]);
                    } else {
                        eprintln!("--search / -q requires a value");
                        std::process::exit(1);
                    }
                } else if arg.starts_with("--search=") {
                    search_query = Some(arg.trim_start_matches("--search="));
                } else if arg.starts_with("-q=") {
                    search_query = Some(arg.trim_start_matches("-q="));
                } else if arg == "--status" || arg == "-s" {
                    resolve_filter_or_sort(
                        &args, &mut i, "--status=", &mut status_filter, "status", &mut sort_keys,
                    );
                } else if arg == "--type" || arg == "-t" {
                    resolve_filter_or_sort(
                        &args, &mut i, "--type=", &mut type_filter, "type", &mut sort_keys,
                    );
                } else if arg == "--priority" || arg == "-p" {
                    resolve_filter_or_sort(
                        &args, &mut i, "--priority=", &mut priority_filter, "priority", &mut sort_keys,
                    );
                } else if arg == "--area" || arg == "-a" {
                    resolve_filter_or_sort(
                        &args, &mut i, "--area=", &mut area_filter, "area", &mut sort_keys,
                    );
                } else if arg.starts_with("--status=") {
                    status_filter = Some(arg.trim_start_matches("--status="));
                } else if arg.starts_with("--type=") {
                    type_filter = Some(arg.trim_start_matches("--type="));
                } else if arg.starts_with("--priority=") {
                    priority_filter = Some(arg.trim_start_matches("--priority="));
                } else if arg.starts_with("--area=") {
                    area_filter = Some(arg.trim_start_matches("--area="));
                } else {
                    eprintln!("unknown flag: {}", arg);
                    eprintln!("use --help for usage");
                    std::process::exit(1);
                }
                i += 1;
            }
            if check {
                let ok = plan::validate_all();
                if !ok {
                    std::process::exit(1);
                }
                return;
            }
            plan::list_tasks(
                status_filter,
                type_filter,
                priority_filter,
                area_filter,
                search_query,
                kanban,
                &sort_keys,
            );
        }
        Some(other) => {
            eprintln!("unknown subcommand: {other}");
            print_help_summary();
            std::process::exit(1);
        }
        None => {
            print_help("");
            std::process::exit(1);
        }
    }
}

fn resolve_filter_or_sort<'a>(
    args: &'a [String],
    i: &mut usize,
    _eq_prefix: &str,
    filter: &mut Option<&'a str>,
    sort_key: &str,
    sort_keys: &mut Vec<String>,
) {
    if *i + 1 < args.len() && !args[*i + 1].starts_with('-') {
        *i += 1;
        *filter = Some(&args[*i]);
    } else {
        sort_keys.push(sort_key.to_string());
    }
}

fn print_help(sub: &str) {
    match sub {
        "plan" => {
            println!("cargo xtask plan [FLAGS]\n");
            println!("Search tasks in .plan directory. Default is table view sorted by priority then slug.\n");
            println!("Flags that take a value filter; without a value they become sort keys:");
            println!("  -s, --status <s>     filter or sort by status");
            println!("  -t, --type <t>       filter or sort by type");
            println!("  -p, --priority <p>   filter or sort by priority");
            println!("  -a, --area <a>       filter or sort by area");
            println!("  -q, --search <q>     search slug and body text (requires value)");
            println!("  -k, --kanban         group by status (vertical sections)");
            println!("  --lint               validate frontmatter only, then exit");
            println!("  --format             normalize frontmatter + body, then exit\n");
            println!("Frontmatter is validated on every invocation; warnings go to stderr.\n");
            println!("Examples:");
            println!("  cargo xtask plan -a -p            sort by area then priority");
            println!("  cargo xtask plan -s open -p high  filter open, priority=high");
            println!("  cargo xtask plan -a backend -k    filter area=backend, kanban view");
            println!("  cargo xtask plan --lint           validate only");
            println!("  cargo xtask plan --format         auto-format all task files");
        }
        _ => {
            println!("cargo xtask <subcommand>\n");
            println!("  emit-openapi    write gallery-backend/openapi.json from utoipa annotations");
            println!("  test-backend    generate E2E scenarios from YAML and run via nextest");
            println!("  test-generator  run negative self-tests for the assertion pipeline");
            println!("  plan            list/search/validate .plan tasks (see `cargo xtask plan --help`)");
        }
    }
}

fn print_help_summary() {
    eprintln!("available: emit-openapi, test-backend, test-generator, plan");
    eprintln!("use --help for details");
}

fn workspace_root() -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
        std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string()
    }));
    // CARGO_MANIFEST_DIR is xtask/; workspace root is one level up
    dir.parent().unwrap().to_path_buf()
}

fn emit_openapi() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--package",
            "urocissa",
            "--bin",
            "urocissa-openapi",
            "--features",
            "openapi",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .output()
        .expect("failed to run urocissa-openapi");

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    let spec = String::from_utf8(output.stdout).expect("openapi output is not valid UTF-8");
    let path = std::path::Path::new("gallery-backend").join("openapi.json");
    std::fs::write(&path, spec.as_bytes()).expect("failed to write openapi.json");

    eprintln!("wrote {}", path.display());
}
