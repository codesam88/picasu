//! Unit tests for the request-taint reachability analysis in
//! `build/reachability.rs`, plus the registry gate over the real tree.
//!
//! The analysis decides whether a value derived from a Rocket request
//! parameter can reach a panicking construct — the failure class behind the
//! `GET /get/get-scroll-bar` panic on an unknown snapshot id — and the guard
//! check catches the companion `let _ = auth;` bug, where a guard is bound
//! and thrown away. Both historical bugs were reachable only through shapes
//! that a naive pass misses (`spawn_blocking` closures, guard discards), so
//! each propagation rule has a fixture here; a regression in the rules turns
//! into a failing test rather than a silently greener gate.
//!
//! The last test is the gate itself: it runs the analysis over `src/`, parses
//! `backend/reachability-registry.txt` and fails on a *new* site (an
//! unregistered finding) or a *stale* entry (an entry whose site is gone).

// The analysis under test; the same module `build.rs` includes. Self-contained
// (no `super::` references), so this single include is enough.
#[path = "../../build/reachability.rs"]
mod analysis;

use std::path::{Path, PathBuf};

use analysis::{Error, GuardKind, Report, SinkKind, analyze, gate_diff, parse_registry};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Analyse the given `(path, content)` units.
fn analysis(sources: &[(&str, &str)]) -> Report {
    let owned: Vec<(String, String)> = sources
        .iter()
        .map(|(path, content)| ((*path).to_string(), (*content).to_string()))
        .collect();
    analyze(&owned)
}

/// Analyse a single source unit as `src/fixture.rs`.
fn fixture(source: &str) -> Report {
    analysis(&[("src/fixture.rs", source)])
}

/// Every analysis error rendered — the fixtures assert there are none, so a
/// parse failure or a fixpoint that hit the bound cannot hide a green run.
fn error_messages(report: &Report) -> Vec<String> {
    report.errors.iter().map(Error::message).collect()
}

/// The (kind, line, seed) of every sink, for compact assertions.
fn sinks(report: &Report) -> Vec<(SinkKind, usize, String)> {
    report
        .sinks
        .iter()
        .map(|sink| (sink.kind, sink.line, sink.seed.clone()))
        .collect()
}

/// The 1-based line of the first occurrence of `needle` in `source` —
/// expected sink lines are derived from the fixture text instead of being
/// hand-counted (the raw strings start with a newline, which shifts every
/// absolute line number).
fn line_of(source: &str, needle: &str) -> usize {
    let offset = source
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not found in fixture"));
    source[..offset].matches('\n').count() + 1
}

/// Render a report as registry text, with a placeholder reason per line —
/// the input shape the gate compares against.
fn registry_from(report: &Report) -> String {
    let mut lines: Vec<String> = report
        .sinks
        .iter()
        .map(|sink| sink.registry_line("fixture: reachable from the request"))
        .chain(
            report
                .guards
                .iter()
                .map(|guard| guard.registry_line("fixture: guard binding discarded")),
        )
        .collect();
    lines.sort();
    lines.join("\n")
}

/// Parse a registry built from a report (fixture text must be valid).
fn entries_of(text: &str) -> Vec<analysis::RegistryEntry> {
    parse_registry(text).unwrap_or_else(|error| panic!("{}", error.message()))
}

/// Every `.rs` file under `backend/src`, sorted for deterministic pass order,
/// as `(path relative to the backend root, content)` pairs — the same input
/// shape `build.rs` uses.
fn crate_sources() -> Vec<(String, String)> {
    let backend_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(backend_root.join("src"))
        .into_iter()
        .filter_map(Result::ok)
        .map(|entry| entry.path().to_path_buf())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    files.sort();
    files
        .iter()
        .map(|path| {
            let relative = path
                .strip_prefix(backend_root)
                .unwrap_or(path)
                .display()
                .to_string();
            let content = std::fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"));
            (relative, content)
        })
        .collect()
}

// ── Propagation fixtures ─────────────────────────────────────────────────────

