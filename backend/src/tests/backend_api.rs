#![cfg(test)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::Value;

use snapfab::selection::{FixturePlan, RandomizableFormat, select};
use snapfab::{PhotoSpec, generate_batch};

use crate::DATA_PATH;
use crate::tests::bootstrap::*;
use crate::tests::fixtures::*;
use crate::tests::seeds::{DEFAULT_SET, active_seeds, env_override, seed_manifest};

// ── Variable interpolation ──

fn interpolate(s: &str, vars: &HashMap<String, String>) -> String {
    let mut result = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find('}') {
            let bare = &after[..end];
            let val = match bare.split_once(':') {
                // `${var:start:end}` substring slice, e.g. `${hash:0:2}`
                Some((var, range)) => {
                    let src = vars.get(var).cloned().unwrap_or_default();
                    let chars: Vec<char> = src.chars().collect();
                    match range.split_once(':') {
                        Some((s, e)) => {
                            let s = s.parse::<usize>().unwrap_or(0);
                            let e = e.parse::<usize>().unwrap_or(chars.len());
                            if s >= e || s >= chars.len() {
                                String::new()
                            } else {
                                chars[s.min(chars.len())..e.min(chars.len())]
                                    .iter()
                                    .collect()
                            }
                        }
                        None => src,
                    }
                }
                None => vars.get(bare).cloned().unwrap_or_default(),
            };
            result.push_str(&val);
            rest = &after[end + 1..];
        } else {
            result.push_str("${");
            rest = after;
        }
    }
    result.push_str(rest);
    result
}

fn interpolate_value(val: &Value, vars: &HashMap<String, String>) -> Value {
    match val {
        Value::String(s) => Value::String(interpolate(s, vars)),
        Value::Array(arr) => Value::Array(arr.iter().map(|v| interpolate_value(v, vars)).collect()),
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), interpolate_value(v, vars));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

// ── JSON path navigation ──

/// [`navigate_json`], but a path that does not resolve comes back as `None`
/// rather than as `Value::Null`.
///
/// The two are different questions — "is the key there?" and "what does it
/// hold?" — and `serde_json` answers both with the same `Value::Null`, so the
/// `absent` assertion cannot be written on top of `navigate_json`: a payload
/// carrying an explicit `null` would satisfy it. Everything else is deliberately
/// the same, index segments included, so the two functions cannot drift into
/// disagreeing about what a path means. An index that does not exist stays the
/// hard error `navigate_json` reports: `absent` is about a *key*, and a missing
/// array element inside an array that does exist is a typo worth stopping on.
fn resolve_json<'a>(root: &'a Value, field_path: &str) -> Option<&'a Value> {
    let mut current = root;
    for seg in field_path.split('.') {
        if seg.starts_with('[') && seg.ends_with(']') {
            let idx: usize = seg[1..seg.len() - 1]
                .parse()
                .unwrap_or_else(|_| panic!("invalid array index: {seg}"));
            current = &current[idx];
        } else {
            current = current.get(seg)?;
        }
    }
    Some(current)
}

fn navigate_json<'a>(root: &'a Value, field_path: &str) -> &'a Value {
    let mut current = root;
    for seg in field_path.split('.') {
        if seg.starts_with('[') && seg.ends_with(']') {
            let idx: usize = seg[1..seg.len() - 1]
                .parse()
                .unwrap_or_else(|_| panic!("invalid array index: {seg}"));
            current = &current[idx];
        } else {
            current = &current[seg];
        }
    }
    current
}

// ── Then assertion helpers ──

fn assert_json_field(root: &Value, key: &str, expected: &Value, vars: &HashMap<String, String>) {
    let field_path = key.strip_prefix("response.json.").unwrap_or(key);
    let actual = navigate_json(root, field_path);
    let expected = interpolate_value(expected, vars);
    assert_eq!(*actual, expected, "{key} mismatch");
}

fn assert_json_contains(root: &Value, key: &str, val: &Value, vars: &HashMap<String, String>) {
    let field_path = key.strip_prefix("response.json.").unwrap_or(key);
    let arr = navigate_json(root, field_path)
        .as_array()
        .unwrap_or_else(|| panic!("{key} must be an array"));
    let contained = val
        .as_object()
        .and_then(|o| o.get("contains"))
        .expect("{key}: expected {{contains: ...}}");
    let expected_val = interpolate_value(contained, vars);
    assert!(
        arr.contains(&expected_val),
        "{key} does not contain {expected_val}"
    );
}

/// The absence counterpart of [`assert_json_contains`], and the only way to
/// say "the cache is unchanged" about an edit that *removes* a value: asserting
/// that the old value is still there would pass for a cache that stored both
/// the removal and the addition.
fn assert_json_not_contains(root: &Value, key: &str, val: &Value, vars: &HashMap<String, String>) {
    let field_path = key.strip_prefix("response.json.").unwrap_or(key);
    let arr = navigate_json(root, field_path)
        .as_array()
        .unwrap_or_else(|| panic!("{key} must be an array"));
    let absent = val
        .as_object()
        .and_then(|o| o.get("not_contains"))
        .expect("{key}: expected {{not_contains: ...}}");
    let expected_val = interpolate_value(absent, vars);
    assert!(
        !arr.contains(&expected_val),
        "{key} contains {expected_val}, which it must not"
    );
}

fn assert_json_compare(root: &Value, key: &str, val: &Value, vars: &HashMap<String, String>) {
    let field_path = key.strip_prefix("response.json.").unwrap_or(key);
    let actual = navigate_json(root, field_path);
    let Some(actual) = actual.as_i64() else {
        panic!("{key}: compare requires an integer field, got {actual}");
    };
    let conds = val
        .as_object()
        .expect("compare value must be an object of operator -> value");
    for (op, expected_val) in conds {
        let expected = interpolate_value(expected_val, vars);
        let Some(expected) = expected.as_i64() else {
            panic!("{key}: compare requires integer expected values, got {expected_val}");
        };
        match op.as_str() {
            "<" => assert!(actual < expected, "{key}: {actual} is not < {expected}"),
            "<=" => assert!(actual <= expected, "{key}: {actual} is not <= {expected}"),
            ">" => assert!(actual > expected, "{key}: {actual} is not > {expected}"),
            ">=" => assert!(actual >= expected, "{key}: {actual} is not >= {expected}"),
            other => panic!("compare: unknown operator '{other}'"),
        }
    }
}

/// The `response.<path> absent` form: the dot-path must not resolve.
///
/// This is the only way to pin that a payload has *no* such key. Asserting the
/// key's value is `null` passes for a payload that omits it, and — because
/// `navigate_json` yields `Value::Null` for a key that is not there — it also
/// passes for a key that is not there at all, which is the same claim for two
/// different payloads. `resolve_json` is what tells them apart, so an explicit
/// `null` fails here: the key is present, and the assertion is about the key.
///
/// `absent` is a form marker, not a value: it is never interpolated, so a
/// scenario cannot accidentally compare a field against the literal string
/// `absent` and have that read as a pass. `not_null` is the same kind of marker
/// and carries the same trade-off — a response that really does carry the string
/// `"absent"` cannot be asserted with this form.
fn assert_json_absent(root: &Value, key: &str) {
    let field_path = key.strip_prefix("response.json.").unwrap_or(key);
    match resolve_json(root, field_path) {
        None => {}
        Some(Value::Null) => panic!(
            "{key} is present and explicitly null: `absent` requires the key to be missing \
             from the response, and null is not absent"
        ),
        Some(value) => {
            panic!("{key} is present as {value}, but `absent` requires it to be missing")
        }
    }
}

fn assert_all_absolute(root: &Value, key: &str) {
    let field_path = key.strip_prefix("response.json.").unwrap_or(key);
    let arr = navigate_json(root, field_path)
        .as_array()
        .unwrap_or_else(|| panic!("{key} must be an array"));
    for child in arr {
        let path_str = child
            .as_str()
            .unwrap_or_else(|| panic!("child must be a string"));
        assert!(
            std::path::Path::new(path_str).is_absolute(),
            "expected absolute path, got {path_str}"
        );
    }
}

fn assert_array_min_counts(root: &Value, val: &Value, vars: &HashMap<String, String>) {
    let pairs = val.as_object().expect("array_min_counts must be an object");
    let tags = root
        .as_array()
        .expect("response must be an array for array_min_counts");
    for (tag, count_val) in pairs {
        let min = count_val.as_u64().unwrap_or(0);
        let got = tags
            .iter()
            .find(|t| t["tag"].as_str() == Some(tag.as_str()))
            .and_then(|t| t["number"].as_u64())
            .unwrap_or(0);
        assert!(got >= min, "tag '{tag}': expected >= {min}, got {got}");
    }
    let _ = vars;
}

fn assert_array_where(root: &Value, val: &Value, vars: &HashMap<String, String>) {
    let aw = val
        .as_object()
        .expect("array_where value must be an object");
    let where_obj = aw
        .get("where")
        .and_then(|w| w.as_object())
        .expect("array_where requires 'where' object");

    let arr = root
        .as_array()
        .expect("response must be an array for array_where");

    let found = arr.iter().find(|item| {
        where_obj.iter().all(|(field, cond)| {
            let cond_interp = interpolate_value(cond, vars);
            navigate_json(item, field.as_str()).clone() == cond_interp
        })
    });

    let expect = aw
        .get("expect")
        .and_then(|e| e.as_str())
        .unwrap_or("present");
    match expect {
        "absent" => {
            assert!(
                found.is_none(),
                "array_where: expected no element matching where conditions, but found one"
            );
        }
        "present" => {
            let found = found.expect("no element matching array_where conditions");
            if let Some(assert_obj) = aw.get("assert").and_then(|a| a.as_object()) {
                for (field, expected) in assert_obj {
                    let expected_interp = interpolate_value(expected, vars);
                    assert_eq!(
                        navigate_json(found, field).clone(),
                        expected_interp,
                        "array_where {field} mismatch"
                    );
                }
            }
        }
        other => panic!("array_where expect must be 'present' or 'absent', got '{other}'"),
    }
}

