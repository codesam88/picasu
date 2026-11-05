use std::sync::LazyLock;

use rayon::ThreadPoolBuilder;
use tokio::runtime::{Builder, Runtime};

/// Hardware parallelism (`N`) for the process. The single Tokio runtime runs
/// `N` workers; the global rayon pool is the bulk pool with `N` compute
/// threads.
///
/// The bulk pool runs deprioritized (higher nice value): interactive work
/// lives on its own pool (see [`INTERACTIVE_POOL`]), and the OS scheduler
/// preempts bulk threads for it. This replaces static thread reservation:
/// instead of fencing off pool threads, bulk simply yields the CPU whenever
/// interactive work is runnable, and gets everything back when it is not.
/// See `lower_this_thread_priority` for the mechanism.
///
/// Forcing this value builds the global rayon pool first: `build_global` must
/// run before anything else observes rayon, otherwise rayon lazily creates
/// its implicit default-sized pool and the budget is lost. The binary
/// forces this static first thing in `run()`.
pub static CURRENT_NUM_THREADS: LazyLock<usize> = LazyLock::new(|| {
    let n = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    // `Err` means the implicit pool already exists: a dependency touched
    // rayon before startup, which happens when tests reach request-path
    // rayon before the first coordinator use. Keep that pool rather than
    // panic; `run()` forces this static first, so the served process always
    // gets the sized pool.
    let _ = ThreadPoolBuilder::new()
        .num_threads(pool_thread_count(n))
        .thread_name(|i| format!("picasu-bulk-{i}"))
        .start_handler(|_| lower_this_thread_priority(BACKGROUND_NICE))
        .build_global();
    n
});

/// Rayon pool width for N hardware threads: the full budget.
/// Pure for testability; the static above applies it.
pub(crate) fn pool_thread_count(n: usize) -> usize {
    n.max(1)
}

/// Bulk-work admittance: how many bulk jobs may hold the pool at once.
/// Kept at `max(1, N-1)` as a queue-depth bound (each admitted job pins
/// bulk intermediate state); interactive latency no longer depends on it,
/// that is the interactive pool's job.
pub(crate) fn admittance_permits(n: usize) -> usize {
    n.saturating_sub(1).max(1)
}

/// Interactive pool: latency-sensitive request-path work only, at default
/// OS priority, so the scheduler preempts deprioritized bulk threads for
/// it. Sized at half the cores (min 2): enough for concurrent prefetch
/// joins with headroom, small enough that idle threads cost nothing.
/// Explicitly built (never global) so third-party rayon use stays on the
/// bulk pool and request paths opt in via `pool.install(...)`.
pub static INTERACTIVE_POOL: LazyLock<rayon::ThreadPool> = LazyLock::new(|| {
    ThreadPoolBuilder::new()
        .num_threads(interactive_pool_threads(*CURRENT_NUM_THREADS))
        .thread_name(|i| format!("picasu-interactive-{i}"))
        .build()
        .expect("Failed to build interactive Rayon pool")
});

/// Interactive pool width for N hardware threads: half, floored at two so
/// small machines still get join parallelism. Pure for testability.
pub(crate) fn interactive_pool_threads(n: usize) -> usize {
    (n / 2).max(2)
}

/// Deprioritize bulk compute threads relative to interactive work.
/// Higher nice value, set per-thread: the OS scheduler then preempts bulk
/// threads whenever interactive work is runnable, and hands the CPUs back
/// when it is not. Unprivileged-safe (only raising nice, never lowering),
/// best-effort: under seccomp or on non-Linux targets this is a no-op and
/// the pools still isolate by thread count.
const BACKGROUND_NICE: libc::c_int = 10;

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn lower_this_thread_priority(nice: libc::c_int) {
    // SAFETY: gettid always succeeds for the calling thread; setpriority on
    // the caller's own TID only ever raises niceness here (never lowers it),
    // so the call cannot fail with EACCES for privilege reasons. A failed
    // conversion or call (seccomp, containers) silently drops the hint.
    let tid = unsafe { libc::syscall(libc::SYS_gettid) };
    let Ok(tid) = libc::id_t::try_from(tid) else {
        return;
    };
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, tid, nice);
    }
}

#[cfg(not(target_os = "linux"))]
fn lower_this_thread_priority(_nice: libc::c_int) {}

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

#[cfg(test)]
mod tests {
    use super::{admittance_permits, interactive_pool_threads, pool_thread_count};

    #[test]
    fn pool_sizing_bounds_bulk_and_sizes_interactive() {
        // Single core cannot spare anything: everything clamps to 1..=2.
        assert_eq!(pool_thread_count(1), 1);
        assert_eq!(admittance_permits(1), 1);
        assert_eq!(interactive_pool_threads(1), 2);
        for n in 2..=257usize {
            // Bulk pool takes the full budget; admittance stays one below
            // it as a queue-depth bound, not a reservation.
            assert_eq!(pool_thread_count(n), n);
            assert_eq!(admittance_permits(n), n - 1);
            // Interactive pool is half the cores, floored for join parallelism.
            assert_eq!(interactive_pool_threads(n), (n / 2).max(2));
        }
    }
}
