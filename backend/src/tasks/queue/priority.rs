//! Priority lanes for background intake items, strongest first:
//! `Interactive > Watcher > Scan > Expire > Verify`.
//!
//! Only `Scan` is submitted in this slice; the remaining lanes are reserved
//! for slice 2b (watcher debounce, expire, verify/GC), which adds lane
//! scheduling and admittance on top of this envelope.

/// Background-job priority lane carried on every intake item.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Priority {
    /// Interactive request-path work (fastest lane, slice 2b).
    Interactive,
    /// Filesystem-watcher debounce work (slice 2b).
    Watcher,
    /// Walk-driven scan work (the only submitted lane in this slice).
    Scan,
    /// Expiry work (slice 2b).
    Expire,
    /// Verify/GC work (slowest lane, slice 2b).
    Verify,
}