// ── Run assertions on a response (status first, then body) ──
// Takes ownership because `into_bytes()` consumes the response.

fn check_status_assertions(
    response: &rocket::local::blocking::LocalResponse<'_>,
    then_items: &[Value],
    vars: &HashMap<String, String>,
) {
    for item in then_items {
        if let Some(code) = item["response.status"].as_i64() {
            assert_eq!(response.status(), Status::from_code(code as u16).unwrap(),);
        } else if let Some(code) = item["response.status_not"].as_i64() {
            assert_ne!(response.status(), Status::from_code(code as u16).unwrap(),);
        } else if let Some(obj) = item.as_object() {
            for (key, val) in obj {
                if let Some(name) = key.strip_prefix("response.header.") {
                    let expected = interpolate(
                        val.as_str()
                            .expect("response.header value must be a string"),
                        vars,
                    );
                    let actual = response
                        .headers()
                        .get(name)
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    assert_eq!(actual, expected, "response header {name}");
                }
            }
        }
    }
}

fn check_body_assertions(body_bytes: &[u8], then_items: &[Value], vars: &HashMap<String, String>) {
    let parsed: Value = serde_json::from_slice(body_bytes).expect("valid JSON response body");

    for item in then_items {
        if let Some(obj) = item.as_object() {
            for (key, val) in obj {
                if key.starts_with("response.json.") {
                    if val
                        .as_object()
                        .and_then(|o| o.get("not_contains"))
                        .is_some()
                    {
                        assert_json_not_contains(&parsed, key, val, vars);
                    } else if val.as_object().and_then(|o| o.get("contains")).is_some() {
                        assert_json_contains(&parsed, key, val, vars);
                    } else if val
                        .as_object()
                        .and_then(|o| o.get("all_absolute"))
                        .and_then(|v| v.as_bool())
                        == Some(true)
                    {
                        assert_all_absolute(&parsed, key);
                    } else if val.as_str() == Some("not_null") {
                        let field_path = key.strip_prefix("response.json.").unwrap_or(key);
                        let actual = navigate_json(&parsed, field_path);
                        assert!(!actual.is_null(), "{key}: expected not null, got null");
                    } else if val.as_str() == Some("absent") {
                        assert_json_absent(&parsed, key);
                    } else {
                        assert_json_field(&parsed, key, val, vars);
                    }
                } else if key == "array_min_counts" {
                    assert_array_min_counts(&parsed, val, vars);
                } else if key == "array_where" {
                    assert_array_where(&parsed, val, vars);
                } else if key == "compare" {
                    let pairs = val.as_object().expect("compare must be an object");
                    for (sub_key, sub_val) in pairs {
                        assert_json_compare(&parsed, sub_key, sub_val, vars);
                    }
                }
            }
        }
    }
}

/// Resolve a `file_exists` / `file_absent` / `file.contains` path against
/// `IMAGE_HOME`.
///
/// The documented form is `IMAGE_HOME`-relative with an optional leading `/`,
/// which is stripped before the join. Stripping a leading `/` is also what an
/// *absolute* path looks like once interpolated, so a `${var}` that yields one
/// (`${data_path}`) used to resolve under `<image_home>/<absolute path>/…`:
/// such a path can never exist, so `file_absent` passed no matter what the
/// handler under test did, and `file_exists` could only ever fail. An absolute
/// result is rejected here rather than silently resolving to a location that
/// means nothing.
fn image_home_path(
    raw: &str,
    assertion: &str,
    data: &Path,
    vars: &HashMap<String, String>,
) -> PathBuf {
    let resolved = interpolate(raw.trim_start_matches('/'), vars);
    assert!(
        !Path::new(&resolved).is_absolute(),
        "{assertion} path must be IMAGE_HOME-relative, but `{raw}` interpolates to the \
         absolute path {resolved}: joined onto IMAGE_HOME it would resolve to \
         <IMAGE_HOME>{resolved}, which can never exist and would pass vacuously"
    );
    data.join(&resolved)
}

/// The one asset id in the content hash's dupe group, read through the
/// test-only probe the `dup_*` scenarios already use.
///
/// [`assert_serves_image`] needs it only to fill the second claim of the token
/// it mints; the compressed route authorizes on the hash alone. Resolving it
/// through the probe rather than requiring the scenario to bind `asset_id_as`
/// keeps `serve_image_ok` usable with a bare `id_as`, and an empty group means
/// the asset is not indexed, which is a failure the assertion should report.
fn dupe_group_asset_id(client: &Client, hash: &str) -> String {
    let response = client
        .get(format!("/get/test/dupe-group/{hash}"))
        .cookie(auth_cookie(client))
        .dispatch();
    assert_eq!(
        response.status(),
        Status::Ok,
        "dupe-group probe for {hash}: expected 200"
    );
    let body: Value = serde_json::from_slice(&response.into_bytes().expect("dupe-group body"))
        .expect("dupe-group JSON");
    body.as_array()
        .and_then(|members| members.first())
        .and_then(|member| member["assetId"].as_str())
        .map_or_else(
            || {
                panic!(
                    "serve_image_ok: no indexed asset has content hash {hash}, so there is \
                     nothing to serve"
                )
            },
            ToOwned::to_owned,
        )
}

/// Assert the app still serves the compressed thumbnail for a content hash.
///
/// "Serving" is the route the frontend fetches:
/// `GET /object/compressed/<hash[0..2]>/<hash>.jpg`, authorized by the admin
/// cookie (`GuardShare`) and by a `ClaimsHash` bearer token whose `hash` claim
/// has to equal the hash in the path (`GuardHash`). The harness mints that
/// token instead of walking prefetch/get-data for it, because a scenario
/// binding a bare `id_as` has a hash and no asset id to look the asset up by,
/// and token issuance is what `token_hash_compressed_serving.yaml` covers.
///
/// Status alone is not enough: a handler can answer 200 with an error page. The
/// content type and the JPEG magic are therefore part of the assertion.
fn assert_serves_image(client: &Client, photo_var: &str, vars: &HashMap<String, String>) {
    let resolved = interpolate(photo_var, vars);
    let hash = match resolved.strip_prefix('$') {
        Some(name) => vars
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("serve_image_ok: unknown variable {name}")),
        None => resolved,
    };
    assert_eq!(
        hash.len(),
        64,
        "serve_image_ok: {hash:?} is not a 64-character content hash"
    );
    let asset_id: arrayvec::ArrayString<64> = dupe_group_asset_id(client, &hash)
        .parse()
        .expect("serve_image_ok: asset_id must fit an ArrayString<64>");
    let hash_claim: arrayvec::ArrayString<64> = hash.parse().expect("content hash");
    let token = crate::router::auth::ClaimsHash::new(
        hash_claim,
        asset_id,
        chrono::Utc::now().timestamp_millis(),
        false,
    )
    .encode();

    let response = client
        .get(format!("/object/compressed/{}/{hash}.jpg", &hash[..2]))
        .cookie(auth_cookie(client))
        .header(rocket::http::Header::new(
            "Authorization",
            format!("Bearer {token}"),
        ))
        .dispatch();

    assert_eq!(
        response.status(),
        Status::Ok,
        "serve_image_ok {hash}: the compressed route must answer 200"
    );
    let content_type = response
        .content_type()
        .map(|value| value.to_string())
        .unwrap_or_default();
    let bytes = response
        .into_bytes()
        .expect("serve_image_ok: response body");
    assert!(
        !bytes.is_empty(),
        "serve_image_ok {hash}: the response carries no bytes"
    );
    assert_eq!(
        content_type.split(';').next().unwrap_or_default().trim(),
        "image/jpeg",
        "serve_image_ok {hash}: expected an image/jpeg content type, got {content_type:?}"
    );
    assert_eq!(
        &bytes[..2],
        b"\xff\xd8",
        "serve_image_ok {hash}: the served bytes are not a JPEG"
    );
}

fn check_file_and_serve_assertions(
    then_items: &[Value],
    data: &Path,
    vars: &HashMap<String, String>,
    client: &Client,
) {
    for item in then_items {
        if let Some(file_path) = item["file_exists"].as_str() {
            let path = image_home_path(file_path, "file_exists", data, vars);
            assert!(path.exists(), "file should exist: {}", path.display());
        } else if let Some(file_path) = item["file_absent"].as_str() {
            let path = image_home_path(file_path, "file_absent", data, vars);
            assert!(!path.exists(), "file should be absent: {}", path.display());
        } else if let Some(photo_var) = item["thumb_absent"].as_str() {
            let bare = photo_var.trim_start_matches('$');
            let hash = vars
                .get(bare)
                .unwrap_or_else(|| panic!("thumb_absent: unknown var {photo_var}"));
            let prefix = &hash[..2];
            let thumb = DATA_PATH
                .get()
                .expect("DATA_PATH set")
                .join(format!("object/compressed/{prefix}/{hash}.jpg"));
            assert!(
                !thumb.exists(),
                "thumbnail should be absent: {}",
                thumb.display()
            );
        } else if let Some(photo_var) = item["thumb_exists"].as_str() {
            let bare = photo_var.trim_start_matches('$');
            let hash = vars
                .get(bare)
                .unwrap_or_else(|| panic!("thumb_exists: unknown var {photo_var}"));
            let prefix = &hash[..2];
            let thumb = DATA_PATH
                .get()
                .expect("DATA_PATH set")
                .join(format!("object/compressed/{prefix}/{hash}.jpg"));
            assert!(
                thumb.exists(),
                "thumbnail should exist: {}",
                thumb.display()
            );
        } else if let Some(photo_var) = item["serve_image_ok"].as_str() {
            assert_serves_image(client, photo_var, vars);
        } else if let Some(file_path) = item["file.contains"].as_str() {
            let path = image_home_path(file_path, "file.contains", data, vars);
            let text = item["text"]
                .as_str()
                .unwrap_or_else(|| panic!("file.contains: missing 'text' field"));
            let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!("file.contains: failed to read {}: {e}", path.display())
            });
            assert!(
                content.contains(text),
                "file.contains: {} does not contain {text:?}.\nFile content:\n{content}",
                path.display()
            );
        } else if let Some(file_path) = item["file.not_contains"].as_str() {
            let path = image_home_path(file_path, "file.not_contains", data, vars);
            let text = item["text"]
                .as_str()
                .unwrap_or_else(|| panic!("file.not_contains: missing 'text' field"));
            let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!("file.not_contains: failed to read {}: {e}", path.display())
            });
            assert!(
                !content.contains(text),
                "file.not_contains: {} unexpectedly contains {text:?}.\nFile content:\n{content}",
                path.display()
            );
        }
    }
}

