//! Bounded, always-on timing record for the tree/snapshot pipeline.
//!
//! This is long-lived monitoring, not a debug trace: normal operations record
//! a sample and emit nothing. Only a stage that exceeds its documented
//! threshold emits a single rate-limited `warn!` line (at most one per stage
//! per [`REPORT_INTERVAL_MS`]), and the scan-completion hook emits one compact
//! per-stage table per scan.
//!
//! Compiled only with the `observe` feature (on by default). `observe_stub.rs`
//! provides a signature-compatible no-op surface when the feature is off.
//!
//! # What is recorded
//!
//! Every [`record`] call stores a [`Sample`] in that stage's fixed-size ring
//! ([`RING_CAPACITY`] samples, oldest overwritten). Each stage also keeps
//! monotonic counters: total recorded, currently in flight, threshold
//! breaches, and ring wraps. A [`Snapshot`] aggregates the ring into `n`,
//! p50, p95 and max.
//! Percentiles use the nearest-rank method on the retained ring, not the full
//! lifetime history, so they describe recent behaviour and reset at phase
//! boundaries.
//!
//! A second, rate-limited guard watches the ring itself: when a push evicts
//! the oldest sample (a wrap), one `warn!` line dumps the current per-stage
//! table. This is the safety net for event *rates* the duration thresholds
//! miss — by the time the ring churns, something is already wrong, so the
//! dump's own cost lands on top of an observed failure, never on the
//! healthy path.
//!
//! # Thresholds
//!
//! Thresholds are deliberately an order of magnitude above normal operation;
//! they are not service-level objectives. A breach means "this stage was
//! blocked or overloaded, look at the attached queue state".
//!
//! | stage | threshold (µs) | why |
//! | --- | --- | --- |
//! | `filter_snapshot_clone` | `10_000` | cloning the current tree, one allocation-heavy walk; above 10 ms means the read guard or allocator is stalling |
//! | `filter_par_iter` | `50_000` | the `Rayon` join over the snapshot; above 50 ms means pool scheduling, not row work |
//! | `compute_locate` | `25_000` | the parallel position scan; same scheduling exposure with a smaller payload |
//! | `tree_refresh_total` | `250_000` | one full cascade rebuild end to end |
//! | `tree_refresh_build` | `150_000` | redb iteration plus row composition |
//! | `tree_refresh_sort` | `100_000` | parallel sort of the rebuilt rows |
//! | `tree_refresh_write_wait` | `20_000` | acquiring the tree write guard; long waits mean a reader (prefetch) holds it |
//! | `cpu_permit_wait` | `25_000` | acquiring one of the `N-1` admittance permits; a long wait means the pool is saturated |
//! | `cpu_exec` | `1_000_000` | a bridged closure body; single-image indexing reaches several hundred milliseconds, so only a second or more is an outlier |
//! | `prefetch_total` | `1_000_000` | the whole prefetch body; storm prefetch med ~3 ms and p95 ~15 ms, with 100 ms–3 s outliers, so 1 s marks the multi-second tail |
//! | `blocking_queue_wait` | `50_000` | Tokio blocking-pool queueing before prefetch work starts; dispatch normally takes microseconds |
//! | `interactive_exec` | `500_000` | interactive-lane job bodies; they must stay short, so half a second flags a hog |
//! | `blocking_queue_wait` | `50_000` | Tokio blocking-pool queueing before prefetch work starts; dispatch is normally microseconds, so 50 ms means the blocking pool itself is backlogged |
//!
//! # Synchronization
//!
//! Samples use a per-slot seqlock: writers publish odd/even sequence stamps
//! and readers discard torn slots. Counters are atomics. There is no mutex and
//! no I/O on the recording path. The breach path (rate-limited) takes a ring
//! snapshot and reads two cheap atomics sets to describe queue state.

use std::fmt::Write as _;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering, fence};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use log::{info, warn};

