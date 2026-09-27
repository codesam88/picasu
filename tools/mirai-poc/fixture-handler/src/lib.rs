//! Plan step 4, part 1: the Rocket *handler* shapes Picasu needs, with Rocket
//! itself left out.
//!
//! Every shape here is copied from a real route in `backend/src/router`, and
//! every case is one `pub fn` so that `--single_func <name>` makes the
//! attribution unambiguous. The `Json<T>`, guard and `Result` types are minimal
//! stand-ins with the same access paths the backend uses; the `#[get]`/`#[post]`
//! attribute half of the shape lives in `../fixture-rocket`.
//!
//! One case per row of the matrix in ../README.md. Nothing here is production
//! code, and the crate has no dependencies, so a run costs a fraction of a
//! second.
//!
//! Background for the expectations, all measured on MIRAI 1.1.12:
//!
//! - A `pub` non-generic function in the analyzed crate is an analysis entry
//!   point with **unconstrained parameters**, so a request-derived primitive
//!   reaching a sink is a _possible_ panic. That is what `--diag=paranoid`
//!   reports; `--diag=verify` reports nothing because nothing is provable.
//! - A closure body is analyzed only when the closure is *called* from analyzed
//!   code. An uncalled closure passed to a function outside the crate is
//!   unreachable, so its sink is never visited.
//! - An `async fn` body is a coroutine body, which is only entered by polling a
//!   future. MIRAI never polls, so no `async fn` body is analyzed, not even one
//!   that panics on a locally constructed value.
//! - A call that MIRAI cannot resolve to a body marks the analysis incomplete
//!   from there on. Further possible-panic diagnostics are suppressed unless the
//!   level is `paranoid`.

use std::ops::{Deref, DerefMut};

// -------------------------------------------------------------- guard shape

/// Stand-in for `crate::error::AppError`, which is both the handler return
/// error and the guard error: `backend/src/router/mod.rs` defines
/// `AppResult<T> = Result<T, AppError>` and `GuardResult<T> = Result<T, AppError>`
/// as two aliases for the same type.
pub struct AppError {
    pub kind: &'static str,
}

pub type AppResult<T> = Result<T, AppError>;
pub type GuardResult<T> = Result<T, AppError>;

/// Stand-in for `crate::router::auth::GuardTimestamp`: a guard that only succeeds
/// when the caller presented a valid timestamp.
#[derive(Debug)]
pub struct GuardTimestamp {
    pub timestamp: i64,
}

impl GuardTimestamp {
    pub fn timestamp(&self) -> i64 {
        self.timestamp
    }
}

// --------------------------------------------------------------- `Json<T>`

/// Stand-in for `rocket::serde::json::Json<T>`, providing the two access paths
/// the backend uses: `Deref` into a field and `into_inner()`. The `serde` bound
/// and the `JsonData` impl that Rocket needs to accept the body are not modeled
/// here, because they are about Rocket's data plumbing rather than about how a
/// value reaches a sink.
#[derive(Debug)]
pub struct Json<T>(pub T);

impl<T> Json<T> {
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for Json<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Json<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

/// Body shape of `EditRatingData` / `DeleteList`: every field is attacker
/// controlled and every field type is a request primitive or a collection of
/// them.
#[derive(Debug)]
pub struct EditRatingData {
    pub timestamp: i64,
    pub index_array: Vec<usize>,
    pub rating: Option<u8>,
}

// ------------------------------------------------------ private storage API

/// Stand-in for `SnapshotReadError`.
#[derive(Debug)]
pub enum SnapshotReadError {
    NotFound,
    Storage,
}

/// Stand-in for the `TREE_SNAPSHOT` singleton in
/// `backend/src/storage/cache.rs`: a `'static` value whose private methods are
/// the helpers the route handlers call.
pub struct TreeSnapshot;

impl TreeSnapshot {
    pub fn read_scrollbar(&'static self, timestamp: i64) -> Result<Vec<u8>, SnapshotReadError> {
        if timestamp < 0 {
            return Err(SnapshotReadError::NotFound);
        }
        Ok(vec![timestamp as u8])
    }

