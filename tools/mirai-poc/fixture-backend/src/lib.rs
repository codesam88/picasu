//! Plan step 5: the real `get_rows` / `get_scroll_bar` / `read_scrollbar` /
//! `read_row` bodies, reduced to a crate with no dependencies.
//!
//! `../fixture-handler` holds stand-ins for the *kinds* of shape a Rocket
//! handler can have. This crate holds the actual bodies of the functions the
//! plan names, in the revision that still contained the panic sites
//! (`5083c5fb^`) and in the current one, so the two questions step 5 asks can
//! be answered at the real source rather than at a list of representative
//! shapes:
//!
//! 1. is `timestamp -> read_scrollbar -> expect` detected?
//! 2. is a discarded guard distinguishable from a propagated one?
//!
//! What the reduction loses, in full:
//!
//! - **Rocket.** No `#[get]` attribute, no query-primitive parsing, no
//!   `Json<T>` plumbing. `../fixture-rocket` covers the attribute; a handler
//!   parameter is the trust boundary in either case, and the two revisions of
//!   the handlers are otherwise character-identical.
//! - **The stores.** `TREE_SNAPSHOT` holds two const arrays instead of a
//!   `DashMap` and a redb table, and `read_tree_snapshot` returns one snapshot
//!   instead of a `MyCow` enum with two arms. No redb transaction, no table
//!   open, no `TableError`, no `LazyLock`.
//! - **The snapshot size.** `read_row` and `get_width_height` take the number
//!   of rows the snapshot holds as a `usize` parameter, because the real value
//!   is read from the store and is not knowable statically. With a const length
//!   MIRAI proves every bounds check in `read_row` and the cases below would
//!   pass for the wrong reason. This is the only signature change, and it is
//!   visible in the case names.
//! - **The runtime.** `tokio::task::spawn_blocking` becomes an in-crate
//!   `FnOnce` that calls its argument, and awaiting the handle is a ready
//!   future. The worker pool, the `std::thread::spawn` that starts it and the
//!   `Poll` bookkeeping are not modeled, so a sink reported through the
//!   hand-off here says nothing about tokio. `../fixture-tokio` measures the
//!   real hand-off; this crate only isolates `async` and the closure.
//! - **chrono.** `Utc.timestamp_millis_opt(..).single()` becomes a two-field
//!   struct that yields `None` outside a small range, derived with operations
//!   that cannot overflow. The `Option`-returning shape is kept because the
//!   pre-fix code consumes it with `Option::expect`, and the arithmetic is kept
//!   overflow-free because a diagnostic caused by the stand-in would answer a
//!   different question than the one the case asks.
//! - **`info!`, `Instant::now()`, `#[utoipa::path]`, `or_raise`'s closure body,
//!   the `Debug` derive.** No control flow, and a derive MIRAI cannot resolve.
//!   `or_raise` itself is kept, because the `?` it wraps is a call MIRAI may
//!   fail to resolve.
//!
//! Background for the expectations, all measured on MIRAI 1.1.12 and recorded in
//! ../README.md: a `pub` non-generic function in the analyzed crate is an entry
//! point with **unconstrained parameters**, so a request-derived primitive
//! reaching a sink is a _possible_ panic that only `--diag=paranoid` reports;
//! `Result::expect` is a sink, `Option::expect` is not, because MIRAI's own
//! contract for `option::expect_failed` is `assume_unreachable!()`; an `async
//! fn` body is a coroutine body, which is only entered by polling a future,
//! which MIRAI never does; and a value a loop produced is lost, which is what
//! the `b16`-`b18` group measures.

use std::future::Future;
use std::ops::Deref;
use std::ops::Range;
use std::pin::Pin;
use std::task::{Context, Poll};

/// `backend/src/constant.rs`.
pub const ROW_BATCH_NUMBER: usize = 20;

// ---------------------------------------------------------- error and result

/// Stand-in for `crate::error::AppError`.
///
/// `Debug` is written out by hand rather than derived because `Result::unwrap`
/// requires it and the derive emits a call MIRAI cannot resolve, which adds an
/// `incomplete analysis` note to the crate-wide run. A diagnostic in this
/// fixture should be attributable to the backend shape under test.
pub struct AppError {
    pub kind: &'static str,
}

