mod plan;
mod test_image;

fn main() {
    let subcommand = std::env::args().nth(1);

    if matches!(subcommand.as_deref(), Some("-h" | "--help")) {
        print_help("");
        return;
    }

    match subcommand.as_deref() {
        Some("test-image") => {
            let args: Vec<String> = std::iter::once("test-image".into())
                .chain(std::env::args().skip(2))
                .collect();
            test_image::run_cli(args.into_iter());
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
                        &args,
                        &mut i,
                        "--status=",
                        &mut status_filter,
                        "status",
                        &mut sort_keys,
                    );
                } else if arg == "--type" || arg == "-t" {
                    resolve_filter_or_sort(
                        &args,
                        &mut i,
                        "--type=",
                        &mut type_filter,
                        "type",
                        &mut sort_keys,
                    );
                } else if arg == "--priority" || arg == "-p" {
                    resolve_filter_or_sort(
                        &args,
                        &mut i,
                        "--priority=",
                        &mut priority_filter,
                        "priority",
                        &mut sort_keys,
                    );
                } else if arg == "--area" || arg == "-a" {
                    resolve_filter_or_sort(
                        &args,
                        &mut i,
                        "--area=",
                        &mut area_filter,
                        "area",
                        &mut sort_keys,
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
            println!(
                "Search tasks in .plan directory. Default is table view sorted by priority then slug.\n"
            );
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
            println!(
                "  plan            list/search/validate .plan tasks (see `cargo xtask plan --help`)"
            );
            println!(
                "  test-image      generate test images from JSON specs (single, batch, library)"
            );
        }
    }
}

fn print_help_summary() {
    eprintln!("available: plan, test-image");
    eprintln!("use --help for details");
}