/// Samples retained per stage. 256 is enough for stable p50/p95 inside one
/// scan while keeping the whole record under ~100 KiB.
pub const RING_CAPACITY: usize = 256;

/// Minimum spacing between breach lines for the same stage.
pub const REPORT_INTERVAL_MS: u64 = 5_000;

/// One recorded stage. The numeric value is the slot index in the recorder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Stage {
    /// `filter_items`: cloning the current tree out from under the read guard.
    FilterSnapshotClone = 0,
    /// `filter_items`: the `par_iter` filter/collect over the clone.
    FilterParIter,
    /// `compute_locate`: the parallel position lookup for `locate`.
    ComputeLocate,
    /// `refresh_tree_cache`: total wall time.
    TreeRefreshTotal,
    /// `refresh_tree_cache`: rebuild from the asset tables.
    TreeRefreshBuild,
    /// `refresh_tree_cache`: the parallel sort.
    TreeRefreshSort,
    /// `refresh_tree_cache`: waiting for the in-memory tree write guard.
    TreeRefreshWriteWait,
    /// Bridge: waiting for a CPU admittance permit.
    CpuPermitWait,
    /// Bridge: executing the bridged closure on the `Rayon` pool.
    CpuExec,
    /// The whole prefetch body: cache lookup, filter, locate, snapshot insert
    /// and response construction.
    PrefetchTotal,
    /// Time spent queued for a Tokio blocking-pool thread before prefetch
    /// work starts (measured at the handoff, not inside the job).
    BlockingQueueWait,
    /// Interactive-lane work on the dedicated pool (spawned via the bridge,
    /// never admitted through the bulk semaphore).
    InteractiveExec,
}

/// Number of stages, must match [`Stage`]'s variants.
pub const STAGE_COUNT: usize = 12;

// Breach thresholds in microseconds. They are compile-time defaults, not
// runtime configuration: each is deliberately an order of magnitude above
// normal operation so that only blocked or overloaded stages report.
/// Cloning the tree is one walk, normally well under a millisecond; 10 ms
/// means the read guard or allocator is stalling (storm prefetch p95 ~15 ms
/// bounds the whole request).
const FILTER_SNAPSHOT_CLONE_US: u64 = 10_000;
/// The `Rayon` join over the clone: row work is microseconds per row, so
/// 50 ms points at pool scheduling rather than filtering.
const FILTER_PAR_ITER_US: u64 = 50_000;
/// The parallel position scan: same scheduling exposure with a smaller
/// payload, hence half the join budget.
const COMPUTE_LOCATE_US: u64 = 25_000;
/// One full cascade rebuild end to end; normal magnitude is tens of ms.
const TREE_REFRESH_TOTAL_US: u64 = 250_000;
/// Redb iteration plus row composition; 150 ms indicates store pressure.
const TREE_REFRESH_BUILD_US: u64 = 150_000;
/// Parallel sort of the rebuilt rows; normally the cheapest refresh phase.
const TREE_REFRESH_SORT_US: u64 = 100_000;
/// Acquiring the tree write guard; 20 ms means a reader (prefetch) holds it.
const TREE_REFRESH_WRITE_WAIT_US: u64 = 20_000;
/// Acquiring one of the `N-1` admittance permits; a long wait means the pool
/// is saturated.
const CPU_PERMIT_WAIT_US: u64 = 25_000;
/// A bridged closure body; single-image indexing reaches several hundred
/// milliseconds, so only a second or more is an outlier.
const CPU_EXEC_US: u64 = 1_000_000;
/// Whole prefetch body. Storm prefetch med ~3 ms and p95 ~15 ms, with
/// 100 ms–3 s outliers: 1 s isolates the multi-second tail without firing
/// on the ordinary 100 ms stalls.
const PREFETCH_TOTAL_US: u64 = 1_000_000;
/// Tokio blocking-pool queueing before prefetch work starts; measured at
/// the handoff. Dispatch normally takes microseconds; 50 ms means the
/// blocking pool is backlogged, not that filtering is slow.
const BLOCKING_QUEUE_WAIT_US: u64 = 50_000;
/// Interactive-lane jobs run without bulk admittance on a dedicated pool;
/// 500 ms flags an interactive job that stopped being interactive.
const INTERACTIVE_EXEC_US: u64 = 500_000;