impl std::fmt::Debug for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppError")
    }
}

impl AppError {
    pub fn new(kind: &'static str) -> Self {
        Self { kind }
    }
}

/// `backend/src/router/mod.rs`: both aliases name the same type.
pub type AppResult<T> = Result<T, AppError>;
pub type GuardResult<T> = Result<T, AppError>;

/// Stand-in for `crate::error::ResultExt`, the trait that carries the `or_raise`
/// tail of every `spawn_blocking` handler. It is a dependency-free method, so
/// MIRAI resolves it; what matters for the cases below is that the call exists,
/// because a call MIRAI cannot resolve makes the rest of the analysis
/// incomplete.
pub trait ResultExt<T> {
    fn or_raise(self, error: impl FnOnce() -> AppError) -> Result<T, AppError>;
}

impl<T> ResultExt<T> for Result<T, AppError> {
    fn or_raise(self, error: impl FnOnce() -> AppError) -> Result<T, AppError> {
        self.map_err(|_| error())
    }
}

/// Stand-in for `crate::router::auth::GuardTimestamp`.
pub struct GuardTimestamp {
    pub timestamp: i64,
}

impl GuardTimestamp {
    pub fn timestamp(&self) -> i64 {
        self.timestamp
    }
}

/// Stand-in for `rocket::serde::json::Json<T>`, with the `Deref` path the
/// backend uses.
pub struct Json<T>(pub T);

impl<T> Deref for Json<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

// ------------------------------------------------------------ model response

/// `backend/src/model/response.rs`. The `serde` derives are not modeled: they
/// are response plumbing, not control flow.
pub struct DisplayElement {
    pub display_width: u32,
    pub display_height: u32,
}

pub struct Row {
    pub start: usize,
    pub end: usize,
    pub display_elements: Vec<DisplayElement>,
    pub row_index: usize,
}

pub struct ScrollBarData {
    pub year: usize,
    pub month: usize,
    pub index: usize,
}

// ------------------------------------------------------------------- storage

/// `backend/src/storage/cache.rs`: distinguishes a snapshot id that was never
/// minted or has been dropped (client staleness, 400) from a store fault
/// (500). `Debug` is hand-written for the same reason as `AppError`'s: the
/// `expect` under test needs the bound, and the derive's own MIR is noise.
pub enum SnapshotReadError {
    NotFound { timestamp: i64 },
    Storage,
}

impl std::fmt::Debug for SnapshotReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SnapshotReadError")
    }
}

/// Reduction of `MyCow`: the rows of one snapshot, as slices. The real enum wraps
/// either a `DashMap` bucket or a redb table, and its length is a fallible
/// `Result<usize>` read from the store.
pub struct Snapshot {
    pub dates: &'static [i64],
    pub dims: &'static [(u32, u32)],
}

impl Snapshot {
    /// The pre-fix `get_width_height` (`5083c5fb^`), the panic site on the
    /// `get_rows` path: the caller-derived row index is used to index the
    /// snapshot's dimensions directly.
    pub fn get_width_height_prefix(&self, index: usize, rows: usize) -> Result<(u32, u32), SnapshotReadError> {
        let dims = snapshot_dims(rows);
        let data = &dims[index];
        Ok((data.0, data.1))
    }

    /// The current `get_width_height`, which the same commit changed to `.get()`.
    pub fn get_width_height(&self, index: usize, rows: usize) -> Result<(u32, u32), SnapshotReadError> {
        let dims = snapshot_dims(rows);
        dims.get(index).copied().ok_or(SnapshotReadError::Storage)
    }
}

/// Stand-in for the `TREE_SNAPSHOT` singleton. The real one is a `LazyLock`
/// holding a redb `Database` and a `DashMap`.
pub struct TreeSnapshot;

