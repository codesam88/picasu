//! Execution bridges for background work.
//!
//! This module (with `tasks/runtime.rs`) is the only place that may call
//! `tokio::task::spawn_blocking` or `rayon::spawn`: bulk CPU work enters the
//! shared Rayon pool here through an admittance semaphore, and short
//! blocking I/O enters the Tokio blocking pool here. Interactive work never
//! touches this module's bulk path: it runs on the dedicated interactive
//! pool via [`run_interactive`], so bulk saturation cannot starve it.
//! Orchestration code in `workflow` and the queue worker calls these bridges
//! instead of spawning directly, so the N-1 core budget holds by construction.
//!
//! Budget accounting: the semaphore caps concurrent bulk jobs at
//! `max(1, N-1)` on the N-thread bulk pool. Every bulk CPU section holds one
//! permit across the result await, so background CPU parallelism can never
//! exceed the pool no matter how many queue workers queue CPU work at once.
//! The permit moves into the Rayon closure and drops after the result is
//! sent, which keeps it held for the whole await.
//!
//! Ordering: bulk lanes (scan, expire) use LIFO `rayon::spawn`; the
//! interactive bridge uses FIFO `spawn_fifo` on its own pool so concurrent
//! request-path updates complete in arrival order. Neither bridge nests:
//! bridged closures never re-enter a bridge (each Rayon pool work-steals
//! nested `par_*` sections without extra permits).

use std::sync::{Arc, LazyLock};

use anyhow::Context;
use tokio::sync::Semaphore;

use crate::tasks::observe::{self, Stage};

/// Rayon admittance: caps concurrent bulk jobs below the bulk pool width.
/// A queue-depth bound, not a reservation: interactive latency is the
/// interactive pool's job.
static CPU_PERMITS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| {
    let threads =
        crate::tasks::runtime::admittance_permits(*crate::tasks::runtime::CURRENT_NUM_THREADS);
    Arc::new(Semaphore::new(threads))
});

/// Free admittance permits in the shared CPU budget. Diagnostics only.
#[cfg(feature = "observe")]
pub(crate) fn available_cpu_permits() -> usize {
    CPU_PERMITS.available_permits()
}

/// Run bulk-lane CPU work `f` on the shared Rayon pool (LIFO order).
pub async fn run_cpu<F, T>(f: F) -> anyhow::Result<T>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let permit_timer = observe::Timer::start(Stage::CpuPermitWait);
    let permit = CPU_PERMITS
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| anyhow::anyhow!("cpu admittance semaphore closed"))?;
    permit_timer.finish(u64::try_from(CPU_PERMITS.available_permits()).unwrap_or(u64::MAX));
    let (tx, rx) = tokio::sync::oneshot::channel();
    rayon::spawn(move || {
        let _permit = permit;
        let exec_timer = observe::Timer::start(Stage::CpuExec);
        let result = f();
        exec_timer.finish(0);
        let _ = tx.send(result);
    });
    rx.await
        .map_err(|_| anyhow::anyhow!("background cpu task dropped its result"))?
}

/// Run interactive-lane CPU work `f` on the dedicated interactive pool
/// (FIFO order). No admittance permit: interactive jobs are short and the
/// pool is sized for them, so they never queue behind bulk work — the OS
/// scheduler preempts the niced-down bulk pool instead.
pub async fn run_interactive<F, T>(f: F) -> anyhow::Result<T>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    crate::tasks::runtime::INTERACTIVE_POOL.spawn_fifo(move || {
        let exec_timer = observe::Timer::start(Stage::InteractiveExec);
        let result = f();
        exec_timer.finish(0);
        let _ = tx.send(result);
    });
    rx.await
        .map_err(|_| anyhow::anyhow!("interactive cpu task dropped its result"))?
}

/// Run short blocking I/O `f` on the Tokio blocking pool. For reads, writes,
/// and other brief blocking sections only — never CPU work, which would
/// evade the N-1 budget the [`run_cpu`] semaphore enforces.
pub async fn run_io<F, T>(f: F) -> anyhow::Result<T>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .context("blocking worker failed")?
}
