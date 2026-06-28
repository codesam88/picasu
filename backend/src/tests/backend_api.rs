#![cfg(test)]

use std::collections::HashMap;
use std::path::Path;

use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::Value;

use xtask::test_image::{PhotoSpec, generate_batch};

use crate::tests::bootstrap::*;
use crate::tests::fixtures::*;

// ── Variable interpolation ──

fn interpolate(s: &str, vars: &HashMap<String, String>) -> String {
    let mut result = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find('}') {
            let bare = &after[..end];
            let val = vars.get(bare).cloned().unwrap_or_default();
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
    let assert_obj = aw
        .get("assert")
        .and_then(|a| a.as_object())
        .expect("array_where requires 'assert' object");

    let arr = root
        .as_array()
        .expect("response must be an array for array_where");

    let found = arr
        .iter()
        .find(|item| {
            where_obj.iter().all(|(field, cond)| {
                let cond_interp = interpolate_value(cond, vars);
                item[field.as_str()] == cond_interp
            })
        })
        .expect("no element matching array_where conditions");

    for (field, expected) in assert_obj {
        let expected_interp = interpolate_value(expected, vars);
        assert_eq!(
            found[field.as_str()],
            expected_interp,
            "array_where {field} mismatch"
        );
    }
}

// ── Run assertions on a response (status first, then body) ──
// Takes ownership because `into_bytes()` consumes the response.

fn check_status_assertions(
    response: &rocket::local::blocking::LocalResponse<'_>,
    then_items: &[Value],
) {
    for item in then_items {
        if let Some(code) = item["response.status"].as_i64() {
            assert_eq!(response.status(), Status::from_code(code as u16).unwrap(),);
        } else if let Some(code) = item["response.status_not"].as_i64() {
            assert_ne!(response.status(), Status::from_code(code as u16).unwrap(),);
        }
    }
}

fn check_body_assertions(body_bytes: &[u8], then_items: &[Value], vars: &HashMap<String, String>) {
    let parsed: Value = serde_json::from_slice(body_bytes).expect("valid JSON response body");

    for item in then_items {
        if let Some(obj) = item.as_object() {
            for (key, val) in obj {
                if key.starts_with("response.json.") {
                    if val.as_object().and_then(|o| o.get("contains")).is_some() {
                        assert_json_contains(&parsed, key, val, vars);
                    } else if val
                        .as_object()
                        .and_then(|o| o.get("all_absolute"))
                        .and_then(|v| v.as_bool())
                        == Some(true)
                    {
                        assert_all_absolute(&parsed, key);
                    } else {
                        assert_json_field(&parsed, key, val, vars);
                    }
                } else if key == "array_min_counts" {
                    assert_array_min_counts(&parsed, val, vars);
                } else if key == "array_where" {
                    assert_array_where(&parsed, val, vars);
                }
            }
        }
    }
}

fn check_file_and_serve_assertions(
    then_items: &[Value],
    data: &Path,
    client: &Client,
    vars: &HashMap<String, String>,
) {
    for item in then_items {
        if let Some(file_path) = item["file_exists"].as_str() {
            let trimmed = file_path.trim_start_matches('/');
            assert!(data.join(trimmed).exists(), "file should exist: {trimmed}");
        } else if let Some(file_path) = item["file_absent"].as_str() {
            let trimmed = file_path.trim_start_matches('/');
            assert!(
                !data.join(trimmed).exists(),
                "file should be absent: {trimmed}"
            );
        } else if let Some(photo_var) = item["serve_image_ok"].as_str() {
            let bare = photo_var.trim_start_matches('$');
            let hash = vars
                .get(bare)
                .unwrap_or_else(|| panic!("serve_image_ok: unknown var {photo_var}"));
            assert_eq!(
                serve_compressed_image(client, hash),
                Status::Ok,
                "serve_image_ok: {photo_var}"
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

fn has_json_assertions(item: &Value) -> bool {
    item.as_object().is_some_and(|m| {
        m.keys().any(|k| {
            k.starts_with("response.json.") || k == "array_min_counts" || k == "array_where"
        })
    })
}

// ── Interpreter main logic ──

fn interpret_scenario(scenario: &Value) {
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

            for item in items {
                if let Some(dir) = item["dir_album"].as_str() {
                    let trimmed = dir.trim_start_matches('/');
                    std::fs::create_dir_all(&data.join(trimmed))
                        .unwrap_or_else(|e| panic!("create dir {trimmed}: {e}"));

                    if item.get("id_as").is_some() {
                        has_id_as = true;
                        let ph = format!("{trimmed}/.__picasu_ph__.jpg");
                        photo_specs.push(PhotoSpec {
                            output: Some(data.join(&ph).to_string_lossy().to_string()),
                            format: Some("jpeg".into()),
                            width: Some(4),
                            height: Some(4),
                            tags: None,
                            exif_date: None,
                        });
                    }
                } else if let Some(photo) = item["photo"].as_str() {
                    let trimmed = photo.trim_start_matches('/');

                    if item.get("id_as").is_some() {
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

                    photo_specs.push(PhotoSpec {
                        output: Some(data.join(trimmed).to_string_lossy().to_string()),
                        format: Some("jpeg".into()),
                        width: Some(4),
                        height: Some(4),
                        tags: if has_tags { Some(tags) } else { None },
                        exif_date: exif_date.map(|d| d.to_string()),
                    });

                    let idx = vars.len();
                    vars.insert(format!("photo_{idx}"), format!("photo_{idx}"));
                } else if let Some(remove_path) = item["remove"].as_str() {
                    remove_files.push(remove_path.trim_start_matches('/').to_string());
                } else if let Some(config) = item.get("config").and_then(|c| c.as_object()) {
                    if let Some(enabled) = config.get("read_only_mode").and_then(|v| v.as_bool()) {
                        write_config(&serde_json::json!({"read_only_mode": enabled}));
                    }
                }
            }

            if !photo_specs.is_empty() {
                generate_batch(&photo_specs).expect("generate photos");
            }

            if has_scan_items {
                let client = make_client();

                let _scan_resp = client
                    .post("/post/index/album")
                    .cookie(auth_cookie(&client))
                    .header(ContentType::JSON)
                    .body(serde_json::json!({"album": "/"}).to_string())
                    .dispatch();
                assert_eq!(_scan_resp.status(), Status::Accepted, "scan trigger");
                wait_for_album_index(&client, 30000);

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
            let resp = execute_call(call, &vars, client);

            let is_last = i == calls.len() - 1;

            if is_last {
                let has_json = then_items.iter().any(has_json_assertions);
                check_status_assertions(&resp, then_items);
                if has_json {
                    let body = resp.into_bytes().expect("response body");
                    check_body_assertions(&body, then_items, &vars);
                }
                check_file_and_serve_assertions(then_items, &data, client, &vars);
            } else {
                if let Some(call_then) = call.get("then").and_then(|v| v.as_array()) {
                    check_status_assertions(&resp, call_then);
                }
                if call
                    .get("capture")
                    .and_then(|c| c.as_object())
                    .is_some_and(|c| !c.is_empty())
                {
                    let body = resp.into_bytes().expect("response body");
                    process_capture(call, &body, &mut vars);
                }
                process_calc(call, &mut vars);
            }
        }
    } else {
        let client = make_client();
        let resp = execute_call(when, &vars, &client);
        check_status_assertions(&resp, then_items);
        let has_json = then_items.iter().any(has_json_assertions);
        if has_json {
            let body = resp.into_bytes().expect("response body");
            check_body_assertions(&body, then_items, &vars);
        }
        check_file_and_serve_assertions(then_items, &data, &client, &vars);
    }

    if has_config_item {
        write_config(&serde_json::json!({"read_only_mode": false}));
    }
}

// ── Scenario runners (called from generated test functions) ──

pub fn run_backend_scenario(name: &str) {
    let dir: std::path::PathBuf = std::env::var("CARGO_MANIFEST_DIR").map_or_else(
        |_| std::env::current_dir().unwrap(),
        std::path::PathBuf::from,
    );
    let path = dir.join(format!("tests/scenarios/{name}.yaml"));

    let yaml_str =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let scenario: Value =
        serde_yaml::from_str(&yaml_str).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));

    interpret_scenario(&scenario);
}

pub fn run_selftest_scenario(name: &str) {
    let dir: std::path::PathBuf = std::env::var("CARGO_MANIFEST_DIR").map_or_else(
        |_| std::env::current_dir().unwrap(),
        std::path::PathBuf::from,
    );
    let path = dir.join(format!("tests/scenarios/selftest/{name}.yaml"));

    let yaml_str =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let scenario: Value =
        serde_yaml::from_str(&yaml_str).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        interpret_scenario(&scenario);
    }));

    assert!(
        result.is_err(),
        "generator test should have panicked: {name}"
    );
}

include!(concat!(env!("OUT_DIR"), "/scenarios.rs"));