impl Stage {
    /// Every stage in table order.
    pub const ALL: [Self; STAGE_COUNT] = [
        Self::FilterSnapshotClone,
        Self::FilterParIter,
        Self::ComputeLocate,
        Self::TreeRefreshTotal,
        Self::TreeRefreshBuild,
        Self::TreeRefreshSort,
        Self::TreeRefreshWriteWait,
        Self::CpuPermitWait,
        Self::CpuExec,
        Self::PrefetchTotal,
        Self::BlockingQueueWait,
        Self::InteractiveExec,
    ];

    /// Stable snake-case name used in logs and tables.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::FilterSnapshotClone => "filter_snapshot_clone",
            Self::FilterParIter => "filter_par_iter",
            Self::ComputeLocate => "compute_locate",
            Self::TreeRefreshTotal => "tree_refresh_total",
            Self::TreeRefreshBuild => "tree_refresh_build",
            Self::TreeRefreshSort => "tree_refresh_sort",
            Self::TreeRefreshWriteWait => "tree_refresh_write_wait",
            Self::CpuPermitWait => "cpu_permit_wait",
            Self::CpuExec => "cpu_exec",
            Self::PrefetchTotal => "prefetch_total",
            Self::BlockingQueueWait => "blocking_queue_wait",
            Self::InteractiveExec => "interactive_exec",
        }
    }

    /// Breach threshold in microseconds (see the module table).
    #[must_use]
    pub const fn threshold_us(self) -> u64 {
        match self {
            Self::FilterSnapshotClone => FILTER_SNAPSHOT_CLONE_US,
            Self::FilterParIter => FILTER_PAR_ITER_US,
            Self::ComputeLocate => COMPUTE_LOCATE_US,
            Self::TreeRefreshTotal => TREE_REFRESH_TOTAL_US,
            Self::TreeRefreshBuild => TREE_REFRESH_BUILD_US,
            Self::TreeRefreshSort => TREE_REFRESH_SORT_US,
            Self::TreeRefreshWriteWait => TREE_REFRESH_WRITE_WAIT_US,
            Self::CpuPermitWait => CPU_PERMIT_WAIT_US,
            Self::CpuExec => CPU_EXEC_US,
            Self::PrefetchTotal => PREFETCH_TOTAL_US,
            Self::BlockingQueueWait => BLOCKING_QUEUE_WAIT_US,
            Self::InteractiveExec => INTERACTIVE_EXEC_US,
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// One ring entry.
struct Sample {
    duration_us: u64,
    timestamp_ms: u64,
    context: u64,
}

/// One slot in a stage ring, published through a sequence stamp.
#[derive(Default)]
struct RingSlot {
    /// Even = stable, odd = a write is in progress.
    seq: AtomicU64,
    duration_us: AtomicU64,
    timestamp_ms: AtomicU64,
    context: AtomicU64,
}

struct StageRing {
    slots: [RingSlot; RING_CAPACITY],
    write_index: AtomicUsize,
    samples: AtomicUsize,
}

impl Default for StageRing {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| RingSlot::default()),
            write_index: AtomicUsize::new(0),
            samples: AtomicUsize::new(0),
        }
    }
}

