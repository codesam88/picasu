mod plan;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut status_filter: Option<String> = None;
    let mut type_filter: Option<String> = None;
    let mut priority_filter: Option<String> = None;
    let mut area_filter: Option<String> = None;
    let mut search_query: Option<String> = None;
    let mut sort_keys: Vec<String> = Vec::new();
    let mut kanban = false;
    let mut check = false;
    let mut root: Option<std::path::PathBuf> = None;

    let mut i = 0;

    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        print_help();
        return;
    }

    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--kanban" || arg == "-k" {
            kanban = true;
        } else if arg == "--lint" {
            check = true;
        } else if arg == "--format" {
            let root = root.unwrap_or_else(plan::workspace_root);
            plan::normalize_all(&root);
            return;
        } else if arg == "--root" || arg == "-r" {
            if i + 1 < args.len() {
                i += 1;
                root = Some(std::path::PathBuf::from(&args[i]));
            } else {
                eprintln!("--root / -r requires a path");
                std::process::exit(1);
            }
        } else if arg.starts_with("--root=") {
            root = Some(std::path::PathBuf::from(arg.trim_start_matches("--root=")));
        } else if arg.starts_with("-r=") {
            root = Some(std::path::PathBuf::from(arg.trim_start_matches("-r=")));
        } else if arg == "--search" || arg == "-q" {
            if i + 1 < args.len() {
                i += 1;
                search_query = Some(args[i].clone());
            } else {
                eprintln!("--search / -q requires a value");
                std::process::exit(1);
            }
        } else if arg.starts_with("--search=") {
            search_query = Some(arg.trim_start_matches("--search=").to_string());
        } else if arg.starts_with("-q=") {
            search_query = Some(arg.trim_start_matches("-q=").to_string());
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
            status_filter = Some(arg.trim_start_matches("--status=").to_string());
        } else if arg.starts_with("--type=") {
            type_filter = Some(arg.trim_start_matches("--type=").to_string());
        } else if arg.starts_with("--priority=") {
            priority_filter = Some(arg.trim_start_matches("--priority=").to_string());
        } else if arg.starts_with("--area=") {
            area_filter = Some(arg.trim_start_matches("--area=").to_string());
        } else {
            eprintln!("unknown flag: {}", arg);
            eprintln!("use --help for usage");
            std::process::exit(1);
        }
        i += 1;
    }

    let root = root.unwrap_or_else(plan::workspace_root);

    if check {
        let ok = plan::validate_all(&root);
        if !ok {
            std::process::exit(1);
        }
        return;
    }

    plan::list_tasks(
        &root,
        status_filter.as_deref(),
        type_filter.as_deref(),
        priority_filter.as_deref(),
        area_filter.as_deref(),
        search_query.as_deref(),
        kanban,
        &sort_keys,
    );
}

fn resolve_filter_or_sort(
    args: &[String],
    i: &mut usize,
    _eq_prefix: &str,
    filter: &mut Option<String>,
    sort_key: &str,
    sort_keys: &mut Vec<String>,
) {
    if *i + 1 < args.len() && !args[*i + 1].starts_with('-') {
        *i += 1;
        *filter = Some(args[*i].clone());
    } else {
        sort_keys.push(sort_key.to_string());
    }
}

fn print_help() {
    println!("plan [FLAGS]\n");
    println!(
        "Search tasks in .plan directory. Default is table view sorted by priority then slug.\n"
    );
    println!("Flags that take a value filter; without a value they become sort keys:");
    println!("  -s, --status <s>     filter or sort by status");
    println!("  -t, --type <t>       filter or sort by type");
    println!("  -p, --priority <p>   filter or sort by priority");
    println!("  -a, --area <a>       filter or sort by area");
    println!("  -r, --root <path>    repo root containing .plan (default: auto-detect)");
    println!("  -q, --search <q>     search slug and body text (requires value)");
    println!("  -k, --kanban         group by status (vertical sections)");
    println!("  --lint               validate frontmatter only, then exit");
    println!("  --format             normalize frontmatter + body, then exit\n");
    println!("Frontmatter is validated on every invocation; warnings go to stderr.\n");
    println!("Examples:");
    println!("  plan -a -p            sort by area then priority");
    println!("  plan -s open -p high  filter open, priority=high");
    println!("  plan -a backend -k    filter area=backend, kanban view");
    println!("  plan --lint           validate only");
    println!("  plan --format         auto-format all task files");
}
