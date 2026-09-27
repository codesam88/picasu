//! Rocket request guards as a handler's signature declares them.
//!
//! Two shapes have to be told apart, because only one of them is enforced
//! without the handler's help: Rocket runs a guard parameter before the body, so
//! a rejected request never reaches it, while a `GuardResult<T>`/`Option<T>`
//! parameter hands the failure to the body. The second shape is the one that has
//! been dropped before — see `.plan/bug-get-rows-auth-guard-discarded.md` — so
//! every case below is about whether the body acts on the failure or only reads
//! the value.
//!
//! The vocabulary of guard names is checked against the backend here rather than
//! trusted: `every_request_guard_in_the_backend_is_known` fails when the codebase
//! adds a guard this crate does not recognise, and when the list names a guard the
//! codebase no longer declares.

use std::path::{Path, PathBuf};

use openapi_sanity::{GuardClass, scan_handlers};

/// The `struct` names a guard is recognised by, as the crate lists them.
const KNOWN: &[&str] = openapi_sanity::KNOWN_GUARDS;

/// A handler source with the given parameters and body.
///
/// The route attribute is there because a function declaring neither a route nor
/// an annotation is not a handler: the scanner reports nothing about it, and a
/// test about what a handler's parameters say needs a handler.
fn handler(params: &str, body: &str) -> String {
    format!("#[get(\"/page\")]\npub fn page({params}) {{\n{body}\n}}\n")
}

/// The guard bindings of a single handler, as `(class, parameter, enforcement)`.
fn bindings(params: &str, body: &str) -> Vec<(GuardClass, String, String)> {
    let source = handler(params, body);
    let scan = scan_handlers("guards.rs", &source);

    assert_eq!(scan.handlers.len(), 1, "one handler is declared");
    scan.handlers[0]
        .guards
        .iter()
        .map(|binding| {
            (
                binding.class,
                binding.parameter.clone(),
                format!("{:?}", binding.enforcement),
            )
        })
        .collect()
}

/// Every diagnostic the scan reports for a handler, as it is printed.
fn findings(params: &str, body: &str) -> Vec<String> {
    scan_handlers("guards.rs", &handler(params, body))
        .findings
        .iter()
        .map(ToString::to_string)
        .collect()
}

// ── A direct guard ────────────────────────────────────────────────────────────

#[test]
fn a_bare_guard_parameter_is_enforced_by_rocket() {
    assert_eq!(
        bindings("_auth: GuardAuth", ""),
        vec![(
            GuardClass::AdminCookie,
            "_auth".to_string(),
            "ByRocket".to_string()
        )]
    );
}

#[test]
fn a_guard_named_by_a_path_is_recognised_by_its_last_segment() {
    // Handlers name their guards through an import, so `crate::router::auth::
    // GuardShare` and `GuardShare` are the same guard; only the last segment of
    // the type path is read.
    assert_eq!(
        bindings("auth: crate::router::auth::GuardShare", ""),
        vec![(
            GuardClass::Share,
            "auth".to_string(),
            "ByRocket".to_string()
        )]
    );
}

#[test]
fn a_guard_behind_a_reference_is_a_direct_guard() {
    assert_eq!(
        bindings("auth: &GuardHash", ""),
        vec![(GuardClass::Hash, "auth".to_string(), "ByRocket".to_string())]
    );
}

#[test]
fn a_direct_guard_is_enforced_even_though_the_body_never_reads_it() {
    // The body is irrelevant here: Rocket runs the guard, so `_auth` needs no
    // reference to be safe. A name starting with `_` is the codebase's way of
    // saying exactly that.
    assert_eq!(
        bindings("_auth: GuardAuth", "let _ = 1;"),
        vec![(
            GuardClass::AdminCookie,
            "_auth".to_string(),
            "ByRocket".to_string()
        )]
    );
}

#[test]
fn several_guards_of_different_classes_are_all_reported() {
    let observed = bindings(
        "auth: GuardResult<GuardShare>, hash_guard: GuardResult<GuardHash>",
        "let _ = auth?;\nlet _ = hash_guard?;",
    );

    assert_eq!(
        observed,
        vec![
            (
                GuardClass::Share,
                "auth".to_string(),
                "ByHandler".to_string()
            ),
            (
                GuardClass::Hash,
                "hash_guard".to_string(),
                "ByHandler".to_string()
            ),
        ]
    );
}

// ── A deferred guard ──────────────────────────────────────────────────────────

#[test]
fn an_option_guard_is_deferred_and_matched() {
    // Rocket's `Option<T>` request wrapper turns a failed guard into `None`, so
    // the body still has to act on it — and dropping it is a wildcard binding like
    // any other.
    assert_eq!(
        bindings("auth: Option<GuardAuth>", "let _ = auth;"),
        vec![(
            GuardClass::AdminCookie,
            "auth".to_string(),
            "Discarded(Dropped)".to_string()
        )]
    );
    assert_eq!(
        bindings(
            "auth: Option<GuardAuth>",
            "match auth {\n    Some(claims) => drop(claims),\n    None => (),\n}"
        ),
        vec![(
            GuardClass::AdminCookie,
            "auth".to_string(),
            "ByHandler".to_string()
        )]
    );
}

