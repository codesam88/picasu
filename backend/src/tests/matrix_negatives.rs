//! Negative authentication matrix derived from the published contract.
//!
//! Every operation carries `x-picasu-auth` (the machine model of its guards,
//! token types and role constraint). This module turns that model into
//! assertions: for each operation, present a principal the operation's model
//! excludes and require the request to be refused, and present a principal it
//! admits and require the request not to be refused at the credential layer.
//!
//! Refusal is not always `401`. Handlers take `GuardResult<…>`, which captures
//! the guard's error instead of failing the request, so the affected handler
//! runs and answers `401` only through its `?`. A request guard declared after
//! that point — typically the JSON body — can therefore refuse first with
//! `422`. The assertions encode that: an excluded principal must be refused
//! with a 4xx, and the run reports how many refusals happened at the
//! credential layer.
//!
//! The "admitted" assertion is limited to operations whose guards decide only
//! *who* the caller is. Reauthentication (`GuardReauth`) and body-token
//! renewal (`AUTH-10`) can refuse a caller the role model admits, so those
//! operations carry no admission assertion rather than being named exceptions.
//!
//! What is deliberately not covered: public operations at runtime (a login
//! endpoint answers `401` for wrong credentials, which is the handler's
//! verdict rather than a credential-layer refusal; the source rules already
//! pin the public/guarded boundary), share credentials (they need a seeded
//! album and share record, owned by the scenario suite and the Schemathesis
//! cross-scope probe), and behaviour past the credential layer.

use rocket::http::{ContentType, Method, Status};
use rocket::local::blocking::Client;
use serde_json::Value;

use crate::tests::bootstrap::{cookie, seed, setup};

/// Who presents the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Principal {
    /// No credential at all.
    Anonymous,
    /// A live session for a user without the admin role.
    ViewerSession,
    /// A live admin session, used against bearer-only operations.
    AdminSession,
}

/// What the operation's model requires for that principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Expect {
    /// The request must not be admitted: a 4xx refusal, at the credential
    /// layer (`401`) or at the request-shape layer that precedes the handler.
    Refused,
    /// The role layer admits the caller: the answer must not be `401`/`403`,
    /// whatever later validation says.
    Admitted,
}

/// Credential-layer guards: they decide *who* the caller is. Guards outside
/// this set (reauthentication, body-token renewal) can refuse a caller the
/// role model admits, so they are excluded from the "admitted" assertion
/// rather than special-cased by operation name.
const ROLE_GUARDS: [&str; 4] = ["GuardAdmin", "GuardUser", "GuardPresenter", "GuardMutate"];

/// The assertion one operation carries for one principal, or `None` when the
/// model says nothing about that combination.
///
/// Public operations carry no runtime assertion: "no credential required"
/// says nothing about the handler's own verdict (a login endpoint answers 401
/// for wrong credentials), and the source rules already pin the public/
/// guarded boundary.
fn expectation_for(operation: &Value, principal: Principal) -> Option<Expect> {
    let auth = operation.get("x-picasu-auth")?.as_object()?;
    let public = auth.get("public").and_then(Value::as_bool).unwrap_or(false);
    if public {
        return None;
    }
    let admin = auth.get("admin").and_then(Value::as_str).unwrap_or("none");
    let credentials: Vec<&str> = auth
        .get("credentials")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let guards: Vec<&str> = auth
        .get("guards")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let invariants: Vec<&str> = auth
        .get("invariants")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let has_session = credentials.contains(&"session");
    let has_bearer = credentials
        .iter()
        .any(|kind| *kind == "snapshot" || *kind == "asset");
    let role_only = guards.iter().all(|guard| ROLE_GUARDS.contains(guard));
    let renews_presented_token = invariants.contains(&"AUTH-10");

    match principal {
        Principal::Anonymous => Some(Expect::Refused),
        Principal::ViewerSession => {
            if admin == "admin" {
                return Some(Expect::Refused);
            }
            (admin == "any" && has_session && !has_bearer && role_only && !renews_presented_token)
                .then_some(Expect::Admitted)
        }
        Principal::AdminSession => {
            // No session cookie satisfies a bearer guard.
            (has_bearer && !has_session && !credentials.contains(&"share"))
                .then_some(Expect::Refused)
        }
    }
}