// ── Calc expression ──

fn calc_expression(expr: &str, vars: &HashMap<String, String>) -> String {
    if let Some((var_part, suffix)) = expr.split_once('+') {
        let var_name = var_part.trim_start_matches("${").trim_end_matches('}');
        let val: i64 = vars
            .get(var_name)
            .unwrap_or_else(|| panic!("calc: unknown var ${{{var_name}}}"))
            .parse()
            .expect("calc: var is not a number");
        let n: i64 = suffix.trim().parse().expect("calc: suffix is not a number");
        (val + n).to_string()
    } else {
        interpolate(expr, vars)
    }
}

// ── Execute a single when call ──

fn execute_call<'c>(
    call: &Value,
    vars: &HashMap<String, String>,
    client: &'c Client,
) -> rocket::local::blocking::LocalResponse<'c> {
    let call_str = call["call"].as_str().expect("when.call is required");
    let body_val = call.get("body");
    let auth = call.get("auth").and_then(|v| v.as_bool()).unwrap_or(true);

    let parts: Vec<&str> = call_str.splitn(2, ' ').collect();
    let method = parts[0];
    let path = parts[1];

    let body_str = if let Some(raw) = call.get("raw_body").and_then(|v| v.as_str()) {
        interpolate(raw, vars)
    } else {
        body_val
            .map(|b| {
                let interp = interpolate_value(b, vars);
                serde_json::to_string(&interp).unwrap_or_default()
            })
            .unwrap_or_default()
    };

    let path_interp = interpolate(path, vars);
    let path_interp: &'static str = Box::leak(path_interp.into_boxed_str());

    let mut req = match method.to_uppercase().as_str() {
        "GET" => client.get(path_interp),
        "POST" => client.post(path_interp),
        "PUT" => client.put(path_interp),
        "DELETE" => client.delete(path_interp),
        other => panic!("unsupported HTTP method: {other}"),
    };

    if auth {
        req = req.cookie(auth_cookie(client));
    }

    if let Some(headers) = call.get("headers").and_then(|h| h.as_object()) {
        for (hdr_name, hdr_val) in headers {
            let val_str = match hdr_val {
                Value::String(s) => interpolate(s, vars),
                other => other.to_string(),
            };
            req = req.header(rocket::http::Header::new(hdr_name.clone(), val_str));
        }
    }

    if method.to_uppercase().as_str() != "GET" {
        req = req.header(ContentType::JSON);
        if !body_str.is_empty() {
            req = req.body(body_str);
        }
    }

    req.dispatch()
}

// ── Multipart upload body builder ──

fn build_upload_multipart(
    file_data: &[u8],
    filename: &str,
    last_modified: u64,
    content_type: &str,
) -> (Vec<u8>, String) {
    let boundary = "----picasu-test-upload-boundary";
    let mut body = Vec::new();

    // File part
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"file\"; filename=\"");
    body.extend_from_slice(filename.as_bytes());
    body.extend_from_slice(b"\"\r\n");
    body.extend_from_slice(b"Content-Type: ");
    body.extend_from_slice(content_type.as_bytes());
    body.extend_from_slice(b"\r\n\r\n");
    body.extend_from_slice(file_data);
    body.extend_from_slice(b"\r\n");

    // lastModified part
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"lastModified\"\r\n\r\n");
    body.extend_from_slice(last_modified.to_string().as_bytes());
    body.extend_from_slice(b"\r\n");

    // Close
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--\r\n");

    (body, boundary.to_string())
}

// ── Execute an upload call ──

fn execute_upload<'c>(
    item: &Value,
    vars: &HashMap<String, String>,
    client: &'c Client,
) -> rocket::local::blocking::LocalResponse<'c> {
    let upload = &item["upload"];

    let file_path = upload["file"].as_str().expect("upload.file is required");
    let file_path = interpolate(file_path, vars);

    let filename = upload["filename"]
        .as_str()
        .map(|s| interpolate(s, vars))
        .unwrap_or_else(|| {
            std::path::Path::new(&file_path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        });

    let image_home = test_image_home();
    let full_path = image_home.join(file_path.trim_start_matches('/'));
    let file_data = std::fs::read(&full_path)
        .unwrap_or_else(|e| panic!("upload.file not found at {}: {e}", full_path.display()));

    let last_modified = upload["last_modified"].as_u64().unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    });

    // Interpolated like every other string in the harness: a randomized scenario
    // sends the capability manifest's MIME for the format it selected, and the
    // upload path derives the stored extension from this header.
    let content_type = upload["content_type"]
        .as_str()
        .map(|value| interpolate(value, vars))
        .unwrap_or_else(|| "image/jpeg".to_string());

    let (body, boundary) =
        build_upload_multipart(&file_data, &filename, last_modified, &content_type);

    let mut url = "/upload".to_string();
    let mut query_parts: Vec<String> = Vec::new();

    if let Some(album) = upload["target_album"].as_str() {
        let album = interpolate(album, vars);
        query_parts.push(format!("presigned_album_id_opt={album}"));
    }

    if let Some(oc) = upload["on_conflict"].as_str() {
        query_parts.push(format!("on_conflict={oc}"));
    }

    if let Some(ar) = upload["auto_rename"].as_bool() {
        query_parts.push(format!("auto_rename={ar}"));
    }

    if !query_parts.is_empty() {
        url.push('?');
        url.push_str(&query_parts.join("&"));
    }

    let url: &'static str = Box::leak(url.into_boxed_str());

    let auth = upload.get("auth").and_then(|v| v.as_bool()).unwrap_or(true);

    let mut req = client.post(url);
    if auth {
        req = req.cookie(auth_cookie(client));
    }
    let req = req
        .header(rocket::http::Header::new(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        ))
        .body(body);

    req.dispatch()
}

// ── Pinned fixture placement ──

/// Repository root, the base the capability manifest's fixture paths are
/// recorded against. `CARGO_MANIFEST_DIR` is `<repo>/backend`.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| {
            panic!(
                "backend manifest dir {} has no parent",
                env!("CARGO_MANIFEST_DIR")
            )
        })
        .to_path_buf()
}

/// The IMAGE_HOME-relative destination of a `fixture` given item.
fn pinned_fixture_destination(item: &Value) -> String {
    let fixture = item["fixture"]
        .as_object()
        .unwrap_or_else(|| panic!("fixture must be an object with `source` and `destination`"));
    fixture["destination"]
        .as_str()
        .unwrap_or_else(|| panic!("fixture.destination is required"))
        .trim_start_matches('/')
        .to_string()
}

/// Copy a checked-in fixture into `IMAGE_HOME`.
///
/// A `fixture` item (`{source: <id>, destination: <path>}`) is the binary-safe
/// counterpart of `raw_file`, which can only write UTF-8 text and therefore
/// cannot express a real image. `source` names an entry of the capability
/// manifest, so the bytes come from a file whose SHA-256 the manifest tests
/// verify; the scenario never embeds fixture bytes itself.
fn place_pinned_fixture(item: &Value, image_home: &Path) {
    let id = item["fixture"]["source"]
        .as_str()
        .unwrap_or_else(|| panic!("fixture.source must be a fixture id from capabilities.json"));
    let destination = pinned_fixture_destination(item);
    copy_pinned_fixture(id, &destination, image_home);
}

/// Copy the checked-in bytes of manifest fixture `id` to an IMAGE_HOME-relative
/// `destination`.
///
/// Shared by the `fixture` given step and by `random_media`, so a randomized
/// scenario places a pinned format exactly the way a deterministic one does.
fn copy_pinned_fixture(id: &str, destination: &str, image_home: &Path) {
    let entry = snapfab::capabilities::capabilities()
        .fixture_by_id(id)
        .unwrap_or_else(|| panic!("fixture id `{id}` is not declared in the capability manifest"));
    let source = repo_root().join(&entry.path);
    let bytes = std::fs::read(&source)
        .unwrap_or_else(|e| panic!("read fixture {} from {}: {e}", id, source.display()));
    let target = image_home.join(destination);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|e| panic!("create dir for fixture {id} to {destination}: {e}"));
    }
    std::fs::write(&target, &bytes)
        .unwrap_or_else(|e| panic!("write fixture {id} to {destination}: {e}"));
}

// ── Randomized scenarios ──

/// A scenario that carries a `randomize:` block, resolved against
/// `backend/tests/seeds.json`.
struct RandomizePlan {
    /// The seed set to run, defaulting to the fixed CI set.
    set: String,
}

impl PartialEq for RandomizePlan {
    fn eq(&self, other: &Self) -> bool {
        self.set == other.set
    }
}

impl std::fmt::Debug for RandomizePlan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RandomizePlan")
            .field("set", &self.set)
            .finish()
    }
}

