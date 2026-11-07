use crate::tasks::runtime::RUNTIME;
use mini_executor::TaskExecutor;
use std::sync::LazyLock;

pub mod actor;
pub mod batcher;
pub mod looper;
// The real recorder under `task-observer` (default); a signature-compatible no-op
// otherwise so call sites compile unchanged.
#[cfg_attr(not(feature = "task-observer"), path = "observe_stub.rs")]
pub mod observe;
pub mod queue;
pub mod runtime;

pub static BATCH_COORDINATOR: LazyLock<TaskExecutor> =
    LazyLock::new(|| TaskExecutor::new(&RUNTIME));

/// Exercises the call-site surface in both feature modes: the real recorder
/// and the no-op stub must both accept these calls without panicking.
#[cfg(test)]
mod observe_smoke {
    use super::observe::{self, Stage, Timer};

    #[test]
    fn timed_runs_closure_and_drain_is_safe() {
        observe::reset();
        assert_eq!(observe::timed(Stage::CpuExec, 0, || 40 + 2), 42);

        let duration_us = Timer::start(Stage::PrefetchTotal).finish(0);
        let _ = duration_us;

        let snapshot = observe::snapshot();
        let summary = snapshot.stage(Stage::CpuExec);
        let _ = summary.samples;
        let _ = snapshot.table();

        observe::record(Stage::CpuExec, 1, 0);
        // Flag parity: both feature modes expose the opt-in accessors.
        observe::set_observer_stats(true);
        assert!(observe::observer_stats());
        observe::set_observer_stats(false);
        assert!(!observe::observer_stats());
        observe::drain_report("observe smoke");
        observe::reset();
    }
}