#[test]
fn taint_flows_through_a_let_binding() {
    let source = r#"
#[get("/x?<value>")]
fn handler(value: i64) {
    let doubled = value;
    doubled.unwrap();
}
"#;
    let report = fixture(source);
    assert!(report.errors.is_empty(), "{:?}", error_messages(&report));
    assert_eq!(
        sinks(&report),
        vec![(
            SinkKind::Unwrap,
            line_of(source, "doubled.unwrap()"),
            "handler(value)".to_string()
        )],
        "{report:?}"
    );
    assert_eq!(report.sinks[0].function, "handler");
    assert_eq!(report.sinks[0].file, "src/fixture.rs");
    assert!(
        report.sinks[0]
            .path
            .starts_with("handler(value) -> doubled.unwrap()"),
        "path should start at the seed and end at the sink: {}",
        report.sinks[0].path
    );
}

#[test]
fn taint_flows_through_a_method_call_on_a_tainted_receiver() {
    let source = r#"
#[get("/x?<value>")]
fn handler(value: i64) {
    let text = value.to_string();
    text.unwrap();
}
"#;
    let report = fixture(source);
    assert!(report.errors.is_empty(), "{:?}", error_messages(&report));
    assert_eq!(
        sinks(&report),
        vec![(
            SinkKind::Unwrap,
            line_of(source, "text.unwrap()"),
            "handler(value)".to_string()
        )],
        "{report:?}"
    );
}

#[test]
fn taint_flows_into_a_move_closure_body() {
    // The closure captures `value` by move; the sink sits *inside* the body.
    let source = r#"
#[get("/x?<value>")]
fn handler(value: i64) {
    let render = move || {
        let shown = value;
        shown.unwrap()
    };
    render();
}
"#;
    let report = fixture(source);
    assert!(report.errors.is_empty(), "{:?}", error_messages(&report));
    assert_eq!(
        sinks(&report),
        vec![(
            SinkKind::Unwrap,
            line_of(source, "shown.unwrap()"),
            "handler(value)".to_string()
        )],
        "{report:?}"
    );
    assert!(
        report.sinks[0].path.contains("-> closure ->"),
        "the closure frame should appear in the path: {}",
        report.sinks[0].path
    );
}

#[test]
fn taint_reaches_a_sink_inside_spawn_blocking() {
    // The `get_rows` shape: `index` moves into a `spawn_blocking` closure and
    // is used as an index there — reachable only through the closure.
    let source = r#"
#[get("/get/get-rows?<index>")]
fn get_rows(index: usize) -> i64 {
    tokio::task::spawn_blocking(move || ROWS[index])
        .await
        .unwrap()
}
"#;
    let report = fixture(source);
    assert!(report.errors.is_empty(), "{:?}", error_messages(&report));
    assert_eq!(report.sinks.len(), 2, "{report:?}");

    let index_sink = report
        .sinks
        .iter()
        .find(|sink| sink.kind == SinkKind::Index)
        .expect("the tainted index inside the closure is a sink");
    assert_eq!(index_sink.line, line_of(source, "ROWS[index]"));
    assert_eq!(index_sink.seed, "get_rows(index)");
    assert!(
        index_sink.path.contains("-> spawn_blocking ->"),
        "path must show the spawn_blocking frame: {}",
        index_sink.path
    );

    let unwrap_sink = report
        .sinks
        .iter()
        .find(|sink| sink.kind == SinkKind::Unwrap)
        .expect("the closure's tainted result reaching unwrap is a sink");
    assert_eq!(unwrap_sink.seed, "get_rows(index)");
}

#[test]
fn taint_crosses_files_into_a_helper() {
    // Interprocedural: the handler calls a free function defined in another
    // "file"; the callee's parameter is tainted and the sink inside it reports
    // the handler's seed.
    let helper = r#"
fn helper(value: i64) -> String {
    format!("value={value}").expect("fmt must be infallible")
}
"#;
    let report = analysis(&[
        (
            "src/handler.rs",
            r#"
#[get("/x?<value>")]
fn handler(value: i64) {
    helper(value);
}
"#,
        ),
        ("src/helper.rs", helper),
    ]);
    assert!(report.errors.is_empty(), "{:?}", error_messages(&report));
    assert_eq!(
        sinks(&report),
        vec![(
            SinkKind::Expect,
            line_of(helper, ".expect("),
            "handler(value)".to_string()
        )],
        "{report:?}"
    );
    assert_eq!(report.sinks[0].file, "src/helper.rs");
    assert_eq!(report.sinks[0].function, "helper");
    assert!(
        report.sinks[0]
            .path
            .starts_with("handler(value) -> helper -> "),
        "path should show the interprocedural frame: {}",
        report.sinks[0].path
    );
}

