use std::sync::LazyLock;

use rayon::ThreadPoolBuilder;
use tokio::runtime::{Builder, Runtime};

/// Hardware parallelism (`N`) for the process. The single Tokio runtime runs
/// `N` workers; the global rayon pool runs `max(1, N - 1)` compute threads.
///
/// Forcing this value builds the global rayon pool first: `build_global` must
/// run before anything else observes rayon, otherwise rayon lazily creates
/// its implicit default-sized pool and the `N - 1` budget is lost. The binary
/// forces this static first thing in `run()`.
pub static CURRENT_NUM_THREADS: LazyLock<usize> = LazyLock::new(|| {
    let n = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    // `Err` means the implicit pool already exists: a dependency touched
    // rayon before startup, which happens when tests reach request-path
    // rayon before the first coordinator use. Keep that pool rather than
    // panic; `run()` forces this static first, so the served process always
    // gets the sized pool.
    let _ = ThreadPoolBuilder::new()
        .num_threads(n.saturating_sub(1).max(1))
        .build_global();
    n
});

/// The single Tokio runtime: Rocket request handling and background
/// orchestration share its `N` workers.
pub static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    Builder::new_multi_thread()
        .worker_threads(*CURRENT_NUM_THREADS)
        .thread_name("tokio-core")
        .enable_all()
        .build()
        .expect("Failed to build Tokio runtime")
});

/// Spawn `task` onto [`RUNTIME`].
pub fn spawn<F>(task: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    RUNTIME.spawn(task)
}