/// Read a scenario's `randomize:` block.
///
/// The block carries the seed set only, so a randomized run is fully described
/// by a checked-in file: there is no way to write a scenario whose behavior
/// depends on a seed nobody recorded. `seeds` is optional and defaults to the
/// fixed CI set, and an unrecognised key is an error rather than a silent
/// default — `randomize: {seed: 42}` reads like it should do something and must
/// not quietly run the CI set instead.
fn parse_randomize(scenario: &Value) -> Result<Option<RandomizePlan>, String> {
    let Some(block) = scenario.get("randomize").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let object = block
        .as_object()
        .ok_or_else(|| "randomize must be a mapping with a `seeds` key".to_string())?;
    for key in object.keys() {
        if key != "seeds" {
            return Err(format!(
                "randomize: unknown key `{key}`; the only key is `seeds`, the name of a \
                 set in backend/tests/seeds.json (ci, nightly)"
            ));
        }
    }
    let set = match object.get("seeds") {
        None => DEFAULT_SET.to_string(),
        Some(value) => value
            .as_str()
            .ok_or_else(|| "randomize: seeds must be the name of a seed set".to_string())?
            .to_string(),
    };
    Ok(Some(RandomizePlan { set }))
}

/// The line a reader finds for a randomized run: in `--nocapture` output, and in
/// the failure message of a red run. It names the scenario, the run's position in
/// the seed set, and the seed's resolution.
fn seed_banner(scenario: &str, position: usize, total: usize, log_line: &str) -> String {
    format!("[randomized] {scenario} run {position}/{total}: {log_line}")
}

/// Re-raise a panic with `banner` attached.
fn fail_with_banner(banner: &str, payload: Box<dyn std::any::Any + Send>) -> ! {
    std::panic::panic_any(banner_panic_message(banner, &payload))
}

/// The message a red randomized run reports: the banner, then the failure it
/// wraps, so `cargo test`'s captured output names the seed even though the
/// scenario's own `println!` never reached the terminal.
fn banner_panic_message(banner: &str, payload: &(dyn std::any::Any + Send)) -> String {
    let detail = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("<non-string panic payload>");
    format!("{banner}\n{detail}")
}

/// One planned run of a randomized scenario: a seed and the format it resolves
/// to. Returning the plan as data (rather than looping inside the runner) is
/// what makes "one run per seed" a testable claim instead of an observation.
fn planned_runs(
    plan: &RandomizePlan,
    override_value: Option<&str>,
) -> Result<Vec<(u64, RandomizableFormat)>, String> {
    let seeds = active_seeds(seed_manifest(), &plan.set, override_value)
        .map_err(|error| error.to_string())?;
    let capabilities = snapfab::capabilities::capabilities();
    seeds
        .into_iter()
        .map(|seed| {
            let selected =
                select(seed, capabilities).map_err(|error| format!("seed {seed}: {error}"))?;
            Ok((seed, selected))
        })
        .collect()
}

/// Run one randomized scenario once per seed of its set.
///
/// Each pass resets all shared backend state, so the seeds are independent runs
/// of the same scenario rather than steps of one flow. The seed decides the
/// input format; the scenario's assertions are the same for every seed.
fn run_randomized_scenario(name: &str, scenario: &Value, plan: &RandomizePlan) {
    let runs = planned_runs(plan, env_override().as_deref())
        .unwrap_or_else(|error| panic!("randomized scenario `{name}`: {error}"));
    assert!(
        !runs.is_empty(),
        "randomized scenario `{name}` has no seeds to run"
    );
    let total = runs.len();

    for (index, (seed, selected)) in runs.iter().enumerate() {
        let banner = seed_banner(name, index + 1, total, &selected.log_line(*seed));
        println!("{banner}");

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            interpret_scenario(scenario, Some(selected));
        }));
        if let Err(payload) = outcome {
            fail_with_banner(&banner, payload);
        }
    }
}

/// Geometry the harness renders a generated format at.
///
/// Only a non-degenerate size is worth materialising: the point of a randomized
/// pass is that the file behaves like a real upload, and a 2x2 minimal image
/// would exercise the degenerate end of the thumbnail and metadata paths instead.
/// Pinned formats keep their fixture's own geometry (48x32).
const RANDOM_MEDIA_WIDTH: u32 = 64;
const RANDOM_MEDIA_HEIGHT: u32 = 48;

/// The IMAGE_HOME-relative path a `random_media` given item writes.
///
/// The scenario supplies the stem and the harness appends the selected format's
/// canonical extension, so the file lands on disk under an extension the indexer
/// accepts for the format it actually contains — the one thing a randomized
/// scenario must not get wrong by writing `.jpg` bytes under a `.tif` name.
fn random_media_destination(item: &Value, selected: &RandomizableFormat) -> String {
    let stem = item["random_media"]
        .as_str()
        .unwrap_or_else(|| panic!("random_media must be a path stem, got: {item}"))
        .trim_start_matches('/')
        .trim_end_matches('.');
    format!("{stem}.{}", selected.extension)
}

// ── Deterministic byte transforms on an already-placed file ──

/// Change a placed file's or directory's POSIX permissions, for the scenarios
/// that need a filesystem to refuse a read or a write.
///
/// `{path, octal}`, IMAGE_HOME-relative, `octal` in the `chmod` spelling
/// (`"0500"`, `"0200"`). A directory is the way to stop a sidecar write; a
/// single file is the way to stop a sidecar *read* while its directory stays
/// writable, which is a different failure on a different side of the same
/// request. Both depend on the test process not being root, which is what the
/// runner and CI are — a root run would silently make these scenarios assert
/// nothing — so the path is remembered through
/// [`crate::tests::bootstrap::remember_path_mode`] and the harness puts it back
/// even when a scenario panics.
#[cfg(unix)]
fn set_path_mode(item: &Value, data: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let spec = &item["chmod"];
    let rel = spec["path"]
        .as_str()
        .unwrap_or_else(|| panic!("chmod.path is required, got: {item}"))
        .trim_start_matches('/')
        .to_string();
    let octal = spec["octal"]
        .as_str()
        .unwrap_or_else(|| panic!("chmod.octal is required for {rel}"))
        .trim_start_matches("0o")
        .to_string();
    let mode = u32::from_str_radix(&octal, 8)
        .unwrap_or_else(|e| panic!("chmod.octal {octal:?} is not octal: {e}"));
    let path = data.join(&rel);
    assert!(path.exists(), "chmod: {} does not exist", path.display());
    crate::tests::bootstrap::remember_path_mode(&path);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
        .unwrap_or_else(|e| panic!("chmod: mode {octal} on {}: {e}", path.display()));
}

#[cfg(not(unix))]
fn set_path_mode(_item: &Value, _data: &Path) {
    panic!("the chmod verb changes POSIX permissions, which this platform has no equivalent for");
}

/// A `truncate_file` given item: keep only the first `keep` bytes of `path`.
struct TruncateFile {
    path: String,
    keep: usize,
}

/// A `patch_file` given item: replace the first occurrence of `find` with
/// `replace`, in place, so the file keeps its length.
struct PatchFile {
    path: String,
    find: Vec<u8>,
    replace: Vec<u8>,
}

fn parse_truncate_file(item: &Value) -> TruncateFile {
    let path = item["truncate_file"]["path"]
        .as_str()
        .unwrap_or_else(|| panic!("truncate_file.path is required"))
        .trim_start_matches('/')
        .to_string();
    let keep = item["truncate_file"]["bytes"]
        .as_u64()
        .unwrap_or_else(|| panic!("truncate_file.bytes is required: {path}"));
    TruncateFile {
        path,
        keep: usize::try_from(keep).expect("truncate_file.bytes must fit in usize"),
    }
}

fn parse_patch_file(item: &Value) -> PatchFile {
    let path = item["patch_file"]["path"]
        .as_str()
        .unwrap_or_else(|| panic!("patch_file.path is required"))
        .trim_start_matches('/')
        .to_string();
    let find_hex = item["patch_file"]["find_hex"]
        .as_str()
        .unwrap_or_else(|| panic!("patch_file.find_hex is required: {path}"));
    let replace_hex = item["patch_file"]["replace_hex"]
        .as_str()
        .unwrap_or_else(|| panic!("patch_file.replace_hex is required: {path}"));
    PatchFile {
        find: hex_bytes(find_hex, "patch_file.find_hex"),
        replace: hex_bytes(replace_hex, "patch_file.replace_hex"),
        path,
    }
}

fn hex_bytes(hex: &str, what: &str) -> Vec<u8> {
    let hex = hex.trim();
    assert!(
        hex.len().is_multiple_of(2),
        "{what} needs an even number of hex digits, got {hex:?}"
    );
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .unwrap_or_else(|_| panic!("{what} is not hex: {:?}", &hex[i..i + 2]))
        })
        .collect()
}

/// Apply the scenario's byte transforms to files the `given` phase already
/// wrote or generated.
///
/// `raw_file` writes UTF-8 text and `fixture`/`photo` copy or encode whole,
/// decodable files, so neither can express a *damaged* binary: a truncated
/// JPEG, a truncated video, a JPEG whose EXIF block no longer parses. Those
/// cases are derived here instead of checked in as a second blob, so the
/// damaged bytes still descend from a source whose SHA-256 the manifest
/// verifies — a new fixture file would be a second artifact to keep in sync
/// and could drift from the good one it was cut from.
///
/// The transforms run after `generate_batch` and after the `duplicate_of`
/// copies, and before the given phase's scan, so the indexer sees the damaged
/// bytes during its first pass rather than as a later change. A `when:`
/// `truncate_file` step reuses this to damage a file that is already indexed.
fn apply_file_transforms(data: &Path, truncations: &[TruncateFile], patches: &[PatchFile]) {
    for truncation in truncations {
        let path = data.join(&truncation.path);
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("truncate_file: read {}: {e}", truncation.path));
        assert!(
            truncation.keep < bytes.len(),
            "truncate_file: {} is already {} bytes, so keeping {} would be a no-op",
            truncation.path,
            bytes.len(),
            truncation.keep
        );
        std::fs::write(&path, &bytes[..truncation.keep])
            .unwrap_or_else(|e| panic!("truncate_file: write {}: {e}", truncation.path));
    }

    for patch in patches {
        let path = data.join(&patch.path);
        let mut bytes =
            std::fs::read(&path).unwrap_or_else(|e| panic!("patch_file: read {}: {e}", patch.path));
        assert_eq!(
            patch.find.len(),
            patch.replace.len(),
            "patch_file: {} must not change the file length: {} find bytes vs {} replace bytes",
            patch.path,
            patch.find.len(),
            patch.replace.len()
        );
        let at = bytes
            .windows(patch.find.len())
            .position(|window| window == patch.find)
            .unwrap_or_else(|| {
                panic!(
                    "patch_file: {} does not contain the find pattern, so the \
                     corruption would be a silent no-op",
                    patch.path
                )
            });
        bytes[at..at + patch.find.len()].copy_from_slice(&patch.replace);
        std::fs::write(&path, &bytes)
            .unwrap_or_else(|e| panic!("patch_file: write {}: {e}", patch.path));
    }
}

