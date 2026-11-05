//! No-op surface for [`crate::tasks::observe`] when the `observe` feature is off.
//!
//! Call sites compile unchanged: [`Timer`] and [`timed`] exist with the same
//! signatures, [`snapshot`] returns an empty [`Snapshot`], and [`record`],
//! [`reset`] and [`drain_report`] do nothing. Nothing here reads a clock,
//! touches an atomic, or allocates.
//!
//! [`Stage`] mirrors the real module's variants so the same call sites type
//! check. Keep the two definitions in sync when adding a stage.

// The stub deliberately exposes the real module's shape even where the
// disabled build has no consumer for a field or variant.
#![allow(dead_code)]

/// Samples retained per stage. The stub retains none.
pub const RING_CAPACITY: usize = 256;

/// Minimum spacing between breach lines for the same stage.
pub const REPORT_INTERVAL_MS: u64 = 5_000;

/// One recorded stage. The numeric value is the slot index in the recorder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Stage {
    FilterSnapshotClone = 0,
    FilterParIter,
    ComputeLocate,
    TreeRefreshTotal,
    TreeRefreshBuild,
    TreeRefreshSort,
    TreeRefreshWriteWait,
    CpuPermitWait,
    CpuExec,
    PrefetchTotal,
}

/// Number of stages, must match [`Stage`]'s variants.
pub const STAGE_COUNT: usize = 10;

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
        }
    }

    /// Breach threshold in microseconds (see the real module's table).
    #[must_use]
    pub const fn threshold_us(self) -> u64 {
        match self {
            Self::FilterSnapshotClone => 10_000,
            Self::FilterParIter => 50_000,
            Self::ComputeLocate | Self::CpuPermitWait => 25_000,
            Self::TreeRefreshTotal => 250_000,
            Self::TreeRefreshBuild => 150_000,
            Self::TreeRefreshSort => 100_000,
            Self::TreeRefreshWriteWait => 20_000,
            Self::CpuExec | Self::PrefetchTotal => 1_000_000,
        }
    }
}

/// Aggregated view of one stage; always empty without the feature.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StageSummary {
    pub samples: usize,
    pub recorded: usize,
    pub p50_us: u64,
    pub p95_us: u64,
    pub max_us: u64,
    pub in_flight: usize,
    pub breaches: u64,
}

/// Point-in-time aggregation of every stage; always empty without the feature.
#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot;

impl Snapshot {
    /// Summary for one stage.
    #[must_use]
    pub fn stage(self, _stage: Stage) -> StageSummary {
        let _ = self;
        StageSummary::default()
    }

    /// Compact per-stage table; empty without the feature.
    #[must_use]
    pub fn table(self) -> String {
        let _ = self;
        String::new()
    }
}

/// Timer for an explicit start/finish section. Start and finish are no-ops.
#[must_use]
pub struct Timer {
    _stage: Stage,
}

impl Timer {
    /// Start timing `stage` (no-op).
    pub fn start(stage: Stage) -> Self {
        Self { _stage: stage }
    }

    /// Stop timing (no-op). Always returns 0.
    pub fn finish(self, _context: u64) -> u64 {
        let _ = self;
        0
    }
}

/// Run `f` without timing it.
#[must_use]
pub fn timed<T>(_stage: Stage, _context: u64, f: impl FnOnce() -> T) -> T {
    f()
}

/// Record a completed sample (no-op).
pub fn record(_stage: Stage, _duration_us: u64, _context: u64) {}

/// Aggregate every stage; always empty without the feature.
#[must_use]
pub fn snapshot() -> Snapshot {
    Snapshot
}

/// Clear retained samples and counters (no-op).
pub fn reset() {}

/// Emit the once-per-scan table and reset for the next scan (no-op).
pub fn drain_report(_context: &str) {}
