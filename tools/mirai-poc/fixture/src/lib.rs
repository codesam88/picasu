//! Panic baseline for MIRAI 1.1.12.
//!
//! MIRAI analyzes every `pub` non-generic function body in the crate as an
//! analysis croot with unconstrained parameters, so each `case_*` function
//! below is analyzed independently. That is what makes one run per case
//! attributable, and why the runner passes `--single_func`.
//!
//! Naming: `case_a` is the shape the backend cares about (a request-derived
//! value reaching `Option::unwrap`), `case_b` is the shape that must stay
//! silent (a configuration invariant proven locally).

/// Request-like input parameter reaching `Option::unwrap()`.
pub fn case_a_request_unwrap(id: Option<u32>) -> u32 {
    id.unwrap()
}

/// Non-tainted configuration invariant. The `Some` is constructed in the body,
/// so the `unwrap()` is provably safe. False-positive control.
pub fn case_b_config_invariant() -> u32 {
    let configured: Option<u32> = Some(7);
    configured.unwrap()
}

/// The unwrap lives in a private helper reached from a public croot.
pub fn case_c_helper(id: Option<u32>) -> u32 {
    helper_require_id(id)
}

fn helper_require_id(id: Option<u32>) -> u32 {
    id.unwrap()
}

/// `move` closure capturing the request-derived value.
pub fn case_d_move_closure(id: Option<u32>) -> u32 {
    let extract = move |value: Option<u32>| value.unwrap();
    extract(id)
}

/// Spawn-like hand-off. The request-derived value is moved into another
/// thread, standing in for the `spawn_blocking` shape in the backend.
pub fn case_e_spawn_blocking(id: Option<u32>) -> u32 {
    std::thread::spawn(move || id.unwrap()).join().unwrap()
}

/// Bounds check driven by a request-derived index.
pub fn case_f_index_out_of_bounds(items: &[u8], index: usize) -> u8 {
    items[index]
}

/// Explicit panic on a request-derived condition.
pub fn case_g_explicit_panic(code: u32) {
    if code == 400 {
        panic!("bad request code {code}");
    }
}

/// Request-like input parameter reaching `Option::expect()`. Silent, because
/// MIRAI's standard contract treats `Option::expect_failed` as a programmer
/// assumption rather than a defect.
pub fn case_h_request_expect(id: Option<u32>) -> u32 {
    id.expect("snapshot id is required")
}

/// Request-like input parameter reaching `Result::unwrap()`. Unlike `Option`,
/// `Result` is reported at the paranoid level.
pub fn case_i_request_result_unwrap(parsed: Result<u32, String>) -> u32 {
    parsed.unwrap()
}
