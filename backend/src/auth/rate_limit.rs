//! Brute-force throttling oracle + buckets (R7).
//!
//! Pure delay functions and clock-free bucket bookkeeping so the
//! schedules in `.plan/r7-bruteforce-throttling.md` are pinned by unit
//! tests. Callers pass wall-clock seconds; tests drive a fake counter.

/// IP-short window (seconds): 10-minute burst window.
pub const LOGIN_IP_WINDOW_SECS: u64 = 600;
/// IP-short bucket: trips after this many failures in-window (4th bans).
pub const LOGIN_IP_MAX_FAILURES: u32 = 3;
/// IP-short base (seconds); doubles per generation.
pub const LOGIN_IP_BASE_BAN_SECS: u64 = 10;
/// IP-short ban cap (seconds, 8h).
pub const LOGIN_IP_MAX_BAN_SECS: u64 = 28_800;
/// IP-long window (seconds, 24h): per-source daily ceiling catching paced
/// spraying across many usernames.
pub const LOGIN_IP_LONG_WINDOW_SECS: u64 = 86_400;
/// IP-long bucket: trips after this many failures in-window (17th bans).
pub const LOGIN_IP_LONG_MAX_FAILURES: u32 = 16;
/// IP-long base (seconds); doubles per generation.
pub const LOGIN_IP_LONG_BASE_BAN_SECS: u64 = 1800;
/// IP-long ban cap (seconds, 8h).
pub const LOGIN_IP_LONG_MAX_BAN_SECS: u64 = 28_800;
/// Long user-drip window (seconds, 24h): per-account daily ceiling
/// catching low-and-slow attacks that stay under the burst buckets.
pub const LOGIN_USER_LONG_WINDOW_SECS: u64 = 86_400;
/// Long user bucket: trips after this many failures in-window (19th bans).
pub const LOGIN_USER_LONG_MAX_FAILURES: u32 = 18;
/// First long-ban duration (seconds); doubles per generation.
pub const LOGIN_USER_LONG_BASE_BAN_SECS: u64 = 1800;
/// Long-ban cap (seconds, 2h): a real owner can always retry within hours.
pub const LOGIN_USER_LONG_MAX_BAN_SECS: u64 = 7200;

/// Exponential backoff delay: `min(base * 2^n, cap)` with saturation.
///
/// `n` is the ban generation (0 = first ban). Throttled attempts do not
/// advance `n`; only an evaluated failure does. Success resets.
#[must_use]
pub fn ban_delay(base_secs: u64, cap_secs: u64, generation: u32) -> u64 {
    let mut delay = base_secs;
    for _ in 0..generation {
        delay = delay.saturating_mul(2);
        if delay >= cap_secs {
            return cap_secs;
        }
    }
    delay.min(cap_secs)
}

/// One sliding-window throttle bucket: two evaluated over-threshold
/// failures share each ban generation (both ban).
///
/// Times are seconds since an arbitrary epoch (callers pass wall-clock
/// seconds); the bucket itself never reads the clock, so tests drive it
/// with a fake counter.
#[derive(Debug, Default)]
pub struct Bucket {
    window_secs: u64,
    max_failures: u32,
    base_ban_secs: u64,
    max_ban_secs: u64,
    failures: std::collections::VecDeque<u64>,
    ban_until: u64,
}