/// Read a `when:` step's `wait_index` expectation.
///
/// `wait_index: true` — and the explicit `wait_index: {expect: completed}` — both
/// mean the default: wait for a scan that completes. `failed` is the one
/// distinct expectation, and it is spelled out rather than implied by a missing
/// key, so a scenario that asserts a failed scan says so in the file.
fn parse_wait_index(item: &Value) -> Option<IndexExpect> {
    match item.get("wait_index")? {
        Value::Bool(true) => Some(IndexExpect::Completed),
        Value::Bool(false) => None,
        Value::String(state) => Some(parse_expected_index_state(state)),
        Value::Object(block) => Some(parse_expected_index_state(
            block
                .get("expect")
                .and_then(Value::as_str)
                .unwrap_or_else(|| {
                    panic!("wait_index object needs `expect: completed` or `expect: failed`")
                }),
        )),
        other => panic!(
            "wait_index: {other} is not `true`, `false`, \"completed\", \"failed\", or an \
             object with `expect`"
        ),
    }
}

fn parse_expected_index_state(state: &str) -> IndexExpect {
    match state {
        "completed" => IndexExpect::Completed,
        "failed" => IndexExpect::Failed,
        other => panic!(
            "wait_index: unknown expected index state {other:?}; expected \"completed\" or \
             \"failed\""
        ),
    }
}

// ── Dispatch a when item to call or upload ──

fn dispatch_when_item<'c>(
    item: &Value,
    vars: &HashMap<String, String>,
    client: &'c Client,
) -> rocket::local::blocking::LocalResponse<'c> {
    if let Some(expect) = parse_wait_index(item) {
        wait_for_album_index(client, 30000, expect);
        let cookie = auth_cookie(client);
        return client.get("/get/index/status").cookie(cookie).dispatch();
    }
    if item.get("upload").is_some() {
        execute_upload(item, vars, client)
    } else if item.get("chmod").is_some() {
        // Change the permissions of a path the given phase placed, so a
        // scenario can watch the server meet a filesystem that refuses a read
        // or a write.
        set_path_mode(item, &test_image_home());
        // Return a status-200 probe response so this branches cleanly in the
        // `when` flow like any other call/upload verb.
        let cookie = auth_cookie(client);
        client.get("/get/index/status").cookie(cookie).dispatch()
    } else if item.get("truncate_file").is_some() {
        // Cut a file the given phase indexed down to its first `bytes`, with
        // the given-block transform's semantics. `write_file` cannot express
        // this: it writes UTF-8 text, so its content can never carry a media
        // signature, and a file whose bytes match no signature is skipped
        // before it is matched instead of failing. A scenario needs to reach
        // "matched, then undecodable" in `when` to observe an album index
        // whose every matched file failed.
        let truncation = parse_truncate_file(item);
        let home = test_image_home();
        apply_file_transforms(&home, std::slice::from_ref(&truncation), &[]);
        // Return a status-200 probe response so this branches cleanly in the
        // `when` flow like any other call/upload/write verb.
        let cookie = auth_cookie(client);
        client.get("/get/index/status").cookie(cookie).dispatch()
    } else if item.get("write_file").is_some() {
        // Overwrite a file's bytes AFTER it has been indexed, so a scenario can
        // exercise genuine verify paths (e.g. content-change verify-mismatch).
        // Paths are IMAGE_HOME-relative with a leading slash.
        let path = item["write_file"].as_str().expect("write_file is required");
        let full = test_image_home().join(path.trim_start_matches('/'));
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("create dir for write_file {path}: {e}"));
        }
        let content = item["content"].as_str().unwrap_or("");
        std::fs::write(&full, content).unwrap_or_else(|e| panic!("write_file {path}: {e}"));
        // Return a status-200 probe response so this branches cleanly in the
        // `when` flow like any other call/upload verb.
        let cookie = auth_cookie(client);
        client.get("/get/index/status").cookie(cookie).dispatch()
    } else if let Some(dup) = item.get("duplicate_of") {
        // Binary-safe overwrite in `when`: reuses the given-block
        // duplicate_of schema ({source, destination}) with the same fs::copy
        // semantics, so a scenario can replace a file's bytes with valid
        // (but different) media the watcher will re-index.
        let src = dup["source"]
            .as_str()
            .expect("duplicate_of.source is required")
            .trim_start_matches('/')
            .to_string();
        let dst = dup["destination"]
            .as_str()
            .expect("duplicate_of.destination is required")
            .trim_start_matches('/')
            .to_string();
        let home = test_image_home();
        let src_path = home.join(&src);
        let dst_path = home.join(&dst);
        assert!(
            src_path.exists(),
            "duplicate_of source does not exist: {src}"
        );
        if let Some(parent) = dst_path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("create dir for duplicate_of {dst}: {e}"));
        }
        std::fs::copy(&src_path, &dst_path)
            .unwrap_or_else(|e| panic!("duplicate_of copy {src} -> {dst}: {e}"));
        // Return a status-200 probe response so this branches cleanly in the
        // `when` flow like any other call/upload/write verb.
        let cookie = auth_cookie(client);
        client.get("/get/index/status").cookie(cookie).dispatch()
    } else {
        execute_call(item, vars, client)
    }
}

// ── Process capture block ──

fn process_capture(call: &Value, body_bytes: &[u8], vars: &mut HashMap<String, String>) {
    if let Some(capture) = call.get("capture").and_then(|c| c.as_object()) {
        if !capture.is_empty() {
            let body: Value = serde_json::from_slice(body_bytes).expect("capture: valid JSON");

            for (var_name, json_path_val) in capture {
                let bare = var_name.trim_start_matches('$');
                let field = json_path_val.as_str().unwrap_or_else(|| {
                    panic!("capture {var_name}: value must be a string JSON path")
                });
                let access_path = field.strip_prefix("response.").unwrap_or(field);
                let val = navigate_json(&body, access_path);
                let str_val = val
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| val.to_string());
                vars.insert(bare.to_string(), str_val);
            }
        }
    }
}

// ── Process calc block ──

fn process_calc(call: &Value, vars: &mut HashMap<String, String>) {
    if let Some(calc) = call.get("calc").and_then(|c| c.as_object()) {
        for (var_name, expr_val) in calc {
            let bare = var_name.trim_start_matches('$');
            let expr = expr_val
                .as_str()
                .unwrap_or_else(|| panic!("calc {var_name}: value must be a string"));
            let result = calc_expression(expr, vars);
            vars.insert(bare.to_string(), result);
        }
    }
}

// ── Check if a then item has JSON assertion keys ──

/// Whether any item in a `then:` block reads the response body, so the caller
/// knows it has to consume the response before `capture` can read it too.
fn then_needs_body(then_items: &[Value]) -> bool {
    then_items.iter().any(has_json_assertions)
}

fn has_json_assertions(item: &Value) -> bool {
    item.as_object().is_some_and(|m| {
        m.keys().any(|k| {
            k.starts_with("response.json.")
                || k == "array_min_counts"
                || k == "array_where"
                || k == "compare"
        })
    })
}

// ── Interpreter main logic ──