#[test]
fn json_body_parameter_is_a_seed() {
    let source = r#"
#[post("/p", data = "<body>")]
fn handler(body: Json<Request>) {
    let value = body.field;
    value.unwrap();
}
"#;
    let report = fixture(source);
    assert!(report.errors.is_empty(), "{:?}", error_messages(&report));
    assert_eq!(
        sinks(&report),
        vec![(
            SinkKind::Unwrap,
            line_of(source, "value.unwrap()"),
            "handler(body)".to_string()
        )],
        "{report:?}"
    );
}

// ── Sink-class fixtures ──────────────────────────────────────────────────────

#[test]
fn a_tainted_index_is_a_sink_but_a_tainted_collection_is_not() {
    let source = r#"
#[get("/x?<index>")]
fn handler(index: usize) -> i64 {
    let items = load();
    let first = items[0];
    items[index];
    first
}
fn load() -> Vec<i64> { Vec::new() }
"#;
    let report = fixture(source);
    assert!(report.errors.is_empty(), "{:?}", error_messages(&report));
    // Only `items[index]`: the untainted literal index is fine, and the
    // tainted *collection* in `items[0]` is a data question, not a panic.
    assert_eq!(
        sinks(&report),
        vec![(
            SinkKind::Index,
            line_of(source, "items[index]"),
            "handler(index)".to_string()
        )],
        "{report:?}"
    );
}

#[test]
fn a_config_invariant_expect_is_not_reported() {
    // The receiver comes from a static, not from the request: no seed, no
    // finding — even though the handler has a seed parameter in scope.
    let report = fixture(
        r#"
#[get("/x?<value>")]
fn handler(value: i64) {
    CONFIG.get().expect("config invariant");
    let _ = value;
}
"#,
    );
    assert!(report.errors.is_empty(), "{:?}", error_messages(&report));
    assert!(
        report.sinks.is_empty(),
        "a non-request-derived expect must not be reported: {:?}",
        report.sinks
    );
}

#[test]
fn a_panic_macro_under_a_tainted_branch_is_reported() {
    // `unreachable!()` has no operand; the branch condition supplies the seed.
    let source = r#"
#[get("/x?<value>")]
fn handler(value: i64) -> i64 {
    if value < 0 {
        unreachable!("negative value");
    }
    0
}
"#;
    let report = fixture(source);
    assert!(report.errors.is_empty(), "{:?}", error_messages(&report));
    assert_eq!(
        sinks(&report),
        vec![(
            SinkKind::Unreachable,
            line_of(source, "unreachable!("),
            "handler(value)".to_string()
        )],
        "{report:?}"
    );
}

// ── Guard-propagation fixtures ───────────────────────────────────────────────

#[test]
fn a_discarded_guard_is_reported() {
    // The historical `get-rows` / `get-scroll-bar` bug, verbatim in shape:
    // `GuardResult` bound and dropped without `?`, so the guard never runs.
    let source = r#"
#[get("/get/get-rows?<index>")]
fn get_rows(auth: GuardResult<GuardTimestamp>, index: usize) -> i64 {
    let _ = auth;
    index as i64
}
"#;
    let report = fixture(source);
    assert!(report.errors.is_empty(), "{:?}", error_messages(&report));
    assert_eq!(report.guards.len(), 1, "{:?}", report.guards);
    let guard = &report.guards[0];
    assert_eq!(guard.kind, GuardKind::Discarded);
    assert_eq!(guard.handler, "get_rows");
    assert_eq!(guard.guard_type, "GuardResult<GuardTimestamp>");
    assert_eq!(guard.binding, "auth");
    assert_eq!(guard.file, "src/fixture.rs");
    assert_eq!(
        guard.line,
        line_of(source, "let _ = auth;"),
        "the line of `let _ = auth;`"
    );
    let message = guard.message();
    assert!(message.contains("get_rows"), "{message}");
    assert!(message.contains("GuardResult<GuardTimestamp>"), "{message}");
    assert!(
        message.contains(&format!("src/fixture.rs:{}", guard.line)),
        "{message}"
    );
    // The `index` seed still reaches no panic: the guard check is separate
    // from the taint pass and reports a *missing use*, not a sink.
    assert!(report.sinks.is_empty(), "{:?}", report.sinks);
}

