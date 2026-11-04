//! Execution bridges for background work.
//!
//! This module (with `tasks/runtime.rs`) is the only place that may call
//! `tokio::task::spawn_blocking` or `rayon::spawn`: CPU work enters the
//! shared Rayon pool here through an admittance semaphore, and short
//! blocking I/O enters the Tokio blocking pool here. Orchestration code in
//! `workflow` and the queue worker calls these bridges instead of spawning
//! directly, so the N-1 core budget holds by construction.
//!
//! Budget accounting: the semaphore holds as many permits as the shared
//! Rayon pool has threads (`max(1, N-1)`). Every bulk CPU section holds one
//! permit across the result await, so background CPU parallelism can never
//! exceed the pool no matter how many queue workers queue CPU work at once.
//! The permit moves into the Rayon closure and drops after the result is
//! sent, which keeps it held for the whole await.
//!
//! Ordering: bulk lanes (scan, expire) use LIFO `rayon::spawn`; the
//! Interactive lane uses FIFO `spawn_fifo` so concurrent request-path
//! updates complete in arrival order. Neither bridge nests: bridged
//! closures never re-enter a bridge (the Rayon pool work-steals nested
//! `par_*` sections without extra permits).

use std::sync::{Arc, LazyLock};

use anyhow::Context;
use tokio::sync::Semaphore;

/// Rayon admittance: one permit per shared-pool compute thread.
static CPU_PERMITS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| {
    let threads = (*crate::tasks::runtime::CURRENT_NUM_THREADS)
        .saturating_sub(1)
        .max(1);
    Arc::new(Semaphore::new(threads))
});

/// Available CPU permits (diagnostic).
pub fn available_permits() -> usize {
    CPU_PERMITS.available_permits()
}

/// Run bulk-lane CPU work `f` on the shared Rayon pool (LIFO order).
pub async fn run_cpu<F, T>(f: F) -> anyhow::Result<T>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let permit = CPU_PERMITS
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| anyhow::anyhow!("cpu admittance semaphore closed"))?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    rayon::spawn(move || {
        let _permit = permit;
        let _ = tx.send(f());
    });
    rx.await
        .map_err(|_| anyhow::anyhow!("background cpu task dropped its result"))?
}

/// Run Interactive-lane CPU work `f` on the shared Rayon pool (FIFO order).
pub async fn run_cpu_fifo<F, T>(f: F) -> anyhow::Result<T>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let permit = CPU_PERMITS
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| anyhow::anyhow!("cpu admittance semaphore closed"))?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    rayon::spawn_fifo(move || {
        let _permit = permit;
        let _ = tx.send(f());
    });
    rx.await
        .map_err(|_| anyhow::anyhow!("background cpu task dropped its result"))?
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