impl TreeSnapshot {
    /// Reduction of `read_tree_snapshot`: the in-memory lookup with the on-disk
    /// table open as its fallback, and the `NotFound` error both raise for an id
    /// that is not in the store. The signature is the real one; the row count the
    /// store holds is not knowable statically here either, so it enters at
    /// `read_row`, where the real code calls `MyCow::len()`.
    pub fn read_tree_snapshot(
        &'static self,
        timestamp: i64,
    ) -> Result<&'static Snapshot, SnapshotReadError> {
        for snapshot in SNAPSHOTS.iter() {
            if snapshot.dates.first() == Some(&timestamp) {
                return Ok(snapshot);
            }
        }
        Err(SnapshotReadError::NotFound { timestamp })
    }

    /// The pre-fix `read_scrollbar` (`5083c5fb^`), with both of its panic sites:
    /// the `Result` from `read_tree_snapshot` consumed with `Result::expect`, and
    /// the `Option` from the date conversion consumed with `Option::expect`.
    /// The two-arm `match` over the storage backends is reduced to the loop over
    /// the snapshot's dates, which is the only part that can panic.
    pub fn read_scrollbar_prefix(&'static self, timestamp: i64) -> Vec<ScrollBarData> {
        let tree_snapshot = self
            .read_tree_snapshot(timestamp)
            .expect("failed to read tree snapshot for scrollbar");
        let mut scroll_bar_data_vec = Vec::new();
        let mut last_year: Option<usize> = None;
        let mut last_month: Option<usize> = None;

        for (index, date) in tree_snapshot.dates.iter().enumerate() {
            let (year, month) = to_year_month_prefix(*date);
            if last_year != Some(year) || last_month != Some(month) {
                last_year = Some(year);
                last_month = Some(month);
                let scrollbar_data = ScrollBarData { year, month, index };
                scroll_bar_data_vec.push(scrollbar_data);
            }
        }
        scroll_bar_data_vec
    }

    /// The current `read_scrollbar`: `Result` out, no `expect`, and the date
    /// conversion turns an unconvertible stored date into `Storage` rather than
    /// a panic.
    pub fn read_scrollbar(
        &'static self,
        timestamp: i64,
    ) -> Result<Vec<ScrollBarData>, SnapshotReadError> {
        let tree_snapshot = self.read_tree_snapshot(timestamp)?;
        let mut scroll_bar_data_vec = Vec::new();
        let mut last_year: Option<usize> = None;
        let mut last_month: Option<usize> = None;

        let to_year_month = |date: i64| -> Result<(usize, usize), SnapshotReadError> {
            let datetime = timestamp_millis_opt(date)
                .single()
                .ok_or(SnapshotReadError::Storage)?;
            Ok((datetime.year(), datetime.month()))
        };

        for (index, date) in tree_snapshot.dates.iter().enumerate() {
            let (year, month) = to_year_month(*date)?;
            if last_year != Some(year) || last_month != Some(month) {
                last_year = Some(year);
                last_month = Some(month);
                let scrollbar_data = ScrollBarData { year, month, index };
                scroll_bar_data_vec.push(scrollbar_data);
            }
        }
        Ok(scroll_bar_data_vec)
    }

    /// The pre-fix `read_row` (`5083c5fb^`): the `Result` is propagated with `?`,
    /// and the panic site is the `number_vec` range feeding the closure that
    /// calls the indexing `get_width_height`.
    pub fn read_row_prefix(
        &'static self,
        row_index: usize,
        timestamp: i64,
        rows: usize,
    ) -> Result<Row, SnapshotReadError> {
        let tree_snapshot = self.read_tree_snapshot(timestamp)?;

        // The row count the store reports, i.e. the real `MyCow::len()?`.
        let data_length = rows;
        let chunk_count = data_length.div_ceil(ROW_BATCH_NUMBER);

        if row_index > chunk_count {
            return Err(SnapshotReadError::Storage);
        }

        let number_vec = (row_index * ROW_BATCH_NUMBER)
            ..(row_index * ROW_BATCH_NUMBER + ROW_BATCH_NUMBER).min(data_length);

        let display_elements: Vec<DisplayElement> = number_vec
            .map(|index| -> Result<DisplayElement, SnapshotReadError> {
                let (width, height) = tree_snapshot.get_width_height_prefix(index, data_length)?;
                Ok(DisplayElement {
                    display_width: width,
                    display_height: height,
                })
            })
            .collect::<Result<Vec<DisplayElement>, SnapshotReadError>>()?;

        Ok(Row {
            start: row_index * ROW_BATCH_NUMBER,
            end: row_index * ROW_BATCH_NUMBER + ROW_BATCH_NUMBER - 1,
            display_elements,
            row_index,
        })
    }

    /// The current `read_row`, which differs only in the bounds check of
    /// `get_width_height` and in `len()` being fallible.
    pub fn read_row(
        &'static self,
        row_index: usize,
        timestamp: i64,
        rows: usize,
    ) -> Result<Row, SnapshotReadError> {
        let tree_snapshot = self.read_tree_snapshot(timestamp)?;

        // The row count the store reports, i.e. the real `MyCow::len()?`.
        let data_length = rows;
        let chunk_count = data_length.div_ceil(ROW_BATCH_NUMBER);

        if row_index > chunk_count {
            return Err(SnapshotReadError::Storage);
        }

        let number_vec = (row_index * ROW_BATCH_NUMBER)
            ..(row_index * ROW_BATCH_NUMBER + ROW_BATCH_NUMBER).min(data_length);

        let display_elements: Vec<DisplayElement> = number_vec
            .map(|index| -> Result<DisplayElement, SnapshotReadError> {
                let (width, height) = tree_snapshot.get_width_height(index, data_length)?;
                Ok(DisplayElement {
                    display_width: width,
                    display_height: height,
                })
            })
            .collect::<Result<Vec<DisplayElement>, SnapshotReadError>>()?;

        Ok(Row {
            start: row_index * ROW_BATCH_NUMBER,
            end: row_index * ROW_BATCH_NUMBER + ROW_BATCH_NUMBER - 1,
            display_elements,
            row_index,
        })
    }
}

