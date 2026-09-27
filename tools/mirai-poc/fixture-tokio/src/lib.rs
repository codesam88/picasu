//! Plan step 4, part 2: the real `tokio::task::spawn_blocking` hand-off.
//!
//! `../fixture-handler` established that a closure body is analyzed when the
//! closure is called from analyzed code, and that the hand-off into
//! `std::thread::spawn` is not. This crate replaces the stand-in with the actual
//! tokio call the backend uses, to find out which of the two it is: a property
//! of the closure hand-off, or a property of calling into a crate that MIRAI did
//! not analyze.
//!
//! `tokio` is a dependency, and `cargo-mirai` compiles dependencies with plain
//! `rustc`, so tokio's own bodies have no MIR body available to MIRAI. That is
//! the same situation as `std::thread::spawn`, which is why `k5` and `k6` exist:
//! they show that a tokio dependency by itself does not disturb the analysis.
//!
//! Every case is one `pub fn` or `pub async fn`, one sink, and is run with
//! `--single_func`.

/// Stand-in for the guard of `get_rows` / `get_scroll_bar`.
pub struct AppError {
    pub kind: &'static str,
}

pub type GuardResult<T> = Result<T, AppError>;

#[derive(Debug)]
pub struct GuardTimestamp {
    pub timestamp: i64,
}

pub type AppResult<T> = Result<T, AppError>;

/// Private helper reached from a `spawn_blocking` closure, the shape of
/// `TREE_SNAPSHOT.read_row(index, timestamp)`.
fn read_row(index: usize, timestamp: i64) -> Result<u8, AppError> {
    if index == usize::MAX {
        return Err(AppError { kind: "database" });
    }
    Ok(timestamp as u8)
}

/// Private helper that panics, reached from a `spawn_blocking` closure.
fn helper_sink(id: Option<u32>) -> u32 {
    id.unwrap()
}

fn to_millis(timestamp: i64) -> Option<i64> {
    if timestamp < 0 { None } else { Some(timestamp) }
}

// ------------------------------------------------- 1: sink inside the closure

/// The shape of `delete.rs:69` and `get_metadata.rs:47`: an `Option::unwrap`
/// inside a `move` closure handed to `spawn_blocking`.
pub fn k1_spawn_blocking_closure_unwrap(id: Option<u32>) {
    let _handle = tokio::task::spawn_blocking(move || id.unwrap());
}

/// The same with `Option::expect`, which MIRAI's standard contract treats as a
/// programmer assumption.
pub fn k2_spawn_blocking_closure_expect(id: Option<u32>) {
    let _handle = tokio::task::spawn_blocking(move || id.expect("asset id is required"));
}

/// The same with an index sink driven by a request-derived index.
pub fn k3_spawn_blocking_closure_index(items: Vec<u8>, index: usize) {
    let _handle = tokio::task::spawn_blocking(move || items[index]);
}

/// The `get_rows` closure: the sink is in a private helper called by the closure.
pub fn k4_spawn_blocking_closure_calls_helper(id: Option<u32>) {
    let _handle = tokio::task::spawn_blocking(move || helper_sink(id));
}

// ----------------------------------------------------------- 2: controls

/// A `move` closure called from the same function, with tokio in the dependency
/// graph. If this is silent, the closure hand-off is the problem; if it is
/// reported, the problem is specific to the unresolved call.
pub fn k5_sync_closure_called_directly(id: Option<u32>) -> u32 {
    let extract = move |value: Option<u32>| value.unwrap();
    extract(id)
}

/// A plain private helper call, with tokio in the dependency graph. If this is
/// reported, having tokio as a dependency is not itself the blocker.
pub fn k6_sync_helper_sink(timestamp: i64) -> i64 {
    let millis = to_millis(timestamp);
    millis.unwrap()
}

// --------------------------------------------------------- 3: the `await` half

/// The `JoinHandle` result of `spawn_blocking` unwrapped in an `async` handler.
/// The closure itself is sink-free, so the only panic site is the `await`.
pub async fn k7_async_await_handle_unwrap(id: Option<u32>) -> u32 {
    let row = tokio::task::spawn_blocking(move || double(id.unwrap_or(0))).await;
    row.unwrap()
}

/// Sink-free arithmetic, so `k7` has exactly one panic site.
fn double(value: u32) -> u32 {
    value * 2
}

/// The shape of `get_rows` in full: an `async` handler, a guard propagated with
/// `?`, a request-derived index and timestamp moved into a `spawn_blocking`
/// closure, and the helper's `Result` propagated inside the closure.
pub async fn k8_async_get_rows_shape(
    auth: GuardResult<GuardTimestamp>,
    index: usize,
    timestamp: i64,
) -> AppResult<u8> {
    let _ = auth?;
    let row = tokio::task::spawn_blocking(move || read_row(index, timestamp)).await;
    match row {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(AppError { kind: "internal" }),
    }
}