#[test]
fn a_guard_result_propagated_with_a_question_mark_is_enforced() {
    // The codebase's idiom: the value is not needed, the failure is.
    assert_eq!(
        bindings("auth: GuardResult<GuardTimestamp>", "let _ = auth?;"),
        vec![(
            GuardClass::Timestamp,
            "auth".to_string(),
            "ByHandler".to_string()
        )]
    );
}

#[test]
fn a_guard_result_unwrapped_and_then_read_is_enforced() {
    assert_eq!(
        bindings(
            "auth: GuardResult<GuardShare>",
            "let claims = auth?;\nlet _ = claims.claims;"
        ),
        vec![(
            GuardClass::Share,
            "auth".to_string(),
            "ByHandler".to_string()
        )]
    );
}

#[test]
fn a_guard_result_returned_from_the_handler_is_enforced() {
    // The binding is the tail expression: the failure travels out of the handler
    // instead of being dropped inside it.
    assert_eq!(
        bindings("auth: GuardResult<GuardTimestamp>", "auth"),
        vec![(
            GuardClass::Timestamp,
            "auth".to_string(),
            "ByHandler".to_string()
        )]
    );
}

#[test]
fn a_guard_result_inspected_through_its_result_methods_is_enforced() {
    for body in [
        "if auth.is_err() { return; }",
        "let ok = auth.is_ok();",
        "let _ = auth.map_err(|error| error.to_string())?;",
        "let _ = auth.unwrap_or_default();",
        "let _ = auth.expect(\"guard\");",
    ] {
        let observed = bindings("auth: GuardResult<GuardAuth>", body);

        assert_eq!(
            observed,
            vec![(
                GuardClass::AdminCookie,
                "auth".to_string(),
                "ByHandler".to_string()
            )],
            "observing the failure with `{body}` enforces the guard"
        );
    }
}

// ── A deferred guard that is not enforced ─────────────────────────────────────

#[test]
fn a_guard_result_dropped_by_a_wildcard_let_is_discarded() {
    // The exact shape of bug-get-rows-auth-guard-discarded: the value is read and
    // the failure is thrown away, so the handler answers as if the request had
    // been authorized.
    assert_eq!(
        bindings("auth: GuardResult<GuardTimestamp>", "let _ = auth;"),
        vec![(
            GuardClass::Timestamp,
            "auth".to_string(),
            "Discarded(Dropped)".to_string()
        )]
    );
}

#[test]
fn a_guard_result_whose_claims_are_read_but_whose_failure_is_not_is_discarded() {
    assert_eq!(
        bindings("auth: GuardResult<GuardShare>", "let _ = auth.claims;"),
        vec![(
            GuardClass::Share,
            "auth".to_string(),
            "Discarded(Dropped)".to_string()
        )]
    );
}

#[test]
fn a_guard_result_the_body_never_mentions_is_discarded() {
    assert_eq!(
        bindings("auth: GuardResult<GuardUpload>", "let _ = 1;"),
        vec![(
            GuardClass::Upload,
            "auth".to_string(),
            "Discarded(Unread)".to_string()
        )]
    );
}

#[test]
fn a_renamed_binding_does_not_inherit_the_old_enforcement() {
    // `let _ = auth;` followed by a read of a *different* binding is still a
    // dropped guard: the observation has to be of the parameter's own name.
    assert_eq!(
        bindings(
            "auth: GuardResult<GuardAuth>, other: Option<String>",
            "let _ = auth;\nlet _ = other;"
        ),
        vec![(
            GuardClass::AdminCookie,
            "auth".to_string(),
            "Discarded(Dropped)".to_string()
        )]
    );
}

// ── Parameters that are not guards ────────────────────────────────────────────

#[test]
fn a_form_result_is_not_a_guard() {
    // `Result<Form<T>, Errors<T>>` is Rocket's form wrapper, and its first type
    // argument is not a guard; reading the first argument alone must not turn it
    // into one.
    let source = handler(
        "form: Result<Form<Upload<'_>>, Errors<'_>>",
        "let _ = form;",
    );
    let scan = scan_handlers("guards.rs", &source);

    assert!(
        scan.handlers[0].guards.is_empty(),
        "a form is data, not a request guard"
    );
}

#[test]
fn an_option_of_a_request_body_is_not_a_guard() {
    let source = handler(
        "query_data: Option<Json<Expression>>",
        "let _ = query_data;",
    );
    let scan = scan_handlers("guards.rs", &source);

    assert!(scan.handlers[0].guards.is_empty());
}

#[test]
fn a_data_parameter_next_to_a_guard_does_not_hide_it() {
    let source = handler(
        "auth: GuardResult<GuardAuth>, json: Json<EditFlags>, path: PathBuf",
        "let _ = auth?;\nlet _ = json;\nlet _ = path;",
    );
    let scan = scan_handlers("guards.rs", &source);

    assert_eq!(scan.handlers[0].guards.len(), 1);
    assert_eq!(scan.handlers[0].guards[0].class, GuardClass::AdminCookie);
}