#[test]
fn a_propagated_guard_is_not_reported() {
    let report = fixture(
        r#"
#[get("/x")]
fn handler(auth: GuardResult<GuardTimestamp>) -> i64 {
    let _ = auth?;
    0
}
"#,
    );
    assert!(report.errors.is_empty(), "{:?}", error_messages(&report));
    assert!(
        report.guards.is_empty(),
        "`let _ = auth?;` propagates the guard: {:?}",
        report.guards
    );
}

#[test]
fn a_never_read_guard_is_reported() {
    let report = fixture(
        r#"
#[get("/x")]
fn handler(_auth: GuardAuth) -> i64 {
    0
}
"#,
    );
    assert!(report.errors.is_empty(), "{:?}", error_messages(&report));
    assert_eq!(report.guards.len(), 1, "{:?}", report.guards);
    assert_eq!(report.guards[0].kind, GuardKind::NeverRead);
    assert_eq!(report.guards[0].guard_type, "GuardAuth");
    assert!(report.guards[0].message().contains("never reads it"));
}

// ── Gate mechanics ───────────────────────────────────────────────────────────

#[test]
fn gate_reports_a_new_site() {
    let report = fixture(
        r#"
#[get("/x?<value>")]
fn handler(value: i64) {
    value.unwrap();
}
"#,
    );
    assert_eq!(report.sinks.len(), 1, "{report:?}");

    // An empty registry: everything the analysis finds is new.
    let empty = entries_of("");
    let diff = gate_diff(&empty, &report);
    assert!(!diff.is_empty());
    assert_eq!(diff.new_sites.len(), 1);
    assert!(
        diff.new_sites[0].contains("UNREGISTERED"),
        "{}",
        diff.new_sites[0]
    );
    assert!(diff.new_sites[0].contains("taint-unwrap"));
    assert!(diff.new_sites[0].contains("src/fixture.rs"));
    assert!(diff.stale_entries.is_empty());
    assert!(diff.message().contains("new reachability sites"));

    // Registering exactly the finding makes the gate green — the registry is
    // seeded from the analysis output, entry for entry.
    let entries = entries_of(&registry_from(&report));
    assert!(gate_diff(&entries, &report).is_empty());
}

#[test]
fn gate_reports_a_stale_entry() {
    // A report with no findings, against a registry that still claims one.
    let report = fixture("fn unrelated() {}\n");
    assert!(report.sinks.is_empty() && report.guards.is_empty());
    let registry = "taint-expect\tsrc/gone.rs\told_handler\told(seed)\t.expect(\"gone\")\t\
                    fixture: the site was removed";
    let entries = entries_of(registry);
    let diff = gate_diff(&entries, &report);
    assert!(!diff.is_empty());
    assert!(diff.new_sites.is_empty());
    assert_eq!(diff.stale_entries.len(), 1);
    assert!(
        diff.stale_entries[0].contains("src/gone.rs"),
        "{}",
        diff.stale_entries[0]
    );
    assert!(
        diff.message()
            .contains("stale reachability registry entries")
    );
}

#[test]
fn a_moved_site_still_matches_the_registry() {
    // Line numbers are deliberately not part of the registry key: inserting
    // a blank line above the sink shifts every line but not the site, and the
    // gate must stay green (the whole reason the key is function + snippet).
    let original = fixture(
        r#"
#[get("/x?<value>")]
fn handler(value: i64) {
    value.unwrap();
}
"#,
    );
    let entries = entries_of(&registry_from(&original));

    let moved = fixture(
        r#"

#[get("/x?<value>")]
fn handler(value: i64) {
    value.unwrap();
}
"#,
    );
    assert_ne!(original.sinks[0].line, moved.sinks[0].line, "line shifted");
    assert!(
        gate_diff(&entries, &moved).is_empty(),
        "a shifted line must not read as new+stale: {}",
        gate_diff(&entries, &moved).message()
    );
}