impl StageRing {
    /// Store a sample. Returns true when the ring was already full, i.e.
    /// this push evicted the oldest retained sample (a wrap).
    fn push(&self, sample: &Sample) -> bool {
        let index = self.write_index.fetch_add(1, Ordering::Relaxed) % RING_CAPACITY;
        let slot = &self.slots[index];
        let seq = slot.seq.load(Ordering::Relaxed);
        slot.seq.store(seq.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        slot.duration_us
            .store(sample.duration_us, Ordering::Relaxed);
        slot.timestamp_ms
            .store(sample.timestamp_ms, Ordering::Relaxed);
        slot.context.store(sample.context, Ordering::Relaxed);
        fence(Ordering::Release);
        slot.seq.store(seq.wrapping_add(2), Ordering::Release);
        self.samples.fetch_add(1, Ordering::Relaxed) >= RING_CAPACITY
    }

    fn clear(&self) {
        self.write_index.store(0, Ordering::Relaxed);
        self.samples.store(0, Ordering::Relaxed);
        for slot in &self.slots {
            slot.seq.store(0, Ordering::Release);
        }
    }

    /// Durations currently retained in the ring, torn slots skipped.
    fn durations(&self) -> Vec<u64> {
        let mut out = Vec::with_capacity(RING_CAPACITY);
        for slot in &self.slots {
            let before = slot.seq.load(Ordering::Acquire);
            if before == 0 || before % 2 != 0 {
                continue;
            }
            let duration = slot.duration_us.load(Ordering::Relaxed);
            // Reading every published field also makes the sequence check
            // cover the whole sample, not just the duration.
            let _timestamp_ms = slot.timestamp_ms.load(Ordering::Relaxed);
            let _context = slot.context.load(Ordering::Relaxed);
            fence(Ordering::Acquire);
            let after = slot.seq.load(Ordering::Relaxed);
            if before == after {
                out.push(duration);
            }
        }
        out
    }
}

struct StageState {
    ring: StageRing,
    in_flight: AtomicUsize,
    breaches: AtomicU64,
    last_report_ms: AtomicU64,
    wraps: AtomicU64,
    last_wrap_report_ms: AtomicU64,
}

impl Default for StageState {
    fn default() -> Self {
        Self {
            ring: StageRing::default(),
            in_flight: AtomicUsize::new(0),
            breaches: AtomicU64::new(0),
            last_report_ms: AtomicU64::new(0),
            wraps: AtomicU64::new(0),
            last_wrap_report_ms: AtomicU64::new(0),
        }
    }
}

/// Aggregated view of one stage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StageSummary {
    /// Samples retained in the ring (at most [`RING_CAPACITY`]).
    pub samples: usize,
    /// Samples recorded since the last reset (may exceed `samples`).
    pub recorded: usize,
    /// 50th percentile of the retained samples, microseconds.
    pub p50_us: u64,
    /// 95th percentile of the retained samples, microseconds.
    pub p95_us: u64,
    /// Largest retained sample, microseconds.
    pub max_us: u64,
    /// Operations of this stage currently in flight.
    pub in_flight: usize,
    /// Threshold breaches since the last reset.
    pub breaches: u64,
}

/// Point-in-time aggregation of every stage.
#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot {
    stages: [StageSummary; STAGE_COUNT],
}

impl Snapshot {
    /// Summary for one stage.
    #[must_use]
    pub fn stage(&self, stage: Stage) -> StageSummary {
        self.stages[stage.index()]
    }

    /// Compact per-stage table (`n`, p50, p95, max) for the drain report.
    #[must_use]
    pub fn table(&self) -> String {
        let mut out = String::from("  stage                     n   p50_us   p95_us    max_us\n");
        for stage in Stage::ALL {
            let summary = self.stage(stage);
            let _ = writeln!(
                out,
                "  {:<22} {:>3} {:>8} {:>8} {:>8}",
                stage.name(),
                summary.samples,
                summary.p50_us,
                summary.p95_us,
                summary.max_us
            );
        }
        out
    }
}

struct Recorder {
    stages: [StageState; STAGE_COUNT],
}

/// What `Recorder::record_at` claimed for one sample, if anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Report {
    /// Slow sample; the caller logs the per-op breakdown.
    Breach,
    /// Ring wrapped since the last wrap report; the caller dumps the
    /// current per-stage table. Count is pending evictions at claim time.
    Wrap(u64),
}

impl Recorder {
    fn new() -> Self {
        Self {
            stages: std::array::from_fn(|_| StageState::default()),
        }
    }