pub static TREE_SNAPSHOT: TreeSnapshot = TreeSnapshot;

static SNAPSHOTS: [Snapshot; 1] = [Snapshot {
    dates: &[1_700_000_000_000, 1_700_086_400_000],
    dims: &[(1920, 1080), (800, 600)],
}];

static SNAPSHOT_DIMS: [(u32, u32); 2] = [(1920, 1080), (800, 600)];

/// The snapshot's dimension vector, as a slice of unknown length.
///
/// This is the reduction that keeps the bounds checks honest: the real vector is
/// whatever the store holds, and `&SNAPSHOT_DIMS[..rows]` is the widest slice
/// MIRAI accepts from a const array without letting it prove that every index
/// into the result is in bounds.
fn snapshot_dims(rows: usize) -> &'static [(u32, u32)] {
    &SNAPSHOT_DIMS[..rows.min(SNAPSHOT_DIMS.len())]
}

/// Stand-in for the `(i32, u32)` pair chrono returns from `.year()`/`.month()`,
/// with `usize` fields so that the loops below need no `as usize` cast.
struct DateTime {
    year: usize,
    month: usize,
}

impl DateTime {
    fn year(&self) -> usize {
        self.year
    }

    fn month(&self) -> usize {
        self.month
    }
}

struct LocalResult<T> {
    inner: Option<T>,
}

impl<T> LocalResult<T> {
    /// `chrono`'s `.single()`: `Some` only for an unambiguous value.
    fn single(self) -> Option<T> {
        self.inner
    }
}

/// A stored date outside this window yields `None`, which is the branch the
/// pre-fix `read_scrollbar` turned into a panic with `Option::expect`.
///
/// Only the `None` branch matters here, so the bucket values are derived with
/// divisions and remainders by non-zero constants and one addition bounded by
/// the divisor. The real chrono call is arithmetic on the same `i64`, but its
/// result feeds a comparison rather than a sink; if the stand-in could overflow,
/// MIRAI would report that overflow instead of the `expect` under test, and the
/// case would answer the wrong question.
fn timestamp_millis_opt(millis: i64) -> LocalResult<DateTime> {
    if !(0..=4_102_444_800_000).contains(&millis) {
        return LocalResult { inner: None };
    }
    let millis = usize::try_from(millis).unwrap_or(0);
    let year = millis / 1000;
    let month = millis % 12 + 1;
    LocalResult {
        inner: Some(DateTime { year, month }),
    }
}