/// Interpret a scenario, optionally against a selected format.
///
/// `selection` is `Some` for a randomized run: it is the format the run's seed
/// resolved to, and it is what a `random_media` given item materialises. It is
/// `None` for a deterministic scenario, where a format never varies.
fn interpret_scenario(scenario: &Value, selection: Option<&RandomizableFormat>) {
    let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let _ = &*TEST_ENV;
    reset_backend_state();
    let data = test_image_home();
    set_image_home(data.clone());

    let given = scenario["given"].as_array();
    let then_items = scenario["then"].as_array().expect("then must be an array");

    let has_given = given.map(|items| !items.is_empty()).unwrap_or(false);
    let needs_data = has_given
        || then_items
            .iter()
            .any(|item| item.get("file_exists").is_some() || item.get("file_absent").is_some());

    let mut vars: HashMap<String, String> = HashMap::new();

    let has_config_item = given
        .map(|items| items.iter().any(|item| item.get("config").is_some()))
        .unwrap_or(false);

    if needs_data {
        if scenario.to_string().contains("${data_path}") {
            let data_path = data.to_string_lossy().to_string();
            vars.insert("data_path".to_string(), data_path);
        }

        if let Some(items) = given {
            let has_non_config = items.iter().any(|item| item.get("config").is_none());
            let has_scan_items = has_given && has_non_config;

            let mut remove_files: Vec<String> = Vec::new();
            let mut has_id_as = false;

            let mut photo_specs: Vec<PhotoSpec> = Vec::new();
            // Pairs of (source_relative, destination_relative) for byte-identical
            // duplicate fixture files.  Processed after photo generation so the
            // source file exists on disk.
            let mut duplicate_of_pairs: Vec<(String, String)> = Vec::new();
            // Byte transforms (truncation, in-place patch) applied after the
            // files they target exist and before the scan.
            let mut truncations: Vec<TruncateFile> = Vec::new();
            let mut patches: Vec<PatchFile> = Vec::new();

            for item in items {
                if let Some(dir) = item["dir_album"].as_str() {
                    let trimmed = dir.trim_start_matches('/');
                    std::fs::create_dir_all(&data.join(trimmed))
                        .unwrap_or_else(|e| panic!("create dir {trimmed}: {e}"));

                    if item.get("id_as").is_some() {
                        has_id_as = true;
                        let ph = format!("{trimmed}/.__picasu_ph__.jpg");
                        photo_specs.push(PhotoSpec {
                            output: Some(data.join(&ph).to_string_lossy().into_owned()),
                            format: Some("jpeg".into()),
                            width: Some(4),
                            height: Some(4),
                            tags: None,
                            exif_date: None,
                            further_iptc: None,
                            minimal: false,
                        });
                    }
                } else if let Some(raw_file) = item["raw_file"].as_str() {
                    let trimmed = raw_file.trim_start_matches('/');
                    let content = item["content"].as_str().unwrap_or("");
                    let path = data.join(trimmed);
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)
                            .unwrap_or_else(|e| panic!("create dir for raw_file {trimmed}: {e}"));
                    }
                    std::fs::write(&path, content)
                        .unwrap_or_else(|e| panic!("write raw_file {trimmed}: {e}"));
                } else if item.get("fixture").is_some() {
                    if item.get("id_as").is_some() || item.get("asset_id_as").is_some() {
                        has_id_as = true;
                    }
                    place_pinned_fixture(item, &data);
                } else if let Some(stem) = item["random_media"].as_str() {
                    let selected = selection.unwrap_or_else(|| {
                        panic!(
                            "given random_media: {stem} needs a `randomize:` block with a \
                             seed, because the format it materialises is the seed's choice"
                        )
                    });
                    if item.get("id_as").is_some() || item.get("asset_id_as").is_some() {
                        has_id_as = true;
                    }
                    // The scenario reads `${ext}`, `${format}` and `${mime}`, so its
                    // assertions can name what was selected without naming a
                    // format. `mime` comes from the capability manifest because
                    // the upload path takes the stored extension from the
                    // request's Content-Type — a scenario that guessed one would
                    // store, say, MOV bytes as `.jpeg`.
                    vars.insert("format".to_string(), selected.format.clone());
                    vars.insert("ext".to_string(), selected.extension.clone());
                    vars.insert(
                        "mime".to_string(),
                        snapfab::capabilities::capabilities()
                            .capability_for_format(&selected.format)
                            .map(|entry| entry.content_signature.mime.clone())
                            .unwrap_or_default(),
                    );
                    let destination = random_media_destination(item, selected);
                    match &selected.plan {
                        FixturePlan::Generate => photo_specs.push(PhotoSpec {
                            output: Some(data.join(&destination).to_string_lossy().into_owned()),
                            format: Some(selected.format.clone()),
                            width: Some(RANDOM_MEDIA_WIDTH),
                            height: Some(RANDOM_MEDIA_HEIGHT),
                            tags: None,
                            exif_date: None,
                            further_iptc: None,
                            minimal: false,
                        }),
                        FixturePlan::CopyFixture { id } => {
                            copy_pinned_fixture(id, &destination, &data)
                        }
                    }
                } else if let Some(photo) = item["photo"].as_str() {
                    let trimmed = photo.trim_start_matches('/');

                    // Either binding is a reason to run the discovery pass:
                    // `has_id_as` gates the whole pass, so keying it off
                    // `id_as` alone silently dropped a scenario that asked only
                    // for `asset_id_as` — the variable then interpolated to
                    // empty.
                    if item.get("id_as").is_some() || item.get("asset_id_as").is_some() {
                        has_id_as = true;
                    }

                    let tags: Vec<String> = item["tags"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .map(|t| t.as_str().unwrap_or("unknown").to_string())
                                .collect()
                        })
                        .unwrap_or_default();

                    let exif_date = item["exif_date"].as_str();
                    let has_tags = !tags.is_empty();

                    let format = item["format"]
                        .as_str()
                        .map(|f| f.to_string())
                        .unwrap_or_else(|| "jpeg".to_string());
                    assert!(
                        matches!(format.as_str(), "jpeg" | "png"),
                        "given photo format must be jpeg or png, got {format}"
                    );

                    let width = item["width"].as_u64().map_or(4, |w| w as u32);
                    let height = item["height"].as_u64().map_or(4, |h| h as u32);

                    photo_specs.push(PhotoSpec {
                        output: Some(data.join(trimmed).to_string_lossy().into_owned()),
                        format: Some(format),
                        width: Some(width),
                        height: Some(height),
                        tags: if has_tags { Some(tags) } else { None },
                        exif_date: exif_date.map(|d| d.to_string()),
                        // `further_iptc: true` asks for the IIM datasets the
                        // metadata engine surfaces in the read-only
                        // "further data" bucket. Off by default, so a fixture
                        // only carries them when its assertions need them.
                        further_iptc: if item["further_iptc"].as_bool() == Some(true) {
                            Some(true)
                        } else {
                            None
                        },
                        minimal: false,
                    });

                    let idx = vars.len();
                    vars.insert(format!("photo_{idx}"), format!("photo_{idx}"));
                } else if let Some(remove_path) = item["remove"].as_str() {
                    remove_files.push(remove_path.trim_start_matches('/').to_string());
                } else if let Some(config) = item.get("config").and_then(|c| c.as_object()) {
                    if let Some(enabled) = config.get("read_only_mode").and_then(|v| v.as_bool()) {
                        write_config(&serde_json::json!({"read_only_mode": enabled}));
                    }
                    if let Some(enabled) = config.get("fs_notify_watcher").and_then(|v| v.as_bool())
                    {
                        write_config(&serde_json::json!({"fs_notify_watcher": enabled}));
                    }
                    if let Some(enabled) = config
                        .get("validate_upload_content")
                        .and_then(|v| v.as_bool())
                    {
                        write_config(&serde_json::json!({"validate_upload_content": enabled}));
                    }
                    if let Some(enabled) = config
                        .get("use_client_timestamp_info")
                        .and_then(|v| v.as_bool())
                    {
                        write_config(&serde_json::json!({"use_client_timestamp_info": enabled}));
                    }
                } else if let Some(dup) = item.get("duplicate_of") {
                    let src = dup["source"]
                        .as_str()
                        .expect("duplicate_of.source is required")
                        .trim_start_matches('/')
                        .to_string();
                    let dst = dup["destination"]
                        .as_str()
                        .expect("duplicate_of.destination is required")
                        .trim_start_matches('/')
                        .to_string();
                    duplicate_of_pairs.push((src, dst));
                } else if item.get("truncate_file").is_some() {
                    truncations.push(parse_truncate_file(item));
                } else if item.get("patch_file").is_some() {
                    patches.push(parse_patch_file(item));
                }
            }

            if !photo_specs.is_empty() {
                generate_batch(&photo_specs).expect("generate photos");
            }

            // Copy source files to their duplicate destinations.  Source must
            // exist on disk (created by the photo fixtures above).  The copy
            // is a byte-identical duplicate — not a regenerated similar image.
            for (src_rel, dst_rel) in &duplicate_of_pairs {
                let src_path = data.join(src_rel);
                let dst_path = data.join(dst_rel);
                assert!(
                    src_path.exists(),
                    "duplicate_of source does not exist: {src_rel}"
                );
                if let Some(parent) = dst_path.parent() {
                    std::fs::create_dir_all(parent)
                        .unwrap_or_else(|e| panic!("create dir for duplicate_of {dst_rel}: {e}"));
                }
                std::fs::copy(&src_path, &dst_path)
                    .unwrap_or_else(|e| panic!("duplicate_of copy {src_rel} -> {dst_rel}: {e}"));
            }

            // Damage the files the transforms target only now that every photo,
            // fixture and duplicate exists on disk.
            apply_file_transforms(&data, &truncations, &patches);

            if has_scan_items {
                let client = make_client();

                let _scan_resp = client
                    .post("/post/index/album")
                    .cookie(auth_cookie(&client))
                    .header(ContentType::JSON)
                    .body(serde_json::json!({"album": "/"}).to_string())
                    .dispatch();
                assert_eq!(_scan_resp.status(), Status::Accepted, "scan trigger");
                wait_for_album_index(&client, 30000, IndexExpect::Completed);

                for rp in &remove_files {
                    std::fs::remove_file(&data.join(rp)).expect("remove file");
                }

                if has_id_as {
                    for item in items {
                        if let Some(dir) = item["dir_album"].as_str() {
                            if item.get("id_as").is_some() {
                                let id_name = item["id_as"].as_str().unwrap();
                                let bare = id_name.trim_start_matches('$');
                                let trimmed = dir.trim_start_matches('/');
                                let id = discover_album_id(&client, trimmed);
                                vars.insert(bare.to_string(), id);
                            }
                        } else if let Some(photo) = item["photo"].as_str() {
                            if item.get("id_as").is_some() {
                                let id_name = item["id_as"].as_str().unwrap();
                                let bare = id_name.trim_start_matches('$');
                                let trimmed = photo.trim_start_matches('/');
                                let hash = discover_photo_hash(&client, trimmed);
                                vars.insert(bare.to_string(), hash);
                            }
                            // Also discover asset_id if asset_id_as is specified.
                            if item.get("asset_id_as").is_some() {
                                let id_name = item["asset_id_as"].as_str().unwrap();
                                let bare = id_name.trim_start_matches('$');
                                let trimmed = photo.trim_start_matches('/');
                                let asset_id = discover_asset_id(&client, trimmed);
                                vars.insert(bare.to_string(), asset_id);
                            }
                        } else if item.get("fixture").is_some()
                            || item.get("random_media").is_some()
                        {
                            // A placed fixture and a randomized format are
                            // discoverable exactly like a generated photo, by
                            // their destination path.
                            let destination = if item.get("random_media").is_some() {
                                let selected = selection.unwrap_or_else(|| {
                                    panic!("given random_media without a selected format")
                                });
                                random_media_destination(item, selected)
                            } else {
                                pinned_fixture_destination(item)
                            };
                            if let Some(id_name) = item["id_as"].as_str() {
                                let bare = id_name.trim_start_matches('$');
                                let hash = discover_photo_hash(&client, &destination);
                                vars.insert(bare.to_string(), hash);
                            }
                            if let Some(id_name) = item["asset_id_as"].as_str() {
                                let bare = id_name.trim_start_matches('$');
                                let asset_id = discover_asset_id(&client, &destination);
                                vars.insert(bare.to_string(), asset_id);
                            }
                        }
                    }
                }
            }
        }
    }

    let when = &scenario["when"];

    if when.is_array() {
        let calls = when.as_array().expect("when array");
        let mut client_opt: Option<Client> = None;

        for (i, call) in calls.iter().enumerate() {
            if client_opt.is_none() {
                client_opt = Some(make_client());
            }
            let client = client_opt.as_ref().expect("client");
            let resp = dispatch_when_item(call, &vars, client);

            let is_last = i == calls.len() - 1;

            // The last call is asserted by the scenario's top-level `then:`
            // block; every other call by the `then:` block inside the call
            // itself. Either way the block is *this* response's, so every
            // assertion form in it runs: a `response.json.*` or `file_exists`
            // written under a call that is not the last one describes that
            // call's own response, and skipping it would leave the assertion
            // parsed and never executed.
            let call_then: &[Value] = if is_last {
                then_items
            } else if let Some(then) = call.get("then") {
                // A `then:` that is not a list of assertions would be read and
                // dropped, which is the same silence this branch exists to
                // remove one level up.
                then.as_array()
                    .unwrap_or_else(|| panic!("when call `then:` must be a list, got {then}"))
                    .as_slice()
            } else {
                &[]
            };

            let has_capture = call
                .get("capture")
                .and_then(|c| c.as_object())
                .is_some_and(|c| !c.is_empty());

            check_status_assertions(&resp, call_then, &vars);
            // A `then:` block is asserted before this call's `capture` and
            // `calc` feed the variables, so an assertion cannot depend on a
            // value the response it is asserting produced. It fails loudly
            // (an empty interpolation never equals the field) rather than
            // passing vacuously.
            let body = (then_needs_body(call_then) || has_capture)
                .then(|| resp.into_bytes().expect("response body"));
            if let Some(body) = body.as_deref() {
                if then_needs_body(call_then) {
                    check_body_assertions(body, call_then, &vars);
                }
                if has_capture {
                    process_capture(call, body, &mut vars);
                }
            }
            check_file_and_serve_assertions(call_then, &data, &vars, client);

            if !is_last {
                process_calc(call, &mut vars);
                if let Some(id_as) = call.get("id_as").and_then(|v| v.as_str()) {
                    let bare = id_as.trim_start_matches('$');
                    let discover_path = call
                        .get("discover_path")
                        .and_then(|v| v.as_str())
                        .unwrap_or_else(|| panic!("id_as {id_as}: requires discover_path"));
                    let path = interpolate(discover_path, &vars);
                    let hash = discover_photo_hash(client, &path);
                    vars.insert(bare.to_string(), hash);
                }
                if let Some(asset_id_as) = call.get("asset_id_as").and_then(|v| v.as_str()) {
                    let bare = asset_id_as.trim_start_matches('$');
                    let discover_path = call
                        .get("discover_path")
                        .and_then(|v| v.as_str())
                        .unwrap_or_else(|| {
                            panic!("asset_id_as {asset_id_as}: requires discover_path")
                        });
                    let path = interpolate(discover_path, &vars);
                    let asset_id = discover_asset_id(client, &path);
                    vars.insert(bare.to_string(), asset_id);
                }
            }
        }
    } else {
        let client = make_client();
        let resp = dispatch_when_item(when, &vars, &client);
        check_status_assertions(&resp, then_items, &vars);
        if then_needs_body(then_items) {
            let body = resp.into_bytes().expect("response body");
            check_body_assertions(&body, then_items, &vars);
        }
        check_file_and_serve_assertions(then_items, &data, &vars, &client);
    }

    if has_config_item {
        write_config(&serde_json::json!({"read_only_mode": false, "fs_notify_watcher": true}));
    }
}