    pub fn read_row(
        &'static self,
        index: usize,
        timestamp: i64,
    ) -> Result<Vec<u8>, SnapshotReadError> {
        if index == usize::MAX {
            return Err(SnapshotReadError::Storage);
        }
        Ok(vec![timestamp as u8, index as u8])
    }
}

pub static TREE_SNAPSHOT: TreeSnapshot = TreeSnapshot;

/// Stand-in for the validating conversion inside `read_scrollbar`: a function
/// that returns `None` for an input the client controls. This is the shape that
/// feeds the sinks below, so the only thing under test is whether MIRAI treats
/// the handler parameter as unconstrained input.
fn to_millis(timestamp: i64) -> Option<i64> {
    if timestamp < 0 { None } else { Some(timestamp) }
}

/// The `Option::unwrap` sink reached through a private free function, the shape
/// of `get_scroll_bar` calling into storage.
fn helper_sink(timestamp: i64) -> i64 {
    to_millis(timestamp).unwrap()
}

/// The same sink with `expect`, which is what the backend's helpers use.
fn expect_sink(timestamp: i64) -> i64 {
    to_millis(timestamp).expect("timestamp must be non-negative")
}

/// A `Result`-returning helper whose sink is `Result::expect`. Pairs with
/// `h2`: `Option::expect` is a programmer assumption in MIRAI's contracts and is
/// silent, while `Result::expect` is not.
fn result_expect_sink(timestamp: i64) -> i64 {
    to_millis(timestamp)
        .ok_or("negative timestamp")
        .expect("timestamp must be convertible")
}

fn rows_at(items: &[u8], index: usize) -> u8 {
    items[index]
}

fn rating_sink(body: Json<EditRatingData>) -> u8 {
    body.into_inner().rating.unwrap()
}

fn first_index(indices: &[usize]) -> usize {
    indices[0]
}

/// Stand-in for `tokio::task::spawn_blocking`: the same `FnOnce` hand-off, but
/// defined inside the analyzed crate. Comparing `h14`/`h15` with `h16`
/// separates "MIRAI cannot follow a closure into another thread" from "MIRAI
/// cannot follow a call into a crate it did not analyze".
fn spawn_blocking_like<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
    R: Send + 'static,
{
    f()
}

// ------------------------------------------------- 1: query primitive + helper

/// `timestamp: i64` is a Rocket query primitive. Does MIRAI treat it as
/// unconstrained input, through a private helper, to `Option::unwrap`?
pub fn h1_query_primitive_to_helper_unwrap(timestamp: i64) -> i64 {
    helper_sink(timestamp)
}

/// The same path with `Option::expect`, which the backend's own helpers use.
pub fn h2_query_primitive_to_helper_expect(timestamp: i64) -> i64 {
    expect_sink(timestamp)
}

/// The same path with `Result::expect`, which the backend's storage helpers use
/// (`read_scrollbar` returns a `Result`). Reported, unlike `h2`.
pub fn h21_query_primitive_to_result_expect(timestamp: i64) -> i64 {
    result_expect_sink(timestamp)
}

/// Query index primitive into a private helper and a bounds check. The
/// `index: usize` of `get_rows`.
pub fn h3_query_index_to_helper_index(items: Vec<u8>, index: usize) -> u8 {
    rows_at(&items, index)
}

/// The `'static` singleton method call of `get_scroll_bar`, with a
/// `Result::unwrap` tail.
pub fn h4_static_method_result_unwrap(timestamp: i64) -> Vec<u8> {
    TREE_SNAPSHOT.read_scrollbar(timestamp).unwrap()
}

/// Arithmetic on a request-derived value, to see which panic kinds MIRAI models
/// at all.
pub fn h5_query_primitive_division(count: u32, size: u32) -> u32 {
    count / size
}

// ---------------------------------------------------------- 2: `Json<T>` body

/// `Json<EditRatingData>` through `into_inner()` and a private helper.
pub fn h6_json_body_into_inner_unwrap(body: Json<EditRatingData>) -> u8 {
    rating_sink(body)
}

/// `Json<EditRatingData>` read through `Deref` with no helper in between.
pub fn h7_json_body_deref_unwrap(body: Json<EditRatingData>) -> u8 {
    body.rating.unwrap()
}

/// Index sink driven by a body field, read through `Deref`.
pub fn h8_json_body_deref_index(body: Json<EditRatingData>) -> usize {
    body.index_array[0]
}

/// The same index sink one call deeper, as `edit_rating` does with
/// `&json_data.index_array`.
pub fn h9_json_body_index_into_helper(body: Json<EditRatingData>) -> usize {
    first_index(&body.index_array)
}

// ------------------------------------------------------------- 3: guard use

/// Guard discarded, sink still reached. The shape to compare against `h11`.
pub fn h10_guard_discarded(auth: GuardResult<GuardTimestamp>, timestamp: i64) -> i64 {
    let _ = auth;
    helper_sink(timestamp)
}

/// Guard propagated, sink still reached.
pub fn h11_guard_propagated(auth: GuardResult<GuardTimestamp>, timestamp: i64) -> AppResult<i64> {
    let _ = auth?;
    Ok(helper_sink(timestamp))
}

/// `h10` without the sink: the only difference from `h12` is the discarded
/// guard, so the pair measures what the discard itself costs.
pub fn h12_guard_discarded_no_sink(auth: GuardResult<GuardTimestamp>) -> u8 {
    let _ = auth;
    0
}

/// `h12` with `let _ = auth?;` instead, which is what every handler in
/// `backend/src/router/get` does.
pub fn h13_guard_propagated_no_sink(auth: GuardResult<GuardTimestamp>) -> AppResult<u8> {
    let _ = auth?;
    Ok(0)
}

/// `h10` with a sink that provably panics instead of a possibly panicking one.
/// This is the sharpest available test of whether the guard is visible to MIRAI:
/// `--diag=verify` reports a provable panic unless something earlier in the body
/// already made the analysis incomplete, and `?` is exactly such a call.
pub fn h19_guard_discarded_definite_sink(auth: GuardResult<GuardTimestamp>) -> u8 {
    let _ = auth;
    let missing: Option<u8> = None;
    missing.unwrap()
}

/// The same sink behind the propagated guard.
pub fn h20_guard_propagated_definite_sink(auth: GuardResult<GuardTimestamp>) -> AppResult<u8> {
    let _ = auth?;
    let missing: Option<u8> = None;
    Ok(missing.unwrap())
}

// ------------------------------------------ 4: hand-off into a worker "thread"

/// Hand-off to an `FnOnce` generic that lives in the analyzed crate.
pub fn h14_in_crate_spawn_like_unwrap(id: Option<u32>) -> u32 {
    spawn_blocking_like(move || id.unwrap())
}

/// The same, with an index sink.
pub fn h15_in_crate_spawn_like_index(items: Vec<u8>, index: usize) -> u8 {
    spawn_blocking_like(move || items[index])
}

/// Control for `h14`: the hand-off into `std::thread::spawn`, which is a
/// dependency and therefore has no MIR body for MIRAI to resolve.
pub fn h16_std_thread_spawn_unwrap(id: Option<u32>) -> u32 {
    std::thread::spawn(move || id.unwrap()).join().unwrap()
}

// ------------------------------------------------- 5: `async fn` / async block

/// An `async` route handler reaching the same sink as `h1`. Every handler in
/// `backend/src/router/get` and `put` is `pub async fn`.
pub async fn h17_async_handler_sync_sink(timestamp: i64) -> i64 {
    helper_sink(timestamp)
}

/// The sink inside an `async` block in a synchronous function, with nobody
/// polling the future. The reduction of `h17` that isolates the coroutine body
/// from the handler shape.
pub fn h18_async_block_never_polled(timestamp: i64) -> i64 {
    let future = async { helper_sink(timestamp) };
    let _ = future;
    0
}

// ------------------------------------- 6: is a missing body or a missing input?

/// The three cases below use a *locally constructed* `None`, so the panic is
/// certain rather than possible. They separate the two explanations for the
/// silence of `h16` and `h17`: MIRAI not entering the body at all, or MIRAI
/// entering it and not modelling the inputs. A certain panic is reported
/// verbatim, without the `possible` prefix the request-derived cases carry.

/// A certain panic inside a closure that is never called: no diagnostic. The
/// closure is a body owner, so a crate-wide run selects it, and analyzing it as
/// an entry point reaches nothing.
pub fn h22_uncalled_closure_concrete_sink() {
    let _unused = move || {
        let missing: Option<u8> = None;
        missing.unwrap();
    };
}

/// The same closure, called from the same function: reported as a certain panic.
pub fn h23_called_closure_concrete_sink() -> u8 {
    let call = move || {
        let missing: Option<u8> = None;
        missing.unwrap()
    };
    call()
}

/// A certain panic inside an `async fn`: no diagnostic, at any level, even
/// though MIRAI selects the function. An `async fn` body is a coroutine body and
/// is only entered by polling a future, which MIRAI never does.
pub async fn h24_async_concrete_sink() {
    let missing: Option<u8> = None;
    missing.unwrap();
}