impl Bucket {
    /// Create a bucket: `max_failures` failures within `window_secs`
    /// open the first ban of `base_ban_secs`, doubling per two further
    /// failures up to `max_ban_secs`.
    ///
    /// # Errors
    /// Returns an error when the config is degenerate: zero window (nothing
    /// could ever count), zero `max_failures` (the very first failure would
    /// ban), zero base (bans would be no-ops), or `max_ban_secs` below
    /// `base_ban_secs` (the cap would invert the schedule). Callers
    /// surfacing operator config must reject these instead of clamping —
    /// clamping would silently run a policy nobody asked for.
    pub fn new(
        window_secs: u64,
        max_failures: u32,
        base_ban_secs: u64,
        max_ban_secs: u64,
    ) -> Result<Self, &'static str> {
        if window_secs == 0 {
            return Err("throttle window must be at least 1 second");
        }
        if max_failures == 0 {
            return Err("throttle must allow at least 1 failure before banning");
        }
        if base_ban_secs == 0 {
            return Err("throttle base ban must be at least 1 second");
        }
        if max_ban_secs < base_ban_secs {
            return Err("throttle max ban must be at least the base ban");
        }
        Ok(Self {
            window_secs,
            max_failures,
            base_ban_secs,
            max_ban_secs,
            failures: std::collections::VecDeque::new(),
            ban_until: 0,
        })
    }

    /// Remaining ban seconds at `now` (0 = not banned).
    #[must_use]
    pub fn ban_remaining(&self, now: u64) -> u64 {
        self.ban_until.saturating_sub(now)
    }

    /// Record an evaluated failure at `now`. Returns the ban applied
    /// (0 = still under threshold). Calls during an active ban are
    /// ignored — they return the remaining ban without recording, so
    /// hammering neither extends nor escalates.
    pub fn record_failure(&mut self, now: u64) -> u64 {
        if now < self.ban_until {
            return self.ban_until - now;
        }
        while self
            .failures
            .front()
            .is_some_and(|old| now.saturating_sub(*old) >= self.window_secs)
        {
            self.failures.pop_front();
        }
        self.failures.push_back(now);
        let count = self.failures.len() as u64;
        let max = u64::from(self.max_failures);
        if count <= max {
            return 0;
        }
        let generation = (count - max - 1) / 2;
        let generation = u32::try_from(generation).unwrap_or(u32::MAX);
        let delay = ban_delay(self.base_ban_secs, self.max_ban_secs, generation);
        self.ban_until = now.saturating_add(delay);
        delay
    }

    /// Clear all state (on successful authentication).
    pub fn record_success(&mut self) {
        self.failures.clear();
        self.ban_until = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LOGIN_IP_BASE_BAN_SECS, LOGIN_IP_LONG_BASE_BAN_SECS, LOGIN_IP_LONG_MAX_BAN_SECS,
        LOGIN_IP_LONG_MAX_FAILURES, LOGIN_IP_LONG_WINDOW_SECS, LOGIN_IP_MAX_BAN_SECS,
        LOGIN_IP_MAX_FAILURES, LOGIN_IP_WINDOW_SECS, LOGIN_USER_LONG_BASE_BAN_SECS,
        LOGIN_USER_LONG_MAX_BAN_SECS, LOGIN_USER_LONG_MAX_FAILURES, LOGIN_USER_LONG_WINDOW_SECS,
        ban_delay,
    };

    /// Expected vectors below are hand-computed literals (`base * 2^n`
    /// capped), deliberately not derived from the implementation — a bug
    /// shared by both sides would otherwise stay invisible. The default
    /// pins at the end tie these literals to the shipped constants.
    #[test]
    fn ban_delay_doubles_then_caps() {
        // (base, cap, generation, expected)
        let cases: [(u64, u64, u32, u64); 17] = [
            (10, 28_800, 0, 10),
            (10, 28_800, 1, 20),
            (10, 28_800, 2, 40),
            (10, 28_800, 11, 20_480),
            (10, 28_800, 12, 28_800),
            (10, 28_800, 30, 28_800),
            (1800, 28_800, 0, 1800),
            (1800, 28_800, 3, 14_400),
            (1800, 28_800, 4, 28_800),
            (1800, 7200, 0, 1800),
            (1800, 7200, 1, 3600),
            (1800, 7200, 2, 7200),
            (1800, 7200, 30, 7200),
            // Degenerate-but-valid edges.
            (5, 5, 0, 5),
            (5, 5, 9, 5),
            (1, 1, 100, 1),
            (7, 100, 0, 7),
        ];
        for (base, cap, n, expected) in cases {
            assert_eq!(
                ban_delay(base, cap, n),
                expected,
                "ban_delay({base}, {cap}, {n})"
            );
        }
    }

    #[test]
    fn ban_delay_saturates_without_overflow() {
        // 1 * 2^100 overflows u64: must saturate at the cap, not wrap,
        // and must return quickly (no per-generation cost blowup).
        assert_eq!(ban_delay(1, u64::MAX, 100), u64::MAX);
        assert_eq!(ban_delay(1, u64::MAX, u32::MAX), u64::MAX);
        assert_eq!(ban_delay(u64::MAX, u64::MAX, 3), u64::MAX);
        assert_eq!(ban_delay(28_800, 28_800, u32::MAX), 28_800);
    }

    #[test]
    fn shipped_defaults_match_the_pinned_schedules() {
        // IP-short: 3/10min → 10, 20, … capped 8h.
        assert_eq!(LOGIN_IP_WINDOW_SECS, 600);
        assert_eq!(LOGIN_IP_MAX_FAILURES, 3);
        assert_eq!(LOGIN_IP_BASE_BAN_SECS, 10);
        assert_eq!(LOGIN_IP_MAX_BAN_SECS, 28_800);
        // IP-long: 16/24h → 1800, 3600, … capped 8h.
        assert_eq!(LOGIN_IP_LONG_WINDOW_SECS, 86_400);
        assert_eq!(LOGIN_IP_LONG_MAX_FAILURES, 16);
        assert_eq!(LOGIN_IP_LONG_BASE_BAN_SECS, 1800);
        assert_eq!(LOGIN_IP_LONG_MAX_BAN_SECS, 28_800);
        // User-long: 18/24h → 1800, 3600, capped 2h.
        assert_eq!(LOGIN_USER_LONG_WINDOW_SECS, 86_400);
        assert_eq!(LOGIN_USER_LONG_MAX_FAILURES, 18);
        assert_eq!(LOGIN_USER_LONG_BASE_BAN_SECS, 1800);
        assert_eq!(LOGIN_USER_LONG_MAX_BAN_SECS, 7200);
    }

    #[test]
    fn bucket_rejects_degenerate_config() {
        assert!(Bucket::new(0, 3, 10, 28_800).is_err(), "zero window");
        assert!(Bucket::new(600, 0, 10, 28_800).is_err(), "zero max");
        assert!(Bucket::new(600, 3, 0, 28_800).is_err(), "zero base");
        assert!(Bucket::new(600, 3, 10, 0).is_err(), "zero cap");
        assert!(Bucket::new(600, 3, 30, 10).is_err(), "cap below base");
        assert!(Bucket::new(600, 3, 10, 28_800).is_ok(), "sane config");
        assert!(Bucket::new(1, 1, 1, 1).is_ok(), "minimal config");
    }

    use super::Bucket;

    fn ip_short_bucket() -> Bucket {
        Bucket::new(600, 3, 10, 28_800).expect("valid test config")
    }

    #[test]
    fn bucket_under_threshold_never_bans() {
        let mut b = ip_short_bucket();
        assert_eq!(b.record_failure(0), 0);
        assert_eq!(b.record_failure(1), 0);
        assert_eq!(b.record_failure(2), 0);
        assert_eq!(b.ban_remaining(3), 0);
    }

    #[test]
    fn bucket_two_failures_share_each_level() {
        let mut b = ip_short_bucket();
        assert_eq!(b.record_failure(0), 0);
        assert_eq!(b.record_failure(1), 0);
        assert_eq!(b.record_failure(2), 0);
        // Over-threshold #1-2 share the first rung (10s).
        assert_eq!(b.record_failure(3), 10);
        assert_eq!(b.ban_remaining(4), 9);
        // Attempts during the ban are ignored, never recorded.
        assert_eq!(b.record_failure(10), 3);
        assert_eq!(b.record_failure(14), 10);
        // Over-threshold #3-4 share the next level (20s).
        assert_eq!(b.record_failure(25), 20);
        assert_eq!(b.record_failure(46), 20);
        // Over-threshold #5-6 step up again (40s).
        assert_eq!(b.record_failure(67), 40);
    }

    #[test]
    fn bucket_single_failure_threshold_bans_immediately() {
        let mut b = Bucket::new(60, 1, 5, 50).expect("valid test config");
        assert_eq!(b.record_failure(0), 0);
        assert_eq!(b.record_failure(1), 5);
        assert_eq!(b.record_failure(2), 4);
    }

    #[test]
    fn ip_long_bucket_trips_on_17th_failure_in_24h() {
        let mut b = Bucket::new(86_400, 16, 1800, 28_800).expect("valid test config");
        for t in 0..16 {
            assert_eq!(b.record_failure(t), 0, "failure {t} must be free");
        }
        assert_eq!(b.record_failure(16), 1800);
        // Second failure on the same level shares it (post-expiry).
        assert_eq!(b.record_failure(2000), 1800);
        // Third steps up a level.
        assert_eq!(b.record_failure(4000), 3600);
    }

    #[test]
    fn user_long_bucket_trips_on_19th_failure_in_24h() {
        let mut b = Bucket::new(86_400, 18, 1800, 7200).expect("valid test config");
        for t in 0..18 {
            assert_eq!(b.record_failure(t), 0, "failure {t} must be free");
        }
        assert_eq!(b.record_failure(18), 1800);
    }

    #[test]
    fn bucket_window_expiry_forgives() {
        let mut b = ip_short_bucket();
        b.record_failure(0);
        b.record_failure(1);
        b.record_failure(2);
        // All three age out of the 600s window: fresh start, no ban.
        assert_eq!(b.record_failure(700), 0);
        assert_eq!(b.ban_remaining(701), 0);
    }

    #[test]
    fn bucket_window_boundary_is_exclusive() {
        let mut b = Bucket::new(10, 2, 5, 50).expect("valid test config");
        b.record_failure(0);
        b.record_failure(1);
        // At t=10 the t=0 failure is exactly `window` old → expired,
        // so failures t=1 and t=10 count: still under threshold.
        assert_eq!(b.record_failure(10), 0);
        assert_eq!(b.record_failure(11), 0);
        // t=10, t=11, t=12 are all in window: third in-window failure bans.
        assert_eq!(b.record_failure(12), 5);
    }

    #[test]
    fn bucket_success_clears() {
        let mut b = ip_short_bucket();
        b.record_failure(0);
        b.record_failure(1);
        b.record_failure(2);
        assert_eq!(b.record_failure(3), 10);
        b.record_success();
        assert_eq!(b.ban_remaining(4), 0);
        assert_eq!(b.record_failure(5), 0);
    }

    #[test]
    fn bucket_success_on_empty_is_noop() {
        let mut b = ip_short_bucket();
        b.record_success();
        assert_eq!(b.ban_remaining(0), 0);
        assert_eq!(b.record_failure(0), 0);
    }

    #[test]
    fn bucket_ban_end_is_saturating_at_time_extremes() {
        let mut b = Bucket::new(86_400, 1, u64::MAX, u64::MAX).expect("valid test config");
        b.record_failure(0);
        // ban_until saturates instead of wrapping: the ban runs to u64::MAX.
        assert_eq!(b.record_failure(1), u64::MAX);
        assert_eq!(b.ban_remaining(u64::MAX - 1), 1);
        assert_eq!(b.ban_remaining(u64::MAX), 0);
    }
}