    fn state(&self, stage: Stage) -> &StageState {
        &self.stages[stage.index()]
    }

    fn enter(&self, stage: Stage) {
        self.state(stage).in_flight.fetch_add(1, Ordering::Relaxed);
    }

    fn leave(&self, stage: Stage) {
        self.state(stage).in_flight.fetch_sub(1, Ordering::Relaxed);
    }

    /// Record one sample. Returns which report (if any) this call claimed:
    /// a threshold breach wins over a ring wrap when both apply.
    fn record_at(
        &self,
        stage: Stage,
        duration_us: u64,
        context: u64,
        now_ms: u64,
    ) -> Option<Report> {
        let cell = self.state(stage);
        if cell.ring.push(&Sample {
            duration_us,
            timestamp_ms: wall_ms(),
            context,
        }) {
            cell.wraps.fetch_add(1, Ordering::Relaxed);
        }
        if duration_us >= stage.threshold_us() && claim_report(&cell.last_report_ms, now_ms) {
            cell.breaches.fetch_add(1, Ordering::Relaxed);
            return Some(Report::Breach);
        }
        // Pending wraps survive rate limiting: a churning stage keeps one
        // wrap report queued until its interval passes.
        if cell.wraps.load(Ordering::Relaxed) > 0 && claim_report(&cell.last_wrap_report_ms, now_ms)
        {
            return Some(Report::Wrap(cell.wraps.swap(0, Ordering::Relaxed)));
        }
        None
    }

    fn summary(&self, stage: Stage) -> StageSummary {
        let cell = self.state(stage);
        let mut durations = cell.ring.durations();
        durations.sort_unstable();
        StageSummary {
            samples: durations.len(),
            recorded: cell.ring.samples.load(Ordering::Relaxed),
            p50_us: percentile(&durations, 50),
            p95_us: percentile(&durations, 95),
            max_us: durations.last().copied().unwrap_or(0),
            in_flight: cell.in_flight.load(Ordering::Relaxed),
            breaches: cell.breaches.load(Ordering::Relaxed),
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            stages: std::array::from_fn(|index| self.summary(Stage::ALL[index])),
        }
    }

    /// Clear retained samples and counters. In-flight counts are preserved:
    /// an operation that started before the boundary still decrements after.
    fn reset(&self) {
        for stage in Stage::ALL {
            let cell = self.state(stage);
            cell.ring.clear();
            cell.breaches.store(0, Ordering::Relaxed);
            cell.last_report_ms.store(0, Ordering::Relaxed);
            cell.wraps.store(0, Ordering::Relaxed);
            cell.last_wrap_report_ms.store(0, Ordering::Relaxed);
        }
    }
}

static OBSERVE: LazyLock<Recorder> = LazyLock::new(Recorder::new);

static MONO_START: LazyLock<Instant> = LazyLock::new(Instant::now);

fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Monotonic milliseconds for rate gates (immune to wall-clock jumps).
pub(crate) fn mono_ms() -> u64 {
    u64::try_from(MONO_START.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn elapsed_us(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
}

/// True when `now` is at least one report interval after `last`.
fn report_due(last: &AtomicU64, now: u64) -> bool {
    now.saturating_sub(last.load(Ordering::Relaxed)) >= REPORT_INTERVAL_MS
}

/// Claim the per-stage report slot if the interval has passed.
/// Rate-limit gate shared by breach and wrap reports.
pub(crate) fn claim_report(last: &AtomicU64, now: u64) -> bool {
    if !report_due(last, now) {
        return false;
    }
    let previous = last.load(Ordering::Relaxed);
    if now.saturating_sub(previous) < REPORT_INTERVAL_MS {
        return false;
    }
    last.compare_exchange(previous, now, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
}

/// Nearest-rank percentile over already-sorted input. Returns 0 when empty.
fn percentile(sorted: &[u64], pct: u8) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let last = sorted.len() - 1;
    let index = (last * usize::from(pct) + 50) / 100;
    sorted[index.min(last)]
}

/// Timer for an explicit start/finish section.
///
/// The constructor increments the stage's in-flight counter; either
/// [`Timer::finish`] or drop decrements it exactly once. A timer dropped
/// without `finish` produces no sample (for early-return paths).
pub struct Timer {
    stage: Option<Stage>,
    start: Instant,
}

impl Timer {
    /// Start timing `stage`.
    #[must_use]
    pub fn start(stage: Stage) -> Self {
        OBSERVE.enter(stage);
        Self {
            stage: Some(stage),
            start: Instant::now(),
        }
    }

    /// Stop timing, record the sample and return its duration.
    pub fn finish(mut self, context: u64) -> u64 {
        let duration_us = elapsed_us(self.start);
        let Some(stage) = self.stage.take() else {
            return duration_us;
        };
        OBSERVE.leave(stage);
        record(stage, duration_us, context);
        duration_us
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(stage) = self.stage.take() {
            OBSERVE.leave(stage);
        }
    }
}

/// Time `f` under `stage` with a fixed `context` value.
#[must_use]
pub fn timed<T>(stage: Stage, context: u64, f: impl FnOnce() -> T) -> T {
    let timer = Timer::start(stage);
    let out = f();
    timer.finish(context);
    out
}

/// Record a completed sample. Emits at most one rate-limited breach or
/// wrap line; a wrap dumps the current per-stage table as the safety net
/// for event rates the thresholds miss.
pub fn record(stage: Stage, duration_us: u64, context: u64) {
    match OBSERVE.record_at(stage, duration_us, context, mono_ms()) {
        Some(Report::Breach) => emit_breach(stage, duration_us, context),
        Some(Report::Wrap(wraps)) => emit_wrap(stage, wraps),
        None => {}
    }
}

/// Aggregate every stage. Does not reset.
#[must_use]
pub fn snapshot() -> Snapshot {
    OBSERVE.snapshot()
}

/// Clear retained samples and counters at a phase boundary. In-flight
/// counters are preserved.
pub fn reset() {
    OBSERVE.reset();
}

/// Emit the once-per-scan table and reset for the next scan.
///
/// Called from the scan terminal transition, not per round.
pub fn drain_report(context: &str) {
    let table = snapshot().table();
    info!("observe drain: {context}\n{table}");
    OBSERVE.reset();
}

fn emit_breach(stage: Stage, duration_us: u64, context: u64) {
    let summary = OBSERVE.summary(stage);
    let permits = crate::tasks::queue::bridge::available_cpu_permits();
    let (next, writer, cascade) = crate::tasks::queue::writer::watermarks();
    let mut in_flight = String::new();
    for other in Stage::ALL {
        if !in_flight.is_empty() {
            in_flight.push(' ');
        }
        let _ = write!(
            in_flight,
            "{}={}",
            other.name(),
            OBSERVE.state(other).in_flight.load(Ordering::Relaxed)
        );
    }
    warn!(
        "observe breach: stage={} duration_us={} threshold_us={} context={} | n={} recorded={} p50_us={} p95_us={} max_us={} breaches={} | in_flight [{}] | cpu_permits_available={} seq[next={} writer={} cascade={}]",
        stage.name(),
        duration_us,
        stage.threshold_us(),
        context,
        summary.samples,
        summary.recorded,
        summary.p50_us,
        summary.p95_us,
        summary.max_us,
        summary.breaches,
        in_flight,
        permits,
        next,
        writer,
        cascade
    );
}

fn emit_wrap(stage: Stage, wraps: u64) {
    let table = OBSERVE.snapshot().table();
    let permits = crate::tasks::queue::bridge::available_cpu_permits();
    let (next, writer, cascade) = crate::tasks::queue::writer::watermarks();
    warn!(
        "observe wrap: stage={} wraps={} ring dumping current content\n{} | cpu_permits_available={} seq[next={} writer={} cascade={}]",
        stage.name(),
        wraps,
        table,
        permits,
        next,
        writer,
        cascade
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage_state(recorder: &Recorder, stage: Stage) -> &StageState {
        recorder.state(stage)
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let sorted = [10, 20, 30, 40, 50];
        assert_eq!(percentile(&sorted, 50), 30);
        assert_eq!(percentile(&sorted, 95), 50);
        assert_eq!(percentile(&sorted, 0), 10);
        assert_eq!(percentile(&sorted, 100), 50);
        assert_eq!(percentile(&[], 95), 0);
        assert_eq!(percentile(&[7], 95), 7);
    }

    #[test]
    fn ring_keeps_only_the_last_capacity_samples() {
        let recorder = Recorder::new();
        for now_ms in 1..=300 {
            recorder.record_at(Stage::CpuExec, now_ms, 0, now_ms);
        }
        let summary = recorder.summary(Stage::CpuExec);
        assert_eq!(summary.recorded, 300);
        assert_eq!(summary.samples, RING_CAPACITY);
        assert_eq!(summary.max_us, 300);
        assert_eq!(summary.p50_us, 173);
    }

    #[test]
    fn empty_stage_aggregates_to_zero() {
        let recorder = Recorder::new();
        let summary = recorder.summary(Stage::ComputeLocate);
        assert_eq!(summary, StageSummary::default());
    }

    #[test]
    fn threshold_breach_requires_threshold_and_window() {
        let recorder = Recorder::new();
        let below = Stage::FilterParIter.threshold_us() - 1;
        assert!(
            recorder
                .record_at(Stage::FilterParIter, below, 0, 100_000)
                .is_none()
        );
        assert!(
            recorder
                .record_at(
                    Stage::FilterParIter,
                    Stage::FilterParIter.threshold_us(),
                    0,
                    100_000
                )
                .is_some()
        );
        // Within the window: no second report award.
        assert!(
            recorder
                .record_at(
                    Stage::FilterParIter,
                    Stage::FilterParIter.threshold_us(),
                    0,
                    100_000 + REPORT_INTERVAL_MS - 1,
                )
                .is_none()
        );
        // After the window: report again.
        assert!(
            recorder
                .record_at(
                    Stage::FilterParIter,
                    Stage::FilterParIter.threshold_us(),
                    0,
                    100_000 + REPORT_INTERVAL_MS,
                )
                .is_some()
        );
        assert_eq!(
            stage_state(&recorder, Stage::FilterParIter)
                .breaches
                .load(Ordering::Relaxed),
            2
        );
    }

    #[test]
    fn report_due_is_per_stage_and_clock_safe() {
        let last = AtomicU64::new(10_000);
        assert!(!report_due(&last, 10_000 + REPORT_INTERVAL_MS - 1));
        assert!(report_due(&last, 10_000 + REPORT_INTERVAL_MS));
        // A clock that appears to move backwards does not report.
        assert!(!report_due(&last, 5_000));
    }

    #[test]
    fn reset_clears_samples_but_preserves_in_flight() {
        let recorder = Recorder::new();
        recorder.enter(Stage::FilterParIter);
        recorder.record_at(Stage::FilterParIter, 1_000, 42, 0);
        assert_eq!(recorder.summary(Stage::FilterParIter).samples, 1);
        assert_eq!(recorder.summary(Stage::FilterParIter).in_flight, 1);

        recorder.reset();

        let summary = recorder.summary(Stage::FilterParIter);
        assert_eq!(summary.samples, 0);
        assert_eq!(summary.recorded, 0);
        assert_eq!(summary.breaches, 0);
        assert_eq!(summary.in_flight, 1);
        recorder.leave(Stage::FilterParIter);
    }

    #[test]
    fn wrap_reports_once_per_window_with_dump_armed() {
        let recorder = Recorder::new();
        // Quiet samples below every threshold: the first ring fills silently.
        for _ in 0..RING_CAPACITY {
            assert_eq!(
                recorder.record_at(Stage::ComputeLocate, 1, 0, 100_000),
                None
            );
        }
        // Push 257 overflows the ring: first wrap report fires, carrying n=1.
        assert!(matches!(
            recorder.record_at(Stage::ComputeLocate, 1, 0, 100_000),
            Some(Report::Wrap(1))
        ));
        // Churn continues but the window suppresses repeats.
        assert_eq!(
            recorder.record_at(Stage::ComputeLocate, 1, 0, 100_000),
            None
        );
        // After one interval a fresh wrap report fires with the backlog.
        assert!(matches!(
            recorder.record_at(Stage::ComputeLocate, 1, 0, 100_000 + REPORT_INTERVAL_MS),
            Some(Report::Wrap(_))
        ));
    }

    #[test]
    fn breach_wins_over_wrap_and_wrap_survives() {
        let recorder = Recorder::new();
        for _ in 0..RING_CAPACITY {
            assert_eq!(
                recorder.record_at(Stage::ComputeLocate, 1, 0, 200_000),
                None
            );
        }
        // A breaching sample on a wrapped ring reports the breach, not the wrap...
        let threshold = Stage::ComputeLocate.threshold_us();
        assert!(matches!(
            recorder.record_at(Stage::ComputeLocate, threshold, 0, 200_000),
            Some(Report::Breach)
        ));
        // ...and the pending wrap still fires once its own window allows.
        assert!(matches!(
            recorder.record_at(Stage::ComputeLocate, 1, 0, 200_000 + REPORT_INTERVAL_MS),
            Some(Report::Wrap(_))
        ));
    }

    #[test]
    fn reset_clears_pending_wraps() {
        let recorder = Recorder::new();
        for _ in 0..=RING_CAPACITY {
            recorder.record_at(Stage::ComputeLocate, 1, 0, 300_000);
        }
        recorder.reset();
        // After reset the ring is empty: no wrap, no report.
        assert_eq!(
            recorder.record_at(Stage::ComputeLocate, 1, 0, 300_000 + REPORT_INTERVAL_MS),
            None
        );
        assert_eq!(
            stage_state(&recorder, Stage::ComputeLocate)
                .wraps
                .load(Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn in_flight_tracks_nested_entries() {
        let recorder = Recorder::new();
        recorder.enter(Stage::CpuPermitWait);
        recorder.enter(Stage::CpuPermitWait);
        recorder.leave(Stage::CpuPermitWait);
        assert_eq!(
            stage_state(&recorder, Stage::CpuPermitWait)
                .in_flight
                .load(Ordering::Relaxed),
            1
        );
        recorder.leave(Stage::CpuPermitWait);
        assert_eq!(
            stage_state(&recorder, Stage::CpuPermitWait)
                .in_flight
                .load(Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn snapshot_reports_every_stage() {
        let recorder = Recorder::new();
        recorder.record_at(Stage::FilterSnapshotClone, 5, 1, 0);
        recorder.record_at(Stage::CpuExec, 7, 2, 0);
        let snapshot = recorder.snapshot();
        assert_eq!(snapshot.stage(Stage::FilterSnapshotClone).samples, 1);
        assert_eq!(snapshot.stage(Stage::CpuExec).samples, 1);
        assert_eq!(snapshot.stage(Stage::ComputeLocate).samples, 0);
        assert_eq!(snapshot.table().lines().count(), STAGE_COUNT + 1);
    }

    #[test]
    fn timer_records_once_and_drop_is_safe() {
        reset();
        {
            let timer = Timer::start(Stage::CpuExec);
            drop(timer);
        }
        assert_eq!(snapshot().stage(Stage::CpuExec).samples, 0);
        assert_eq!(snapshot().stage(Stage::CpuExec).in_flight, 0);

        Timer::start(Stage::CpuExec).finish(9);
        assert_eq!(snapshot().stage(Stage::CpuExec).samples, 1);
        assert_eq!(snapshot().stage(Stage::CpuExec).in_flight, 0);
    }
}