/// The date conversion of the pre-fix `read_scrollbar`, as a free function so
/// that `b6` can call it with a request-derived value: in the real code
/// `data.date` is stored, not derived from the request, so the only thing under
/// test there is `Option::expect`.
fn to_year_month_prefix(date: i64) -> (usize, usize) {
    let datetime = timestamp_millis_opt(date)
        .single()
        .expect("invalid timestamp");
    (datetime.year(), datetime.month())
}

/// The same conversion on the `get_rows` path, which is pure arithmetic on the
/// row index. Kept as a separate function so the arithmetic cases have no sink
/// in their name.
fn row_range(row_index: usize, data_length: usize) -> Range<usize> {
    let chunk_count = data_length.div_ceil(ROW_BATCH_NUMBER);
    if row_index > chunk_count {
        return 0..0;
    }
    (row_index * ROW_BATCH_NUMBER)
        ..(row_index * ROW_BATCH_NUMBER + ROW_BATCH_NUMBER).min(data_length)
}

/// `backend/src/router/get/get_data.rs`, unchanged by `5083c5fb`.
fn map_snapshot_read_error(err: SnapshotReadError) -> AppError {
    match err {
        SnapshotReadError::NotFound { .. } => AppError::new("InvalidInput"),
        SnapshotReadError::Storage => AppError::new("Database"),
    }
}

// -------------------------------------------------------- runtime stand-ins

/// Stand-in for `tokio::task::spawn_blocking`: the same `FnOnce` hand-off, but
/// defined inside the analyzed crate, so MIRAI resolves it and calls the
/// closure. `../fixture-tokio` is the same shape on the real tokio.
fn spawn_blocking_like<F, R>(f: F) -> JoinHandleLike<R>
where
    F: FnOnce() -> R,
{
    JoinHandleLike { value: Some(f()) }
}

/// Stand-in for `tokio::task::JoinHandle` and the future it is. `poll` hands the
/// value straight back instead of scheduling it on a blocking worker, and
/// neither `poll` nor `join` contains a sink of its own, so a reported panic in
/// these cases can only come from the handler body.
pub struct JoinHandleLike<T> {
    value: Option<T>,
}

impl<T> Unpin for JoinHandleLike<T> {}

impl<T> JoinHandleLike<T> {
    /// Stands in for awaiting the handle. The real handler awaits it inside the
    /// coroutine, which is `b11`; a synchronous stand-in is what lets `b10`
    /// exercise the `FnOnce` hand-off with no coroutine in the way, so the two
    /// hand-off cases differ only in `async`.
    pub fn join(self) -> Result<T, AppError> {
        match self.value {
            Some(value) => Ok(value),
            None => Err(AppError::new("Internal")),
        }
    }
}

impl<T> Future for JoinHandleLike<T> {
    type Output = Result<T, AppError>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.get_mut().value.take() {
            Some(value) => Poll::Ready(Ok(value)),
            None => Poll::Ready(Err(AppError::new("Internal"))),
        }
    }
}

// ============================================================== the cases ====

// ------------------------------------------- 1: timestamp -> expect (plan 5)

/// The exact edge the plan asks about, with nothing else in the body: the
/// request-derived snapshot id consumed with `Result::expect`, straight out of
/// `read_tree_snapshot`. `b3` is the same edge inside the whole pre-fix
/// `read_scrollbar` body, so the pair separates "the sink is not modelled" from
/// "the sink is modelled but is not what the run reports".
pub fn b1_read_tree_snapshot_expect(timestamp: i64) -> &'static Snapshot {
    TREE_SNAPSHOT
        .read_tree_snapshot(timestamp)
        .expect("failed to read tree snapshot for scrollbar")
}