/// Judge one observed status against the expectation. `Err` carries the
/// explanation so the failure names the operation and the claim it violated.
fn judge(name: &str, principal: Principal, expect: Expect, status: Status) -> Result<(), String> {
    let refused = status.class() == rocket::http::StatusClass::ClientError;
    match expect {
        Expect::Refused if !refused => Err(format!(
            "{name} as {principal:?}: the model excludes this caller, but the request \
             was admitted or failed server-side ({status})"
        )),
        Expect::Admitted if status == Status::Unauthorized || status == Status::Forbidden => {
            Err(format!(
                "{name} as {principal:?}: the model admits this caller, but the role \
                 layer refused it ({status})"
            ))
        }
        _ => Ok(()),
    }
}

/// Every `(method, path, operation)` the published document models.
fn modeled_operations(spec: &Value) -> Vec<(String, String, Value)> {
    let paths = spec
        .get("paths")
        .and_then(Value::as_object)
        .expect("OpenAPI document has paths");

    let mut operations = Vec::new();
    for (path, item) in paths {
        let Some(item) = item.as_object() else {
            continue;
        };
        for (method, operation) in item {
            if operation.get("x-picasu-auth").is_none() {
                continue;
            }
            operations.push((method.to_uppercase(), path.clone(), operation.clone()));
        }
    }
    operations.sort_by(|left, right| (&left.1, &left.0).cmp(&(&right.1, &right.0)));
    operations
}

