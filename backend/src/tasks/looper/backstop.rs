use crate::model::config::AppConfig;
use crate::tasks::actor::album_index::start_index_job;
use crate::tasks::runtime::spawn;

use log::info;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

/// Scheduler state for the backstop loop.
///
/// Encodes the non-overlap + coalescing contract (REQ-14): a cycle that is
/// running is never started again; a tick that arrives while a cycle runs
/// coalesces into a single pending flag (repeated ticks do not accumulate);
/// starting a cycle consumes the pending flag, and `finished()` reports
/// whether a follow-up is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BackstopState {
    running: bool,
    pending: bool,
}

impl BackstopState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that a tick arrived while a cycle was running.
    pub fn note_pending(&mut self) {
        self.pending = true;
    }

    /// Mark a cycle as started; any pending follow-up is consumed by it.
    pub fn started(&mut self) {
        self.running = true;
        self.pending = false;
    }

    /// Mark a cycle as finished. Returns `true` when a coalesced follow-up
    /// should start.
    pub fn finished(&mut self) -> bool {
        self.running = false;
        let follow = self.pending;
        self.pending = false;
        follow
    }

    #[cfg(test)]
    pub fn is_running(self) -> bool {
        self.running
    }

    #[cfg(test)]
    pub fn has_pending(self) -> bool {
        self.pending
    }
}

/// Start the periodic full-scan backstop loop.
///
/// Returns `None` when the configured interval is `0` (disabled). Otherwise
/// spawns a task that owns each backstop cycle: it starts a scan job and
/// awaits that job's completion instead of polling global status. Ticks that
/// arrive while a cycle runs coalesce into one follow-up cycle (REQ-14).
/// The returned handle is aborted during shutdown.
pub(crate) fn start_backstop_loop() -> Option<JoinHandle<()>> {
    let interval_secs =
        crate::model::config::with_config(|c: &AppConfig| c.index_backstop_interval_secs);
    if interval_secs == 0 {
        info!("index backstop disabled (interval 0)");
        return None;
    }

    let interval = Duration::from_secs(interval_secs);
    info!("starting index backstop loop (interval {interval_secs}s)");

    Some(spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        // Consume the interval's immediate first tick so the first scan
        // happens after one full interval.
        ticker.tick().await;

        let mut state = BackstopState::new();
        loop {
            ticker.tick().await;

            loop {
                state.started();
                let Ok(job) = start_index_job("/", false, None) else {
                    // Another trigger owns the cycle; retry on the next tick.
                    state.finished();
                    break;
                };

                let wait = job.wait();
                tokio::pin!(wait);
                loop {
                    tokio::select! {
                        () = &mut wait => break,
                        _ = ticker.tick() => state.note_pending(),
                    }
                }

                if !state.finished() {
                    break;
                }
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::BackstopState;

    #[test]
    fn started_consumes_pending() {
        let mut s = BackstopState::new();
        s.note_pending();
        s.started();
        assert!(!s.has_pending(), "starting a cycle consumes the follow-up");
        assert!(s.is_running());
    }

    #[test]
    fn repeated_running_ticks_coalesce() {
        let mut s = BackstopState::new();
        s.started();
        s.note_pending();
        s.note_pending();
        s.note_pending();
        assert!(s.has_pending(), "ticks during a cycle set one pending flag");
        assert!(s.finished(), "finish should report exactly one follow-up");
        assert!(!s.has_pending(), "the follow-up flag is consumed");
        assert!(!s.is_running());
    }

    #[test]
    fn finish_without_pending_does_not_start_follow_up() {
        let mut s = BackstopState::new();
        s.started();
        assert!(!s.finished());
        assert!(!s.has_pending());
    }

    #[test]
    fn tick_while_idle_is_the_start_path() {
        // The loop starts a cycle on an idle tick; the state itself only
        // carries the pending flag and running marker.
        let mut s = BackstopState::new();
        assert!(!s.is_running());
        s.started();
        assert!(s.is_running());
        assert!(!s.finished());
    }
}