/// The pre-fix `get_scroll_bar` (`5083c5fb^`), character for character apart from
/// the `#[get]` attribute. The sink is the `Result::expect` of `b1`, one call
/// further below, in a `&'static self` method on the singleton.
pub fn b2_get_scroll_bar_prefix_expect(
    auth: GuardResult<GuardTimestamp>,
    timestamp: i64,
) -> AppResult<Json<Vec<ScrollBarData>>> {
    let _ = auth?;
    let scrollbar_data = TREE_SNAPSHOT.read_scrollbar_prefix(timestamp);
    Ok(Json(scrollbar_data))
}

/// The same path without the handler, to show what the handler itself adds.
pub fn b3_read_scrollbar_prefix_expect(timestamp: i64) -> Vec<ScrollBarData> {
    TREE_SNAPSHOT.read_scrollbar_prefix(timestamp)
}

/// The current `get_scroll_bar`: the `expect` is gone and the error is mapped.
pub fn b4_get_scroll_bar_current(
    auth: GuardResult<GuardTimestamp>,
    timestamp: i64,
) -> AppResult<Json<Vec<ScrollBarData>>> {
    let _ = auth?;
    let scrollbar_data = TREE_SNAPSHOT
        .read_scrollbar(timestamp)
        .map_err(map_snapshot_read_error)?;
    Ok(Json(scrollbar_data))
}

/// The pre-fix handler with `Result::unwrap` instead of `Result::expect`, to
/// check that the sink is not specific to the `expect` spelling.
pub fn b5_get_scroll_bar_prefix_unwrap(
    auth: GuardResult<GuardTimestamp>,
    timestamp: i64,
) -> AppResult<Json<Vec<ScrollBarData>>> {
    let _ = auth?;
    let scrollbar_data = TREE_SNAPSHOT
        .read_scrollbar(timestamp)
        .map_err(map_snapshot_read_error)
        .unwrap();
    Ok(Json(scrollbar_data))
}

/// The other panic site of the same pre-fix function, with the request-derived
/// value substituted for the stored date so that the only thing under test is
/// the `Option::expect` contract.
pub fn b6_read_scrollbar_option_expect_prefix(timestamp: i64) -> (usize, usize) {
    to_year_month_prefix(timestamp)
}

// ------------------------------------ 2: get_rows, one mechanism removed at a time

/// The `get_rows` panic site in isolation: a request-derived index used to index
/// the snapshot, with none of the range arithmetic in front of it.
pub fn b7_get_width_height_prefix_index(
    timestamp: i64,
    index: usize,
    rows: usize,
) -> Result<(u32, u32), SnapshotReadError> {
    let tree_snapshot = TREE_SNAPSHOT.read_tree_snapshot(timestamp)?;
    tree_snapshot.get_width_height_prefix(index, rows)
}

/// The pre-fix `read_row` body: the same sink, behind the real range arithmetic
/// and the real closure.
pub fn b8_read_row_prefix_range(
    index: usize,
    timestamp: i64,
    rows: usize,
) -> Result<Row, SnapshotReadError> {
    TREE_SNAPSHOT.read_row_prefix(index, timestamp, rows)
}

/// The arithmetic of `read_row` with no sink at all, to see whether MIRAI
/// models the `usize` overflow in `row_index * ROW_BATCH_NUMBER` and the
/// `+ ROW_BATCH_NUMBER - 1` underflow.
pub fn b9_read_row_chunk_math(index: usize, rows: usize) -> Range<usize> {
    row_range(index, rows)
}

/// The pre-fix `get_rows` with `async` and `.await` removed and the hand-off
/// replaced by a direct call. The only difference from `b12` is the coroutine.
pub fn b10_get_rows_sync_no_handoff(
    auth: GuardResult<GuardTimestamp>,
    index: usize,
    timestamp: i64,
    rows: usize,
) -> AppResult<Json<Row>> {
    let _ = auth?;
    let filtered_rows = TREE_SNAPSHOT
        .read_row_prefix(index, timestamp, rows)
        .map_err(map_snapshot_read_error)?;
    Ok(Json(filtered_rows))
}