// ── Scenario runners (called from generated test functions) ──

/// Read a scenario file from `tests/scenarios/` (`subdir` is `selftest` for the
/// scenarios that are expected to panic).
fn load_scenario(dir: &Path, subdir: &str, name: &str) -> Value {
    let path = dir.join(format!("tests/scenarios/{subdir}/{name}.yaml"));
    let yaml_str =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_yaml::from_str(&yaml_str).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

pub fn run_backend_scenario(name: &str) {
    let dir: std::path::PathBuf = std::env::var("CARGO_MANIFEST_DIR").map_or_else(
        |_| std::env::current_dir().unwrap(),
        std::path::PathBuf::from,
    );
    let scenario = load_scenario(&dir, "", name);

    match parse_randomize(&scenario) {
        Ok(Some(plan)) => run_randomized_scenario(name, &scenario, &plan),
        Ok(None) => interpret_scenario(&scenario, None),
        Err(message) => panic!("scenario {name}: {message}"),
    }
}

pub fn run_selftest_scenario(name: &str) {
    let dir: std::path::PathBuf = std::env::var("CARGO_MANIFEST_DIR").map_or_else(
        |_| std::env::current_dir().unwrap(),
        std::path::PathBuf::from,
    );
    let scenario = load_scenario(&dir, "selftest", name);
    assert!(
        parse_randomize(&scenario)
            .unwrap_or_else(|message| panic!("selftest {name}: {message}"))
            .is_none(),
        "selftest scenarios must not be randomized: they assert a panic"
    );

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        interpret_scenario(&scenario, None);
    }));

    assert!(
        result.is_err(),
        "generator test should have panicked: {name}"
    );
}

include!(concat!(env!("OUT_DIR"), "/scenarios.rs"));

#[cfg(test)]
mod tests {
    use super::{
        RANDOM_MEDIA_HEIGHT, RANDOM_MEDIA_WIDTH, banner_panic_message, check_body_assertions,
        parse_randomize, parse_wait_index, planned_runs, random_media_destination, seed_banner,
    };
    use crate::tests::fixtures::IndexExpect;
    use crate::tests::seeds::{DEFAULT_SET, seed_manifest};
    use snapfab::capabilities;
    use snapfab::selection::{FixturePlan, RandomizableFormat, randomizable_formats, select};

    fn scenario(value: serde_json::Value) -> serde_json::Value {
        value
    }

    /// The `wait_index` expectation the step declares. `true` is the default the
    /// existing scenarios use, and `failed` has to be *spelled* — an
    /// unrecognised spelling is an error, not a fallback to the default, or a
    /// scenario could ask for a failed index and quietly wait for a completed
    /// one.
    #[test]
    fn wait_index_reads_the_state_the_step_expects() {
        let expect = |value: serde_json::Value| {
            parse_wait_index(&scenario(serde_json::json!({ "wait_index": value })))
        };

        for completed in [
            serde_json::json!(true),
            serde_json::json!("completed"),
            serde_json::json!({ "expect": "completed" }),
        ] {
            assert_eq!(expect(completed), Some(IndexExpect::Completed));
        }
        for failed in [
            serde_json::json!("failed"),
            serde_json::json!({ "expect": "failed" }),
        ] {
            assert_eq!(expect(failed), Some(IndexExpect::Failed));
        }

        // A step without the verb is not a wait, and neither is a disabled one.
        assert_eq!(parse_wait_index(&scenario(serde_json::json!({}))), None);
        assert_eq!(expect(serde_json::json!(false)), None);

        for rejected in [
            serde_json::json!("finished"),
            serde_json::json!({ "state": "failed" }),
            serde_json::json!(7),
        ] {
            assert!(
                std::panic::catch_unwind(|| expect(rejected.clone())).is_err(),
                "{rejected} should be rejected rather than defaulting to `completed`"
            );
        }
    }

    /// A scenario without a `randomize:` block is a deterministic one: it runs
    /// once, and a `random_media` in it has no format to materialise.
    #[test]
    fn a_scenario_without_a_randomize_block_runs_once() {
        assert_eq!(
            parse_randomize(&scenario(serde_json::json!({}))).expect("valid"),
            None
        );
        assert_eq!(
            parse_randomize(&scenario(serde_json::json!({"name": "x"}))).expect("valid"),
            None
        );
        // An explicit null means "no randomization", not a broken block.
        assert_eq!(
            parse_randomize(&scenario(serde_json::json!({"randomize": null}))).expect("valid"),
            None
        );
    }

    /// The default set is the fixed CI set, so a scenario that opts into
    /// randomization without naming a set cannot widen what CI runs.
    #[test]
    fn a_randomize_block_without_a_set_runs_the_fixed_ci_set() {
        let plan = parse_randomize(&scenario(serde_json::json!({"randomize": {}})))
            .expect("valid")
            .expect("randomized");
        assert_eq!(plan.set, DEFAULT_SET);

        let named = parse_randomize(&scenario(serde_json::json!({"randomize": {"seeds": "ci"}})))
            .expect("valid")
            .expect("randomized");
        assert_eq!(named.set, "ci");

        let nightly = parse_randomize(&scenario(
            serde_json::json!({"randomize": {"seeds": "nightly"}}),
        ))
        .expect("valid")
        .expect("randomized");
        assert_eq!(nightly.set, "nightly");
    }

    /// A misspelled key must not fall back to the default set. `randomize: {seed:
    /// 42}` reads as if it should do something, and silently running the CI set
    /// instead would leave the scenario running with a seed nobody chose.
    #[test]
    fn an_unrecognised_randomize_key_is_rejected() {
        for block in [
            serde_json::json!({"randomize": {"seed": 42}}),
            serde_json::json!({"randomize": {"seeds": "ci", "count": 3}}),
        ] {
            let error = parse_randomize(&scenario(block.clone()))
                .expect_err("an unknown key should be rejected");
            assert!(
                error.contains("unknown key") && error.contains("`seeds`"),
                "the error must name the accepted key, got: {error}"
            );
        }
    }

