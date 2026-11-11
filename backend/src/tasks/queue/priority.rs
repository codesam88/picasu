//! Priority lanes for background intake items, strongest first:
//! `Interactive > Watcher > Scan > Expire`.
//!
//! Dequeue order is smooth weighted round-robin over the lanes that
//! currently hold items, plus aging: a lane that is ready but unserved for
//! [`MAX_SKIPS`] consecutive picks is served next regardless of deficit,
//! which bounds starvation when a high-weight lane floods.

/// Background-job priority lane carried on every intake item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Priority {
    /// Interactive request-path work (fastest lane).
    Interactive,
    /// Filesystem-watcher debounce work.
    Watcher,
    /// Walk-driven scan work.
    Scan,
    /// Expiry work.
    Expire,
}

impl Priority {
    /// Lane index into [`LANE_WEIGHTS`] and the scheduler state.
    pub fn lane_index(self) -> usize {
        self as usize
    }
}

/// Number of scheduler lanes (one per [`Priority`] variant).
pub const LANE_COUNT: usize = 4;

/// Weighted round-robin weights per lane, strongest lane first:
/// Interactive 8, Watcher 4, Scan 2, Expire 1.
pub const LANE_WEIGHTS: [i32; LANE_COUNT] = [8, 4, 2, 1];

/// Consecutive ready-but-unserved picks after which a lane is served
/// regardless of deficit (aging). Weighted round-robin alone cannot starve
/// a lane, so this only fires under pathological skew; it is a bound, not
/// the scheduling mechanism.
pub const MAX_SKIPS: u32 = 64;

/// Mutable scheduler state: smooth-WRR deficits plus per-lane skip counts.
#[derive(Debug, Clone, Copy)]
pub struct SchedulerState {
    deficit: [i32; LANE_COUNT],
    skips: [u32; LANE_COUNT],
}

impl Default for SchedulerState {
    fn default() -> Self {
        Self {
            deficit: [0; LANE_COUNT],
            skips: [0; LANE_COUNT],
        }
    }
}

/// Pick the next lane to serve among `ready` lanes (indexed by
/// [`Priority::lane_index`]).
///
/// Smooth weighted round-robin: every ready lane accrues its weight, the
/// deepest-deficit ready lane wins, and the winner pays down the weight
/// total. Ties break toward the stronger lane. Only ready lanes accrue, so
/// an idle lane gains no advantage while it has nothing queued. Aging runs
/// first: a ready lane skipped [`MAX_SKIPS`] times in a row wins outright.
pub fn pick_lane(ready: [bool; LANE_COUNT], state: &mut SchedulerState) -> Option<usize> {
    for (index, is_ready) in ready.iter().enumerate() {
        if *is_ready && state.skips[index] >= MAX_SKIPS {
            state.skips[index] = 0;
            return Some(index);
        }
    }

    let total: i32 = LANE_WEIGHTS.iter().sum();
    for (index, is_ready) in ready.iter().enumerate() {
        if *is_ready {
            state.deficit[index] += LANE_WEIGHTS[index];
        }
    }

    let mut best: Option<usize> = None;
    for (index, is_ready) in ready.iter().enumerate() {
        if *is_ready && best.is_none_or(|current| state.deficit[index] > state.deficit[current]) {
            best = Some(index);
        }
    }

    if let Some(winner) = best {
        state.deficit[winner] -= total;
        state.skips[winner] = 0;
        for (index, is_ready) in ready.iter().enumerate() {
            if *is_ready && index != winner {
                state.skips[index] = state.skips[index].saturating_add(1);
            }
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interactive() -> usize {
        Priority::Interactive.lane_index()
    }

    #[test]
    fn no_ready_lane_picks_nothing() {
        let mut state = SchedulerState::default();
        assert_eq!(pick_lane([false; LANE_COUNT], &mut state), None);
    }

    #[test]
    fn single_ready_lane_always_wins() {
        let mut state = SchedulerState::default();
        let mut ready = [false; LANE_COUNT];
        ready[Priority::Scan.lane_index()] = true;
        for _ in 0..32 {
            assert_eq!(
                pick_lane(ready, &mut state),
                Some(Priority::Scan.lane_index())
            );
        }
    }

    #[test]
    fn weights_hold_over_a_window() {
        // One full smooth-WRR cycle (sum of weights = 15 picks) serves each
        // lane exactly its weight; ten cycles must match exactly.
        let mut state = SchedulerState::default();
        let mut counts = [0u32; LANE_COUNT];
        for _ in 0..150 {
            let winner = pick_lane([true; LANE_COUNT], &mut state).expect("a lane is ready");
            counts[winner] += 1;
        }
        assert_eq!(
            counts,
            [80, 40, 20, 10],
            "served counts must match lane weights 8/4/2/1"
        );
    }

    #[test]
    fn idle_lane_gains_no_advantage() {
        // Scan alone for many rounds must not bank deficit: once Interactive
        // has work, it wins immediately as the stronger lane.
        let mut state = SchedulerState::default();
        let mut scan_only = [false; LANE_COUNT];
        scan_only[Priority::Scan.lane_index()] = true;
        for _ in 0..200 {
            pick_lane(scan_only, &mut state);
        }
        let mut both = [false; LANE_COUNT];
        both[interactive()] = true;
        both[Priority::Scan.lane_index()] = true;
        assert_eq!(pick_lane(both, &mut state), Some(interactive()));
    }

    #[test]
    fn aging_forces_a_starved_lane() {
        // A lane skipped MAX_SKIPS times wins outright even against a
        // stronger lane that smooth-WRR would otherwise prefer.
        let mut state = SchedulerState::default();
        state.skips[Priority::Scan.lane_index()] = MAX_SKIPS;
        let mut both = [false; LANE_COUNT];
        both[interactive()] = true;
        both[Priority::Scan.lane_index()] = true;
        assert_eq!(
            pick_lane(both, &mut state),
            Some(Priority::Scan.lane_index())
        );
        assert_eq!(state.skips[Priority::Scan.lane_index()], 0);
    }
}
