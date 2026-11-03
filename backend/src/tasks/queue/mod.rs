//! Bounded scan intake with path coalescing (slice 2a).
//!
//! The walk in `actor::album_index` submits one [`IntakeItem`] per matched
//! file instead of spawning a task per file. Items carry a [`Priority`] lane
//! (only `Scan` is submitted in this slice), a per-path generation, and the
//! file identity observed at enqueue. A fixed set of workers (see
//! [`worker`]) dequeues items and runs them through
//! `workflow::process_file`; completion is reported through the
//! per-job barrier in [`worker::drain_intake`].

pub mod intake;
pub mod priority;
pub mod worker;

pub use intake::submit_scan;
pub use worker::{barrier_flush_tree, barrier_update_tree, drain_intake};