    /// A block of the wrong shape is an error, not a default.
    #[test]
    fn a_malformed_randomize_block_is_rejected() {
        for block in [
            serde_json::json!({"randomize": "ci"}),
            serde_json::json!({"randomize": {"seeds": 3}}),
        ] {
            assert!(
                parse_randomize(&scenario(block)).is_err(),
                "a block that is not a mapping with a set name should be rejected"
            );
        }
    }

    /// The file a randomized scenario writes carries the selected format's own
    /// extension, and never a fixed one: that is what keeps the indexer from
    /// seeing misnamed content, and what `${ext}` in the assertions refers to.
    #[test]
    fn random_media_appends_the_selected_extensions_canonical_form() {
        for seed in 0..64u64 {
            let selected = select(seed, capabilities::capabilities()).expect("eligible");
            let item = serde_json::json!({ "random_media": "/dir/asset" });
            let destination = random_media_destination(&item, &selected);

            assert_eq!(
                destination,
                format!("dir/asset.{}", selected.extension),
                "seed {seed} ({}) must not write another format's extension",
                selected.format
            );
            let extension = destination.rsplit('.').next().expect("an extension");
            assert_eq!(
                capabilities::capabilities()
                    .capability_for_extension(extension)
                    .map(|entry| entry.format.as_str()),
                Some(selected.format.as_str()),
                "{destination} must resolve back to the selected format"
            );
        }
    }

    /// Leading slashes and a trailing dot are the two shapes a scenario author
    /// writes by accident, and both would produce a double extension or a hidden
    /// file.
    #[test]
    fn random_media_normalises_the_stem_it_is_given() {
        let selected = RandomizableFormat {
            format: "tiff".to_string(),
            extension: "tif".to_string(),
            plan: FixturePlan::CopyFixture {
                id: "tiff-48x32-exif".to_string(),
            },
        };

        for stem in ["/dir/asset", "dir/asset", "/dir/asset.", "dir/asset."] {
            assert_eq!(
                random_media_destination(&serde_json::json!({ "random_media": stem }), &selected),
                "dir/asset.tif",
                "stem {stem:?} should normalise to the same file"
            );
        }
    }

    /// The banner is the only place a reader learns which seed produced a red
    /// run, since `cargo test` swallows the scenario's own stdout.
    #[test]
    fn the_seed_banner_names_the_scenario_the_run_and_the_selection() {
        let selected = select(2, capabilities::capabilities()).expect("eligible");
        let banner = seed_banner("randomized_format_indexes", 3, 6, &selected.log_line(2));

        for expected in [
            "randomized_format_indexes",
            "run 3/6",
            &selected.log_line(2),
            "seed=2",
            &format!("format={}", selected.format),
        ] {
            assert!(
                banner.contains(expected),
                "{banner} must contain {expected}"
            );
        }
    }

    /// A red run's message has to carry both the seed and the original failure,
    /// otherwise the seed is only in output `cargo test` swallowed.
    #[test]
    fn a_failed_run_reports_the_seed_and_the_original_failure() {
        let banner = "[randomized] s run 1/2: seed=7 format=png ext=png source=generated";

        for payload in [
            Box::new("assertion failed: boom".to_string()) as Box<dyn std::any::Any + Send>,
            Box::new("assertion failed: borrowed"),
        ] {
            let message = banner_panic_message(banner, payload.as_ref());
            assert!(message.contains(banner), "{message} must carry the banner");
            assert!(
                message.contains("assertion failed"),
                "{message} must carry the original failure"
            );
        }

        let opaque_payload: Box<dyn std::any::Any + Send> = Box::new(42_u8);
        let opaque = banner_panic_message(banner, opaque_payload.as_ref());
        assert!(
            opaque.contains("non-string panic payload"),
            "an opaque payload still has to say something useful: {opaque}"
        );
    }

    /// One run per seed, in seed order, each with the format its seed resolves
    /// to. A runner that executed only the first seed would leave most of the
    /// matrix unexercised while every other test stayed green.
    #[test]
    fn every_seed_of_the_set_is_planned_as_its_own_run() {
        let plan = parse_randomize(&scenario(serde_json::json!({"randomize": {}})))
            .expect("valid")
            .expect("randomized");
        let expected = seed_manifest().sets[DEFAULT_SET].seeds.clone();

        let runs = planned_runs(&plan, None).expect("the fixed set resolves");
        assert_eq!(
            runs.iter().map(|(seed, _)| *seed).collect::<Vec<_>>(),
            expected,
            "each seed of the set must be its own run"
        );
        for (seed, selected) in &runs {
            assert_eq!(
                &selected.format,
                &select(*seed, capabilities::capabilities())
                    .expect("eligible")
                    .format,
                "run {seed} must carry the format its seed selects"
            );
        }
    }

    /// An override replaces the plan, so a reported seed can be replayed without
    /// touching the scenario.
    #[test]
    fn an_override_replaces_the_planned_runs() {
        let plan = parse_randomize(&scenario(serde_json::json!({"randomize": {}})))
            .expect("valid")
            .expect("randomized");

        let runs = planned_runs(&plan, Some("0, 19")).expect("seeds parse");
        assert_eq!(
            runs.iter().map(|(seed, _)| *seed).collect::<Vec<_>>(),
            vec![0, 19]
        );
        assert!(
            planned_runs(&plan, Some("weekly")).is_err(),
            "an undeclared set must not silently plan a partial run"
        );
    }

    /// Generated formats get a geometry that exercises the real thumbnail and
    /// metadata paths; pinned formats keep their fixture's. A degenerate size
    /// here would make every generated pass test the 2x2 path instead.
    #[test]
    fn generated_random_media_is_not_degenerate() {
        assert!(RANDOM_MEDIA_WIDTH > 4 && RANDOM_MEDIA_HEIGHT > 4);
        let generated: Vec<_> = randomizable_formats(capabilities::capabilities())
            .into_iter()
            .filter(|format| format.plan == FixturePlan::Generate)
            .map(|format| format.format)
            .collect();
        assert_eq!(
            generated,
            ["jpeg", "png"],
            "the generated formats are the ones rendered at the harness default"
        );
    }

    /// Run the body assertions of a `then:` list against a literal JSON body and
    /// report the panic message, so a unit test can assert on *why* an assertion
    /// failed. `None` means every assertion passed.
    fn body_assertion_failure(body: &str, then: &serde_json::Value) -> Option<String> {
        let vars = std::collections::HashMap::new();
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            check_body_assertions(body.as_bytes(), std::slice::from_ref(then), &vars);
        }));
        failure.err().map(|payload| {
            payload
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| {
                    payload
                        .downcast_ref::<&str>()
                        .map(|text| (*text).to_owned())
                        .unwrap_or_default()
                })
        })
    }

    /// The documented `response.<path> absent` form: it passes when the
    /// dot-path does not resolve. Before the branch existed, the value fell
    /// through to the equality assertion, which compared `Value::Null` against
    /// the string `absent` — so the one case the form exists for, "this key is
    /// not in the payload", failed, and nothing could be pinned with it.
    #[test]
    fn an_absent_path_passes_only_when_the_key_does_not_resolve() {
        let body = r#"{"ext":"jpg","exifVec":{"width":"48"},"tags":[]}"#;

        for path in [
            "response.json.furtherMetadata",
            "response.json.furtherMetadata.inner",
            "response.json.tags[0].nope",
            "response.json.ext.deeper",
        ] {
            assert_eq!(
                body_assertion_failure(body, &serde_json::json!({ path: "absent" })),
                None,
                "{path} does not resolve in {body}, so `absent` must pass"
            );
        }
    }

    /// `absent` and `null` are different claims, and a payload that carries an
    /// explicit `null` has *not* satisfied `absent`. `serde_json` answers "key
    /// missing" and "key present, value null" with the same `Value::Null`, so an
    /// implementation that reused `navigate_json` could not tell them apart —
    /// which is the limitation the old `furtherMetadata: null` pin was written
    /// around. Each case gets its own message so a red run says which one it hit.
    #[test]
    fn an_explicit_null_is_not_absent() {
        let present_null = r#"{"rating":null,"description":null}"#;

        let explicit_null = body_assertion_failure(
            present_null,
            &serde_json::json!({"response.json.rating": "absent"}),
        )
        .expect("an explicit null must not satisfy `absent`");
        assert!(
            explicit_null.contains("null"),
            "the message must say the key is present and null, got: {explicit_null}"
        );

        let present = body_assertion_failure(
            r#"{"rating":3}"#,
            &serde_json::json!({"response.json.rating": "absent"}),
        )
        .expect("a present value must not satisfy `absent`");
        assert!(
            present.contains("present") && !present.contains("explicitly null"),
            "a non-null value must be reported as present, got: {present}"
        );

        // The distinction itself, on the two payloads the whole form is about.
        assert_eq!(
            body_assertion_failure("{}", &serde_json::json!({"response.json.rating": "absent"})),
            None,
            "a missing key is absent"
        );
        assert!(
            body_assertion_failure(
                present_null,
                &serde_json::json!({"response.json.rating": "absent"})
            )
            .is_some(),
            "a key carrying an explicit null is present, not absent"
        );
    }

    /// The `absent` value is a form marker, not data: it is never interpolated
    /// and never compared, so a `${var}` in a sibling assertion cannot change
    /// what `absent` means. This is the same contract `not_null` has.
    #[test]
    fn absent_is_a_form_marker_rather_than_a_compared_value() {
        let body = r#"{"ext":"jpg"}"#;
        let mut vars = std::collections::HashMap::new();
        vars.insert("ext".to_owned(), "png".to_owned());

        let passed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            check_body_assertions(
                body.as_bytes(),
                &[serde_json::json!({"response.json.absent": "absent"})],
                &vars,
            );
        }));
        assert!(
            passed.is_ok(),
            "`absent` must not compare against an interpolated value"
        );

        // The sibling interpolation still works, so the marker is scoped to its
        // own assertion rather than swallowing the block.
        assert!(
            body_assertion_failure(body, &serde_json::json!({"response.json.ext": "${ext}"}))
                .is_some(),
            "a sibling assertion is still interpolated and still compared"
        );
    }
}
