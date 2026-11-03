//! Bounded scan intake with path coalescing, priority lanes, and aging.
//!
//! The walk in `actor::album_index` submits one item per matched file
//! instead of spawning a task per file; the watcher debounce and the expire
//! looper submit through the same lanes, and request-path album updates
//! arrive on the Interactive lane with a oneshot for the outcome. A fixed
//! set of workers (see [`worker`]) dequeues items in smooth weighted
//! round-robin lane order with aging (see [`priority`]) and runs them
//! through `workflow::process_file` or the matching pipeline. Completion of
//! scan items feeds the per-job barrier in [`worker::drain_intake`].

pub mod bridge;
pub mod intake;
pub mod priority;
pub mod worker;

pub use intake::{
    submit_expire_check, submit_interactive_album, submit_interactive_album_detached, submit_scan,
    submit_watcher_index, submit_watcher_remove,
};
pub use worker::{barrier_flush_tree, barrier_update_tree, drain_intake};