// ── Anchors and the vocabulary ────────────────────────────────────────────────

#[test]
fn a_dropped_guard_is_reported_against_the_parameter_it_is_bound_to() {
    // The diagnostic is the whole rule, so it is asserted in full: a handler
    // name, the guard, the binding and the one character that makes the
    // difference between checking the failure and discarding it.
    assert_eq!(
        findings("auth: GuardResult<GuardTimestamp>", "let _ = auth;"),
        vec![
            "guards.rs:2: page: the deferred guard GuardTimestamp is bound to `auth` \
             and never enforced — `let _ = auth;` drops its failure; propagate it \
             with `?`, return it, or match on it"
                .to_string()
        ]
    );
}

#[test]
fn a_propagated_guard_is_not_reported() {
    assert_eq!(
        findings("auth: GuardResult<GuardTimestamp>", "let _ = auth?;"),
        Vec::<String>::new()
    );
    assert_eq!(findings("_auth: GuardAuth", ""), Vec::<String>::new());
}

#[test]
fn a_guard_binding_is_anchored_at_its_parameter() {
    // The parameter line is where the fix goes, and on a multi-line signature it
    // is not the function's own line.
    let source =
        "#[get(\"/page\")]\npub async fn page(\n    _auth: GuardAuth,\n    name: String,\n) {}\n";
    let scan = scan_handlers("guards.rs", source);

    assert_eq!(scan.handlers[0].guards[0].line, 3);
}

#[test]
fn a_method_self_parameter_is_not_a_guard() {
    let source = "#[get(\"/page\")]\npub async fn page(&self, auth: GuardResult<GuardAuth>) {\n    let _ = auth?;\n}\n";
    let scan = scan_handlers("guards.rs", source);

    assert_eq!(scan.handlers[0].guards.len(), 1);
    assert_eq!(scan.handlers[0].guards[0].parameter, "auth");
}

#[test]
fn the_guard_vocabulary_is_sorted_and_total() {
    // `KNOWN_GUARDS` and the class mapping are two structures naming the same
    // guards; this is what keeps them from drifting apart.
    let mut sorted = KNOWN.to_vec();
    sorted.sort_unstable();
    assert_eq!(KNOWN, sorted, "the guard list is kept sorted for review");

    for name in KNOWN {
        assert!(
            GuardClass::from_name(name).is_some(),
            "`{name}` is listed as a known guard but has no class"
        );
    }
    for class in GuardClass::ALL {
        assert!(
            KNOWN.contains(&class.guard_name()),
            "{} is a class but is not in KNOWN_GUARDS",
            class.guard_name()
        );
    }
}

#[test]
fn only_the_read_only_mode_guard_answers_405() {
    // The one guard that does not reject an unauthenticated request, and the
    // reason the policy has to say which guards enforce authentication rather
    // than counting guards.
    for class in GuardClass::ALL {
        assert_eq!(
            class.rejects_with_unauthorized(),
            *class != GuardClass::ReadOnlyMode,
            "{} rejects an unauthenticated request?",
            class.guard_name()
        );
    }
}

#[test]
fn every_request_guard_in_the_backend_is_a_known_guard() {
    // The guard list is a hand-written assumption about a codebase this crate
    // does not compile. Reading the `FromRequest` implementations is what makes it
    // an assumption with a failure mode instead of a guess: a new guard type stops
    // being recognised, and the auth policy then reports the route using it as
    // unguarded.
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits in <repo>/utils/openapi-sanity")
        .to_path_buf();
    let router = repository.join("backend").join("src").join("router");
    let mut declared: Vec<String> = Vec::new();
    let mut files = Vec::new();
    collect_rust_files(&router, &mut files);

    for file in &files {
        let source = std::fs::read_to_string(file).expect("a router file is readable");
        for line in source.lines() {
            let Some(rest) = line.trim().strip_prefix("impl<'r> FromRequest<'r> for ") else {
                continue;
            };
            if let Some(name) = rest.split(['<', ' ']).next() {
                declared.push(name.to_string());
            }
        }
    }
    declared.sort();
    declared.dedup();

    assert!(
        !declared.is_empty(),
        "no request guard was found in {}, so the scan found nothing to compare",
        router.display()
    );
    for name in &declared {
        assert!(
            KNOWN.contains(&name.as_str()),
            "{name} implements FromRequest but is not in KNOWN_GUARDS — add it with \
             the status it answers, or the routes using it are read as unguarded"
        );
    }
    for name in KNOWN {
        assert!(
            declared.iter().any(|declared| declared == name),
            "KNOWN_GUARDS names `{name}`, which no router file declares"
        );
    }
}

/// Every `.rs` file under a directory, sorted, so a scan is deterministic.
fn collect_rust_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()));

    for entry in entries {
        let path = entry.expect("a directory entry is readable").path();
        if path.is_dir() {
            collect_rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    files.sort();
}