/// The pre-fix `get_rows` with `async` removed and the in-crate hand-off kept, so
/// the closure and the `FnOnce` are the only things left to lose it.
pub fn b11_get_rows_sync_handoff(
    auth: GuardResult<GuardTimestamp>,
    index: usize,
    timestamp: i64,
    rows: usize,
) -> AppResult<Json<Row>> {
    let _ = auth?;
    spawn_blocking_like(move || -> AppResult<Json<Row>> {
        let filtered_rows = TREE_SNAPSHOT
            .read_row_prefix(index, timestamp, rows)
            .map_err(map_snapshot_read_error)?;
        Ok(Json(filtered_rows))
    })
    .join()
    .or_raise(|| AppError::new("Internal"))?
}

/// The real thing: `pub async fn`, the guard, `spawn_blocking`, `.await`,
/// `or_raise`, and the sink two calls below in the closure. This is the shape of
/// every handler in `backend/src/router/{get,put,post}`.
pub async fn b12_get_rows_full_shape(
    auth: GuardResult<GuardTimestamp>,
    index: usize,
    timestamp: i64,
    rows: usize,
) -> AppResult<Json<Row>> {
    let _ = auth?;
    spawn_blocking_like(move || -> AppResult<Json<Row>> {
        let filtered_rows = TREE_SNAPSHOT
            .read_row_prefix(index, timestamp, rows)
            .map_err(map_snapshot_read_error)?;
        Ok(Json(filtered_rows))
    })
    .await
    .or_raise(|| AppError::new("Internal"))?
}

/// The control for `b12`: a panic that is certain rather than possible, in the
/// same handler position. If this is silent, the coroutine body was never
/// entered, whatever the inputs are.
pub async fn b13_get_rows_async_concrete_sink() {
    let missing: Option<u8> = None;
    missing.unwrap();
}

// ------------------------------------------- 3: discarded versus propagated guard

/// The pre-fix handler with the guard result dropped instead of propagated:
/// `let _ = auth;`.
pub fn b14_get_scroll_bar_guard_discarded(
    auth: GuardResult<GuardTimestamp>,
    timestamp: i64,
) -> AppResult<Json<Vec<ScrollBarData>>> {
    let _ = auth;
    let scrollbar_data = TREE_SNAPSHOT.read_scrollbar_prefix(timestamp);
    Ok(Json(scrollbar_data))
}

/// The same handler with `let _ = auth?;`, which is what the current code does.
pub fn b15_get_scroll_bar_guard_propagated(
    auth: GuardResult<GuardTimestamp>,
    timestamp: i64,
) -> AppResult<Json<Vec<ScrollBarData>>> {
    let _ = auth?;
    let scrollbar_data = TREE_SNAPSHOT.read_scrollbar_prefix(timestamp);
    Ok(Json(scrollbar_data))
}

// ------------------------------- 4: why `b1` is silent, and where the Err goes

/// The control for `b1`: the same `Result::expect` on a `Result` this function
/// builds itself out of the same parameter. If this is reported and `b1` is not,
/// then the sink is modelled and what differs is the reachability of the `Err`
/// branch inside `read_tree_snapshot`.
pub fn b16_local_result_expect(timestamp: i64) -> i64 {
    let r: Result<i64, SnapshotReadError> = if timestamp < 0 {
        Err(SnapshotReadError::NotFound { timestamp })
    } else {
        Ok(timestamp)
    };
    r.expect("snapshot id must be known")
}

/// `read_tree_snapshot` with the table scan written out as an `if`, no loop.
pub fn b17_snapshot_err_without_loop(timestamp: i64) -> &'static Snapshot {
    if SNAPSHOTS[0].dates.first() == Some(&timestamp) {
        return &SNAPSHOTS[0];
    }
    Err(SnapshotReadError::NotFound { timestamp }).expect("failed to read tree snapshot")
}

/// The same, with the table scan as the loop `read_tree_snapshot` actually uses.
pub fn b18_snapshot_err_with_loop(timestamp: i64) -> &'static Snapshot {
    let mut found: Option<&'static Snapshot> = None;
    for snapshot in SNAPSHOTS.iter() {
        if snapshot.dates.first() == Some(&timestamp) {
            found = Some(snapshot);
        }
    }
    found.expect("failed to read tree snapshot")
}