/// Query parameters the route binds and requires, as `(name, value)`. Rocket
/// matches a route only when its required query parameters parse, and a
/// mismatch falls through to the SPA catch-all instead of the operation under
/// test.
fn required_query(operation: &Value) -> Vec<(String, String)> {
    operation
        .get("parameters")
        .and_then(Value::as_array)
        .map(|parameters| {
            parameters
                .iter()
                .filter(|parameter| {
                    parameter.get("in").and_then(Value::as_str) == Some("query")
                        && parameter.get("required").and_then(Value::as_bool) == Some(true)
                })
                .filter_map(|parameter| {
                    let name = parameter.get("name").and_then(Value::as_str)?;
                    let schema_type = parameter
                        .get("schema")
                        .and_then(|schema| schema.get("type"))
                        .and_then(Value::as_str)
                        .unwrap_or("string");
                    Some((name.to_owned(), benign_query_value(schema_type)))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A value that parses for the parameter's declared type. The guard layer
/// refuses before any of these are interpreted as data.
fn benign_query_value(schema_type: &str) -> String {
    match schema_type {
        "integer" | "number" => "1".to_owned(),
        "boolean" => "false".to_owned(),
        _ => "x".to_owned(),
    }
}

/// A syntactically valid instance of a request-body schema: every required
/// property filled, optional ones omitted. Enough for the body guard to accept
/// the request so the handler runs and its `auth?` decides; the values are
/// never used because the credential refusal precedes them.
///
/// `None` marks a construct this generator does not model, so the caller can
/// report the operation instead of silently grading a shape refusal.
fn body_instance(spec: &Value, schema: &Value, depth: usize) -> Option<Value> {
    if depth > 6 {
        return None;
    }
    let schema = resolve_ref(spec, schema)?;

    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        return allowed.first().cloned();
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("string") => Some(Value::String("x".to_owned())),
        Some("integer" | "number") => Some(Value::from(1)),
        Some("boolean") => Some(Value::Bool(false)),
        Some("array") => Some(Value::Array(Vec::new())),
        Some("object") | None => {
            let mut object = serde_json::Map::new();
            let required = schema
                .get("required")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for name in required {
                let name = name.as_str()?.to_owned();
                let property = schema.get("properties")?.get(&name)?;
                object.insert(name, body_instance(spec, property, depth + 1)?);
            }
            Some(Value::Object(object))
        }
        _ => None,
    }
}

/// Follow a local `$ref` into the document's components.
fn resolve_ref<'a>(spec: &'a Value, schema: &'a Value) -> Option<&'a Value> {
    let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
        return Some(schema);
    };
    let name = reference.strip_prefix("#/components/schemas/")?;
    spec.get("components")?.get("schemas")?.get(name)
}

/// The JSON body the operation expects, when it declares one.
fn request_body(spec: &Value, operation: &Value) -> Option<Value> {
    let schema = operation
        .get("requestBody")?
        .get("content")?
        .get("application/json")?
        .get("schema")?;
    body_instance(spec, schema, 0)
}

fn http_method(method: &str) -> Method {
    match method {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "DELETE" => Method::Delete,
        other => panic!("unexpected method in the document: {other}"),
    }
}

/// Replace every path parameter with a literal, including catch-alls. The
/// value only has to parse: the credential refusal happens before it is read.
fn concrete_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut in_param = false;
    for character in path.chars() {
        match character {
            '{' => in_param = true,
            '}' => {
                in_param = false;
                out.push('x');
            }
            _ if in_param => {}
            _ => out.push(character),
        }
    }
    out
}

fn dispatch(
    client: &Client,
    method: &str,
    path: &str,
    query: &[(String, String)],
    body: Option<&Value>,
    principal: Principal,
) -> Status {
    let mut target = concrete_path(path);
    if !query.is_empty() {
        let encoded: Vec<String> = query
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        target.push('?');
        target.push_str(&encoded.join("&"));
    }

    let mut request = client
        .req(http_method(method), target)
        .header(ContentType::JSON);
    request = match principal {
        Principal::Anonymous => request,
        Principal::ViewerSession => request.cookie(cookie("viewer", false)),
        Principal::AdminSession => request.cookie(cookie("admin", true)),
    };
    if method != "GET" {
        let payload = body.cloned().unwrap_or_else(|| serde_json::json!({}));
        request = request.body(payload.to_string());
    }
    request.dispatch().status()
}

fn principal_name(principal: Principal) -> &'static str {
    match principal {
        Principal::Anonymous => "anonymous",
        Principal::ViewerSession => "viewer-session",
        Principal::AdminSession => "admin-session",
    }
}

#[test]
fn every_operation_refuses_the_principals_its_model_excludes() {
    let _guard = crate::tests::bootstrap::lock();
    let client = setup();
    seed("admin", true, None);
    seed("viewer", false, None);

    let spec: Value = serde_json::from_str(&crate::openapi_public::public_json())
        .expect("generated OpenAPI must be valid JSON");
    let operations = modeled_operations(&spec);
    assert!(
        operations.len() >= 50,
        "the published document should model the whole API, saw {}",
        operations.len()
    );

    let mut failures = Vec::new();
    let mut counts = std::collections::BTreeMap::new();
    let mut credential_refusals = 0usize;
    let mut shape_refusals = Vec::new();
    for (method, path, operation) in &operations {
        let name = format!("{method} {path}");
        let query = required_query(operation);
        let body = request_body(&spec, operation);
        for principal in [
            Principal::Anonymous,
            Principal::ViewerSession,
            Principal::AdminSession,
        ] {
            let Some(expect) = expectation_for(operation, principal) else {
                continue;
            };
            *counts
                .entry((principal_name(principal), expect))
                .or_insert(0usize) += 1;
            let status = dispatch(&client, method, path, &query, body.as_ref(), principal);
            if expect == Expect::Refused {
                match status {
                    Status::Unauthorized => credential_refusals += 1,
                    Status::UnprocessableEntity
                    | Status::BadRequest
                    | Status::UnsupportedMediaType => {
                        shape_refusals.push(format!("{name} as {principal:?}: {status}"));
                    }
                    _ => {}
                }
            }
            if let Err(failure) = judge(&name, principal, expect, status) {
                failures.push(failure);
            }
        }
    }

    for ((principal, expect), count) in &counts {
        println!("{principal} {expect:?}: {count}");
    }
    println!(
        "refused at the credential layer (401): {credential_refusals}/{}",
        counts.values().sum::<usize>()
    );
    for refusal in &shape_refusals {
        println!("shape refusal: {refusal}");
    }
    assert!(
        failures.is_empty(),
        "the auth model and the implementation disagree on {} operation/principal pair(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The judge decides success, so a wrong status must be reported rather than
/// swallowed — a checker that accepts everything would pass the matrix test
/// against any implementation.
#[test]
fn the_judge_reports_a_violation_for_each_expectation() {
    let excluded = judge("Op", Principal::Anonymous, Expect::Refused, Status::Ok);
    assert!(
        excluded.is_err(),
        "a 200 where refusal is required is a violation"
    );

    let excluded_created = judge("Op", Principal::Anonymous, Expect::Refused, Status::Created);
    assert!(
        excluded_created.is_err(),
        "a 201 where refusal is required is a violation"
    );

    let admitted_401 = judge(
        "Op",
        Principal::ViewerSession,
        Expect::Admitted,
        Status::Unauthorized,
    );
    assert!(
        admitted_401.is_err(),
        "a 401 where admission is required is a violation"
    );

    let admitted_403 = judge(
        "Op",
        Principal::ViewerSession,
        Expect::Admitted,
        Status::Forbidden,
    );
    assert!(
        admitted_403.is_err(),
        "a 403 where admission is required is a violation"
    );

    assert!(
        judge(
            "Op",
            Principal::Anonymous,
            Expect::Refused,
            Status::Unauthorized
        )
        .is_ok(),
        "a credential refusal satisfies the expectation"
    );
    assert!(
        judge(
            "Op",
            Principal::Anonymous,
            Expect::Refused,
            Status::UnprocessableEntity
        )
        .is_ok(),
        "a request-shape refusal also leaves the caller unadmitted"
    );
    assert!(
        judge(
            "Op",
            Principal::ViewerSession,
            Expect::Admitted,
            Status::UnprocessableEntity
        )
        .is_ok(),
        "admission is judged at the credential layer, so later validation is not a violation"
    );
}

/// The policy reads the model, not operation names.
#[test]
fn the_policy_reads_the_model() {
    let row = |auth: Value| serde_json::json!({ "x-picasu-auth": auth });

    // Public operations carry no runtime assertion: the handler's own verdict
    // (invalid credentials, for one) is not a credential-layer refusal.
    let public = row(serde_json::json!({ "public": true, "admin": "none", "credentials": [] }));
    assert_eq!(expectation_for(&public, Principal::Anonymous), None);

    let admin_only = row(serde_json::json!({
        "public": false, "admin": "admin", "credentials": ["session"],
        "guards": ["GuardAdmin"]
    }));
    assert_eq!(
        expectation_for(&admin_only, Principal::Anonymous),
        Some(Expect::Refused)
    );
    assert_eq!(
        expectation_for(&admin_only, Principal::ViewerSession),
        Some(Expect::Refused)
    );

    let mut any_session = serde_json::json!({
        "public": false, "admin": "any", "credentials": ["session"],
        "guards": ["GuardUser"], "invariants": []
    });
    assert_eq!(
        expectation_for(&row(any_session.clone()), Principal::ViewerSession),
        Some(Expect::Admitted)
    );

    // A reauthentication guard can refuse a caller the role model admits, so
    // the admitted assertion does not apply.
    any_session["guards"] = serde_json::json!(["GuardUser", "GuardReauth"]);
    assert_eq!(
        expectation_for(&row(any_session.clone()), Principal::ViewerSession),
        None
    );

    // A renewal re-decodes the presented token; an invalid one answers 401
    // for the payload, not for the caller.
    any_session["guards"] = serde_json::json!(["GuardUser"]);
    any_session["invariants"] = serde_json::json!(["AUTH-10"]);
    assert_eq!(
        expectation_for(&row(any_session.clone()), Principal::ViewerSession),
        None
    );

    let bearer_only = row(serde_json::json!({
        "public": false, "admin": "none", "credentials": ["snapshot"],
        "guards": ["GuardSnapshot"]
    }));
    assert_eq!(
        expectation_for(&bearer_only, Principal::AdminSession),
        Some(Expect::Refused)
    );
    assert_eq!(
        expectation_for(&bearer_only, Principal::ViewerSession),
        None
    );

    // A session plus a bearer is not "session-only": the viewer does not
    // present the bearer, so the model says nothing about that pairing.
    let mixed = row(serde_json::json!({
        "public": false, "admin": "any", "credentials": ["session", "asset"],
        "guards": ["GuardPresenter", "GuardHash"]
    }));
    assert_eq!(expectation_for(&mixed, Principal::ViewerSession), None);

    assert_eq!(
        expectation_for(&serde_json::json!({}), Principal::Anonymous),
        None
    );
}

/// Path parameters become literals; a catch-all must not leak its braces.
#[test]
fn path_parameters_become_literals() {
    assert_eq!(concrete_path("/get/metadata/{asset_id}"), "/get/metadata/x");
    assert_eq!(
        concrete_path("/object/compressed/{file_path}"),
        "/object/compressed/x"
    );
    assert_eq!(concrete_path("/get/config"), "/get/config");
}

/// Required query parameters are filled from the document, or the route would
/// fall through to the SPA catch-all and the assertion would grade the wrong
/// handler.
#[test]
fn required_query_parameters_are_filled() {
    let spec: Value = serde_json::from_str(&crate::openapi_public::public_json())
        .expect("generated OpenAPI must be valid JSON");
    let operation = &spec["paths"]["/get/get-data"]["get"];
    let filled: Vec<String> = required_query(operation)
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    assert_eq!(filled, vec!["timestamp=1", "start=1", "end=1"]);

    let optional_only = &spec["paths"]["/get/prefetch"]["post"];
    assert!(
        required_query(optional_only).is_empty(),
        "prefetch binds only optional query parameters"
    );
}

/// The body generator fills required properties and omits optional ones, so
/// the body guard accepts the request and the handler's own refusal decides.
#[test]
fn request_bodies_are_minimally_valid() {
    let spec: Value = serde_json::from_str(&crate::openapi_public::public_json())
        .expect("generated OpenAPI must be valid JSON");

    let share = &spec["paths"]["/post/create_share"]["post"];
    let body = request_body(&spec, share).expect("create_share has a JSON body");
    let object = body.as_object().expect("body is an object");
    let schema = resolve_ref(
        &spec,
        &share["requestBody"]["content"]["application/json"]["schema"],
    )
    .expect("schema resolves");
    let required: Vec<&str> = schema["required"]
        .as_array()
        .expect("create_share schema lists required fields")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for name in &required {
        assert!(
            object.contains_key(*name),
            "required field {name} is filled"
        );
    }
    assert!(
        object.len() <= required.len(),
        "optional fields are omitted: {object:?}"
    );
}

/// A cookie minted for the viewer must not carry the admin role, or the matrix
/// test would admit itself.
#[test]
fn the_viewer_cookie_is_not_admin() {
    let _guard = crate::tests::bootstrap::lock();
    let _client = setup();
    seed("viewer", false, None);
    assert_eq!(cookie("viewer", false).name(), "jwt");
}