#[test]
fn registry_parse_rejects_malformed_and_unreasoned_entries() {
    // Missing fields.
    let error = parse_registry("taint-expect\tsrc/a.rs").expect_err("too few fields");
    assert!(error.message().contains("tab-separated"), "{error:?}");

    // Every entry must carry a reason.
    let error =
        parse_registry("taint-expect\tsrc/a.rs\tf\tseed\tsnippet\t   ").expect_err("empty reason");
    assert!(error.message().contains("reason"), "{error:?}");

    // Duplicates would make the gate's set comparison ambiguous.
    let line = "taint-expect\tsrc/a.rs\tf\tseed\tsnippet\treason";
    let error = parse_registry(&format!("{line}\n{line}")).expect_err("duplicate");
    assert!(error.message().contains("duplicate"), "{error:?}");

    // Comments and blank lines are fine.
    let entries = parse_registry(&format!("# comment\n\n{line}\n")).expect("valid registry");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].reason, "reason");
}

// ── The gate over the real tree ──────────────────────────────────────────────

/// The enforcement point: `cargo test --lib` (and therefore `just test`)
/// compares the analysis of the whole `src/` tree against the committed
/// registry and fails on a new site, a stale entry, or an analysis error
/// (unparsable file, fixpoint that hit its bound). `build.rs` only warns; a
/// test is where a failure can stop the change.
#[test]
fn reachability_registry_matches_the_analysis() {
    let sources = crate_sources();
    assert!(sources.len() > 50, "expected the crate's sources");
    let started = std::time::Instant::now();
    let report = analyze(&sources);
    let elapsed = started.elapsed();
    eprintln!(
        "[reachability] analysed {} files in {elapsed:?} ({} sinks, {} guards)",
        sources.len(),
        report.sinks.len(),
        report.guards.len()
    );

    let errors = error_messages(&report);
    assert!(
        report.errors.is_empty(),
        "reachability analysis reported errors:\n{}",
        errors.join("\n")
    );

    let registry_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("reachability-registry.txt");
    let registry_text = std::fs::read_to_string(&registry_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", registry_path.display()));
    let entries =
        parse_registry(&registry_text).unwrap_or_else(|error| panic!("{}", error.message()));

    let diff = gate_diff(&entries, &report);
    assert!(diff.is_empty(), "{}", diff.message());
}

/// The analysis must converge inside its published bound on this tree — the
/// iteration-limit error is otherwise a gate failure, and this pins the
/// constant to the value the docs promise.
#[test]
fn analysis_converges_within_the_iteration_limit() {
    let report = analyze(&crate_sources());
    assert!(
        !report.errors.iter().any(|error| matches!(
            error,
            Error::IterationLimit { iterations } if *iterations == analysis::ITERATION_LIMIT
        )),
        "the analysis hit its iteration bound on this tree"
    );
}

/// On-demand helper: print every finding with its full path string. Used by
/// the acid test (reintroducing the two historical bugs) and by reviewers who
/// want to see *how* a site is reached — the gate itself compares structured
/// findings against the registry, not stdout, so this stays ignored.
#[test]
#[ignore = "on-demand: prints the full report with paths"]
fn dump_reachable_sites() {
    let started = std::time::Instant::now();
    let report = analyze(&crate_sources());
    eprintln!(
        "[reachability] {} files in {:?} — {} sinks, {} guards, {} errors",
        crate_sources().len(),
        started.elapsed(),
        report.sinks.len(),
        report.guards.len(),
        report.errors.len()
    );
    for sink in &report.sinks {
        eprintln!(
            "SINK {}  {}:{}  {}",
            sink.kind.as_str(),
            sink.file,
            sink.line,
            sink.function
        );
        eprintln!("    seed: {}", sink.seed);
        eprintln!("    path: {}", sink.path);
    }
    for guard in &report.guards {
        eprintln!("GUARD {}", guard.message());
    }
    for error in &report.errors {
        eprintln!("ERROR {}", error.message());
    }
}
