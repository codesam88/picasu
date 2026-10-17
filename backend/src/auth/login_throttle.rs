//! Per-IP / per-user login throttle tracker.
//!
//! Wraps the three [`Bucket`] schedules from [`super::rate_limit`] —
//! IP-short burst, IP-long daily, user-long daily — behind one small API
//! the authenticate handler can drive with explicit timestamps. The
//! tracker never reads the clock: every method takes `now` in seconds,
//! so unit tests run on a fake counter.

use std::collections::HashMap;

use super::rate_limit::Bucket;

/// Per-client-IP entry cap, enforced by [`LoginThrottle::sweep`] (and
/// defensively whenever [`LoginThrottle::record_failure`] creates an
/// entry, so memory stays bounded between sweeps).
pub const MAX_IP_ENTRIES: usize = 10_000;

/// Per-username entry cap; same enforcement as [`MAX_IP_ENTRIES`].
pub const MAX_USER_ENTRIES: usize = 10_000;

/// Raw bucket parameters, kept so a fresh [`Bucket`] can be built per
/// entry. Validated once by [`LoginThrottle::new`] before being stored.
#[derive(Clone, Copy, Debug)]
struct BucketCfg {
    window_secs: u64,
    max_failures: u32,
    base_ban_secs: u64,
    max_ban_secs: u64,
}

impl BucketCfg {
    fn from_tuple(t: (u64, u32, u64, u64)) -> Self {
        Self {
            window_secs: t.0,
            max_failures: t.1,
            base_ban_secs: t.2,
            max_ban_secs: t.3,
        }
    }

    /// Build a fresh bucket from these params.
    ///
    /// # Panics
    /// Never: [`LoginThrottle::new`] validated every tuple up front.
    fn build(&self) -> Bucket {
        Bucket::new(
            self.window_secs,
            self.max_failures,
            self.base_ban_secs,
            self.max_ban_secs,
        )
        .expect("bucket params validated by LoginThrottle::new")
    }
}

/// One client-IP entry: the short and long buckets plus sweep
/// bookkeeping (`last_seen` and a `banned_until` snapshot, see
/// [`LoginThrottle::sweep`]).
#[derive(Debug)]
struct IpEntry {
    short: Bucket,
    long: Bucket,
    last_seen: u64,
    banned_until: u64,
}

/// One claimed-username entry: the long bucket plus sweep bookkeeping.
#[derive(Debug)]
struct UserEntry {
    long: Bucket,
    last_seen: u64,
    banned_until: u64,
}

/// Brute-force login tracker keyed by client IP and claimed username.
///
/// Uses a plain [`HashMap`] rather than a sharded concurrent map: Picasu
/// runs a single Rocket instance and handlers only hold the lock across
/// these short, in-memory lookups, so a `DashMap`-style table would add
/// complexity without a contention win.
///
/// Entries are created (or refreshed) by
/// [`record_failure`](Self::record_failure), dropped by
/// [`record_success`](Self::record_success), and reaped by
/// [`sweep`](Self::sweep) — see there for the eviction approximation and
/// the entry caps.
#[derive(Debug)]
pub struct LoginThrottle {
    /// Per client IP: short + long buckets.
    ip: HashMap<String, IpEntry>,
    /// Per claimed username: long bucket.
    user: HashMap<String, UserEntry>,
    ip_short: BucketCfg,
    ip_long: BucketCfg,
    user_long: BucketCfg,
    /// Largest window across the three schedules: an entry with no ban
    /// and no failure older than this cannot influence any bucket.
    longest_window: u64,
}

impl LoginThrottle {
    /// Build a tracker from three `(window_secs, max_failures,
    /// base_ban_secs, max_ban_secs)` tuples — IP-short, IP-long,
    /// user-long — in that order.
    ///
    /// # Errors
    /// Propagates [`Bucket::new`]'s validation of each tuple (zero
    /// window/max/base, or cap below base), so operator config is
    /// rejected at startup rather than clamped silently.
    pub fn new(
        ip_short: (u64, u32, u64, u64),
        ip_long: (u64, u32, u64, u64),
        user_long: (u64, u32, u64, u64),
    ) -> Result<Self, &'static str> {
        // Validate all three tuples before storing anything, so `build`
        // below can rely on its params without re-checking.
        Bucket::new(ip_short.0, ip_short.1, ip_short.2, ip_short.3)?;
        Bucket::new(ip_long.0, ip_long.1, ip_long.2, ip_long.3)?;
        Bucket::new(user_long.0, user_long.1, user_long.2, user_long.3)?;
        let longest_window = ip_short.0.max(ip_long.0).max(user_long.0);
        Ok(Self {
            ip: HashMap::new(),
            user: HashMap::new(),
            ip_short: BucketCfg::from_tuple(ip_short),
            ip_long: BucketCfg::from_tuple(ip_long),
            user_long: BucketCfg::from_tuple(user_long),
            longest_window,
        })
    }

    /// Remaining ban seconds for this attempt across all three
    /// applicable buckets (0 = clear). Pure read: no entries are
    /// created and nothing is recorded.
    #[must_use]
    pub fn check(&self, ip: &str, user: &str, now: u64) -> u64 {
        let mut max_ban = 0;
        if let Some(entry) = self.ip.get(ip) {
            max_ban = max_ban
                .max(entry.short.ban_remaining(now))
                .max(entry.long.ban_remaining(now));
        }
        if let Some(entry) = self.user.get(user) {
            max_ban = max_ban.max(entry.long.ban_remaining(now));
        }
        max_ban
    }

    /// Record an evaluated login failure at `now` into the IP-short,
    /// IP-long and user-long buckets (creating entries as needed and
    /// refreshing `last_seen`); returns the largest ban applied
    /// (0 = no bucket banned).
    ///
    /// The handler checks [`check`](Self::check) first, so this should
    /// not run while banned; if it does, `Bucket` ignores mid-ban
    /// attempts and the remaining ban is returned unchanged.
    pub fn record_failure(&mut self, ip: &str, user: &str, now: u64) -> u64 {
        let (short_cfg, long_cfg) = (&self.ip_short, &self.ip_long);
        let entry = self.ip.entry(ip.to_owned()).or_insert_with(|| IpEntry {
            short: short_cfg.build(),
            long: long_cfg.build(),
            last_seen: 0,
            banned_until: 0,
        });
        entry.last_seen = now;
        let short_ban = entry.short.record_failure(now);
        let long_ban = entry.long.record_failure(now);
        entry.banned_until = entry
            .banned_until
            .max(now.saturating_add(short_ban))
            .max(now.saturating_add(long_ban));
        let mut max_ban = short_ban.max(long_ban);

        let user_cfg = &self.user_long;
        let entry = self
            .user
            .entry(user.to_owned())
            .or_insert_with(|| UserEntry {
                long: user_cfg.build(),
                last_seen: 0,
                banned_until: 0,
            });
        entry.last_seen = now;
        let user_ban = entry.long.record_failure(now);
        entry.banned_until = entry.banned_until.max(now.saturating_add(user_ban));
        max_ban = max_ban.max(user_ban);

        self.enforce_caps();
        max_ban
    }

    /// Successful authentication: drop the IP and username entries
    /// entirely, clearing any ban and failure history for both keys.
    pub fn record_success(&mut self, ip: &str, user: &str) {
        self.ip.remove(ip);
        self.user.remove(user);
    }

    /// Reap idle entries, then enforce [`MAX_IP_ENTRIES`] /
    /// [`MAX_USER_ENTRIES`].
    ///
    /// `Bucket` exposes no idleness probe, so each entry keeps a
    /// `banned_until` snapshot updated on every `record_failure`: the
    /// snapshot is always at least the real ban end (buckets only move
    /// their ban forward inside `record_failure`, which stores
    /// `now + returned_ban`), so an active ban can never be swept away.
    /// The approximation errs toward over-retention: if a later, shorter
    /// ban follows a longer one, the entry is kept until the older
    /// snapshot passes — an entry is evicted only when `now` is past
    /// both its ban snapshot and `last_seen + longest_window`, so an
    /// entry that could still influence any bucket survives.
    ///
    /// Over cap, the least-recently-`last_seen` entries go first.
    pub fn sweep(&mut self, now: u64) {
        let longest_window = self.longest_window;
        self.ip.retain(|_, entry| {
            now < entry.banned_until || now.saturating_sub(entry.last_seen) < longest_window
        });
        self.user.retain(|_, entry| {
            now < entry.banned_until || now.saturating_sub(entry.last_seen) < longest_window
        });
        self.enforce_caps();
    }

    /// Evict least-recently-seen entries until each map is within its cap.
    fn enforce_caps(&mut self) {
        while self.ip.len() > MAX_IP_ENTRIES {
            let Some(oldest) = self
                .ip
                .iter()
                .min_by_key(|(_, entry)| entry.last_seen)
                .map(|(ip, _)| ip.clone())
            else {
                break;
            };
            self.ip.remove(&oldest);
        }
        while self.user.len() > MAX_USER_ENTRIES {
            let Some(oldest) = self
                .user
                .iter()
                .min_by_key(|(_, entry)| entry.last_seen)
                .map(|(user, _)| user.clone())
            else {
                break;
            };
            self.user.remove(&oldest);
        }
    }
}

/// Pick the client-IP string used to key [`LoginThrottle`] entries.
///
/// Without a trusted proxy the forwarded header is ignored entirely.
/// With `trust_proxy`, only the leftmost (original-client) comma
/// separated entry of `forwarded_for` is considered, and only if it
/// trims to a non-empty string that parses as [`std::net::IpAddr`];
/// anything else falls back to the socket address — which comes from the
/// transport layer and is used verbatim — or `"unknown"` when absent or
/// empty. No Rocket types: pure and fully unit-testable.
#[must_use]
pub fn select_client_ip(
    forwarded_for: Option<&str>,
    socket_ip: Option<&str>,
    trust_proxy: bool,
) -> String {
    if trust_proxy
        && let Some(leftmost) = forwarded_for.map(|xff| xff.split(',').next().unwrap_or_default())
    {
        let candidate = leftmost.trim();
        if candidate.parse::<std::net::IpAddr>().is_ok() {
            return candidate.to_owned();
        }
    }
    match socket_ip {
        Some(socket) if !socket.is_empty() => socket.to_owned(),
        _ => "unknown".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::rate_limit::Bucket;

    /// Literal test config (deliberately not the shipped constants):
    /// IP-short 2 failures/10s → 5s, IP-long 4/100s → 7s,
    /// user-long 3/100s → 3s. Longest window = 100s.
    fn tracker() -> LoginThrottle {
        LoginThrottle::new((10, 2, 5, 20), (100, 4, 7, 40), (100, 3, 3, 30))
            .expect("valid test config")
    }

    fn bare_ip_entry(last_seen: u64) -> IpEntry {
        IpEntry {
            short: Bucket::new(10, 2, 5, 20).expect("valid test bucket"),
            long: Bucket::new(100, 4, 7, 40).expect("valid test bucket"),
            last_seen,
            banned_until: 0,
        }
    }

    fn bare_user_entry(last_seen: u64) -> UserEntry {
        UserEntry {
            long: Bucket::new(100, 3, 3, 30).expect("valid test bucket"),
            last_seen,
            banned_until: 0,
        }
    }

    #[test]
    fn check_is_clear_for_unknown_keys() {
        let t = tracker();
        assert_eq!(t.check("1.2.3.4", "alice", 0), 0);
    }

    #[test]
    fn ip_short_bucket_trips_on_third_failure() {
        let mut t = tracker();
        assert_eq!(t.record_failure("1.1.1.1", "alice", 0), 0);
        assert_eq!(t.record_failure("1.1.1.1", "bob", 1), 0);
        assert_eq!(t.check("1.1.1.1", "alice", 2), 0);
        // Third IP failure is over the short threshold → 5s ban.
        assert_eq!(t.record_failure("1.1.1.1", "carol", 2), 5);
        assert_eq!(t.check("1.1.1.1", "carol", 2), 5);
        assert_eq!(t.check("1.1.1.1", "alice", 6), 1, "ban runs to t=7");
        assert_eq!(t.check("1.1.1.1", "alice", 7), 0, "ban expired");
    }

    #[test]
    fn ip_long_bucket_trips_on_fifth_failure_from_distinct_users() {
        // IP-short and user-long are made lenient here to isolate IP-long.
        let mut t = LoginThrottle::new((1000, 100, 5, 20), (10, 4, 7, 40), (1000, 100, 3, 30))
            .expect("valid test config");
        for i in 0..4u64 {
            let user = format!("u{i}");
            assert_eq!(t.record_failure("2.2.2.2", &user, i), 0);
        }
        // Fifth failure is over the long threshold → 7s ban.
        assert_eq!(t.record_failure("2.2.2.2", "u4", 4), 7);
        assert_eq!(t.check("2.2.2.2", "u4", 4), 7);
    }

    #[test]
    fn user_bucket_trips_on_fourth_failure_from_distinct_ips() {
        // Both IP buckets are made lenient here to isolate user-long.
        let mut t = LoginThrottle::new((1000, 100, 5, 20), (1000, 100, 7, 40), (10, 3, 3, 30))
            .expect("valid test config");
        for i in 0..3u64 {
            let ip = format!("10.0.0.{i}");
            assert_eq!(t.record_failure(&ip, "victim", i), 0);
        }
        // Fourth failure against one username → 3s ban.
        assert_eq!(t.record_failure("10.0.0.3", "victim", 3), 3);
        assert_eq!(t.check("10.0.0.3", "victim", 3), 3);
    }

    #[test]
    fn check_returns_max_across_buckets() {
        // IP-short 2/10s → 5s; IP-long lenient; user-long 2/100s → 9s.
        let mut t = LoginThrottle::new((10, 2, 5, 20), (1000, 100, 7, 40), (100, 2, 9, 30))
            .expect("valid test config");
        assert_eq!(t.record_failure("10.0.0.1", "victim", 0), 0);
        assert_eq!(t.record_failure("10.0.0.2", "victim", 1), 0);
        // Third failure against the user bans for 9s, IP buckets quiet.
        assert_eq!(t.record_failure("10.0.0.3", "victim", 2), 9);
        assert_eq!(t.check("10.0.0.1", "victim", 2), 9);
        // Trip IP-short on 10.0.0.1 (already one failure in window).
        assert_eq!(t.record_failure("10.0.0.1", "a", 3), 0);
        assert_eq!(t.record_failure("10.0.0.1", "b", 4), 5);
        // At t=4: IP-short owes 5s, the user ban owes 7s → max wins.
        assert_eq!(t.check("10.0.0.1", "victim", 4), 7);
        assert_eq!(
            t.check("10.0.0.1", "b", 4),
            5,
            "fresh user, only the IP ban"
        );
        assert_eq!(
            t.check("10.0.0.9", "victim", 4),
            7,
            "user ban follows the username, not the IP"
        );
    }

    #[test]
    fn record_failure_returns_max_ban_applied() {
        // IP-short 2/10s → 5s; user-long 2/100s → 9s: the third failure
        // against both keys at once must return the larger ban (9).
        let mut t = LoginThrottle::new((10, 2, 5, 20), (1000, 100, 7, 40), (100, 2, 9, 30))
            .expect("valid test config");
        assert_eq!(t.record_failure("ip", "u", 0), 0);
        assert_eq!(t.record_failure("ip", "u", 1), 0);
        assert_eq!(t.record_failure("ip", "u", 2), 9);
        assert_eq!(t.check("ip", "u", 2), 9);
    }

    #[test]
    fn record_failure_during_ban_is_ignored_and_does_not_escalate() {
        let mut t = LoginThrottle::new((10, 2, 5, 20), (1000, 100, 7, 40), (1000, 100, 3, 30))
            .expect("valid test config");
        assert_eq!(t.record_failure("ip", "u", 0), 0);
        assert_eq!(t.record_failure("ip", "u", 1), 0);
        assert_eq!(t.record_failure("ip", "u", 2), 5);
        // Mid-ban calls return the remainder and are not recorded.
        assert_eq!(t.record_failure("ip", "u", 3), 4);
        assert_eq!(t.check("ip", "u", 3), 4, "original ban untouched");
        assert_eq!(t.record_failure("ip", "u", 5), 2);
        assert_eq!(t.check("ip", "u", 7), 0, "ban ends exactly at t=7");
        // Only four failures were ever recorded (t=3, t=5 were ignored),
        // so this one still shares the first rung instead of escalating.
        assert_eq!(t.record_failure("ip", "u", 7), 5);
    }

    #[test]
    fn success_clears_both_entries() {
        let mut t = tracker();
        assert_eq!(t.record_failure("ip", "u", 0), 0);
        assert_eq!(t.record_failure("ip", "u", 1), 0);
        assert_eq!(t.record_failure("ip", "u", 2), 5);
        assert_eq!(t.ip.len(), 1);
        assert_eq!(t.user.len(), 1);
        t.record_success("ip", "u");
        assert!(t.ip.is_empty(), "IP entry removed entirely");
        assert!(t.user.is_empty(), "user entry removed entirely");
        assert_eq!(t.check("ip", "u", 2), 0, "ban gone with the entries");
        assert_eq!(
            t.record_failure("ip", "u", 3),
            0,
            "failure counting restarts from zero"
        );
    }

    #[test]
    fn success_on_unknown_keys_is_noop() {
        let mut t = tracker();
        t.record_success("never-seen", "never-seen");
        assert!(t.ip.is_empty());
        assert!(t.user.is_empty());
    }

    #[test]
    fn sweep_evicts_idle_but_keeps_banned_entries() {
        // Long bans (5000s) outlast the longest window (1000s).
        let mut t =
            LoginThrottle::new((10, 2, 5000, 5000), (1000, 10, 1, 1), (1000, 2, 5000, 5000))
                .expect("valid test config");
        // `ip-a` / `a`: three failures → banned until t=5000.
        assert_eq!(t.record_failure("ip-a", "a", 0), 0);
        assert_eq!(t.record_failure("ip-a", "a", 0), 0);
        assert_eq!(t.record_failure("ip-a", "a", 0), 5000);
        // `ip-b` / `b`: one failure, no ban.
        assert_eq!(t.record_failure("ip-b", "b", 0), 0);

        t.sweep(999);
        assert_eq!(t.ip.len(), 2, "both younger than the longest window");
        assert_eq!(t.user.len(), 2);

        t.sweep(1000);
        assert_eq!(t.ip.len(), 1, "idle `ip-b` evicted, banned `ip-a` kept");
        assert!(t.ip.contains_key("ip-a"));
        assert_eq!(t.user.len(), 1, "idle `b` evicted, banned `a` kept");
        assert!(t.user.contains_key("a"));

        t.sweep(4000);
        assert_eq!(
            t.ip.len(),
            1,
            "ban outlives last_seen + longest_window, so no eviction"
        );
        assert_eq!(t.user.len(), 1);

        t.sweep(5000);
        assert!(t.ip.is_empty(), "ban over and idle → evicted");
        assert!(t.user.is_empty());
        assert_eq!(t.check("ip-a", "a", 5000), 0);
    }

    #[test]
    fn sweep_enforces_ip_cap_retaining_hottest() {
        let mut t = tracker();
        let total = MAX_IP_ENTRIES + 5;
        for i in 0..total {
            let ip = format!("10.1.{}.{}", i / 256, i % 256);
            t.ip.insert(ip, bare_ip_entry(i as u64));
        }
        assert_eq!(t.ip.len(), total);
        t.sweep(0);
        assert_eq!(t.ip.len(), MAX_IP_ENTRIES, "capped");
        assert_eq!(t.ip.values().map(|e| e.last_seen).min(), Some(5));
        assert_eq!(
            t.ip.values().map(|e| e.last_seen).max(),
            Some((total - 1) as u64)
        );
        let hottest = format!("10.1.{}.{}", (total - 1) / 256, (total - 1) % 256);
        assert!(t.ip.contains_key(&hottest), "hottest entry retained");
    }

    #[test]
    fn sweep_enforces_user_cap_retaining_hottest() {
        let mut t = tracker();
        let total = MAX_USER_ENTRIES + 5;
        for i in 0..total {
            t.user.insert(format!("user{i}"), bare_user_entry(i as u64));
        }
        assert_eq!(t.user.len(), total);
        t.sweep(0);
        assert_eq!(t.user.len(), MAX_USER_ENTRIES, "capped");
        assert_eq!(t.user.values().map(|e| e.last_seen).min(), Some(5));
        assert!(t.user.contains_key(&format!("user{}", total - 1)));
    }

    #[test]
    fn record_failure_keeps_entries_within_caps() {
        // All three buckets lenient so nothing bans; only the cap path runs.
        let mut t = LoginThrottle::new(
            (1000, u32::MAX, 1, 1),
            (1000, u32::MAX, 1, 1),
            (1000, u32::MAX, 1, 1),
        )
        .expect("valid test config");
        let total = MAX_IP_ENTRIES + 5;
        for i in 0..total {
            let ip = format!("10.2.{}.{}", i / 256, i % 256);
            t.record_failure(&ip, "victim", i as u64);
        }
        assert_eq!(t.ip.len(), MAX_IP_ENTRIES, "caps hold without sweep");
        assert_eq!(t.ip.values().map(|e| e.last_seen).min(), Some(5));
        assert_eq!(t.user.len(), 1);
    }

    #[test]
    fn constructor_propagates_invalid_bucket_params() {
        let ok: (u64, u32, u64, u64) = (10, 2, 5, 20);
        // IP-short tuple errors.
        assert!(
            LoginThrottle::new((0, 2, 5, 20), ok, ok).is_err(),
            "zero short window"
        );
        assert_eq!(
            LoginThrottle::new((0, 2, 5, 20), ok, ok).unwrap_err(),
            "throttle window must be at least 1 second"
        );
        assert!(
            LoginThrottle::new(ok, (100, 0, 7, 40), ok).is_err(),
            "zero long max_failures"
        );
        assert_eq!(
            LoginThrottle::new(ok, (100, 0, 7, 40), ok).unwrap_err(),
            "throttle must allow at least 1 failure before banning"
        );
        // IP-long / user-long tuple errors.
        assert!(
            LoginThrottle::new(ok, (100, 4, 0, 40), ok).is_err(),
            "zero long base"
        );
        assert_eq!(
            LoginThrottle::new(ok, ok, (100, 3, 0, 30)).unwrap_err(),
            "throttle base ban must be at least 1 second"
        );
        assert!(
            LoginThrottle::new(ok, ok, (100, 3, 40, 30)).is_err(),
            "user cap below base"
        );
        assert_eq!(
            LoginThrottle::new(ok, ok, (100, 3, 40, 30)).unwrap_err(),
            "throttle max ban must be at least the base ban"
        );
        // Minimal and full configs are accepted.
        assert!(LoginThrottle::new((1, 1, 1, 1), (1, 1, 1, 1), (1, 1, 1, 1)).is_ok());
        assert!(LoginThrottle::new(ok, (100, 4, 7, 40), (100, 3, 3, 30)).is_ok());
    }

    #[test]
    fn select_ip_ignores_xff_without_trusted_proxy() {
        assert_eq!(
            select_client_ip(Some("203.0.113.7"), Some("10.0.0.1"), false),
            "10.0.0.1"
        );
        assert_eq!(
            select_client_ip(Some("garbage"), Some("10.0.0.1"), false),
            "10.0.0.1",
            "even a valid-looking header is ignored untrusted"
        );
        assert_eq!(select_client_ip(None, Some("10.0.0.1"), false), "10.0.0.1");
        assert_eq!(
            select_client_ip(Some("203.0.113.7"), None, false),
            "unknown"
        );
        assert_eq!(select_client_ip(None, None, false), "unknown");
    }

    #[test]
    fn select_ip_trusted_proxy_uses_leftmost_valid_entry() {
        assert_eq!(
            select_client_ip(Some("203.0.113.7, 10.0.0.1"), Some("192.168.1.1"), true),
            "203.0.113.7"
        );
        assert_eq!(
            select_client_ip(Some("  203.0.113.7  ,10.0.0.1"), Some("192.168.1.1"), true),
            "203.0.113.7",
            "entries are trimmed"
        );
        assert_eq!(
            select_client_ip(Some("2001:db8::1,10.0.0.1"), None, true),
            "2001:db8::1"
        );
        assert_eq!(
            select_client_ip(Some("203.0.113.7"), None, true),
            "203.0.113.7"
        );
        assert_eq!(select_client_ip(None, Some("10.0.0.1"), true), "10.0.0.1");
    }

    #[test]
    fn select_ip_trusted_proxy_falls_back_on_bad_xff() {
        // Only the leftmost entry is ever considered: a bad leftmost entry
        // disqualifies the whole header, later entries are never scanned.
        assert_eq!(
            select_client_ip(Some("unknown, 203.0.113.7"), Some("10.0.0.1"), true),
            "10.0.0.1"
        );
        assert_eq!(
            select_client_ip(Some("evil garbage, 203.0.113.7"), Some("10.0.0.1"), true),
            "10.0.0.1"
        );
        assert_eq!(
            select_client_ip(Some(""), Some("10.0.0.1"), true),
            "10.0.0.1"
        );
        assert_eq!(
            select_client_ip(Some(" , 203.0.113.7"), Some("10.0.0.1"), true),
            "10.0.0.1",
            "empty leftmost entry"
        );
        assert_eq!(
            select_client_ip(Some("[2001:db8::1]"), Some("10.0.0.1"), true),
            "10.0.0.1",
            "bracketed IPv6 is not an IpAddr"
        );
        assert_eq!(select_client_ip(Some("nope"), None, true), "unknown");
        assert_eq!(select_client_ip(None, None, true), "unknown");
    }

    #[test]
    fn select_ip_keeps_unparseable_socket_verbatim() {
        // The socket comes from the transport layer: unlike XFF it is not
        // parsed; only absent/empty maps to "unknown".
        assert_eq!(
            select_client_ip(Some("garbage"), Some("not-an-ip"), true),
            "not-an-ip"
        );
        assert_eq!(
            select_client_ip(Some("garbage"), Some("not-an-ip"), false),
            "not-an-ip"
        );
        assert_eq!(select_client_ip(None, Some(""), true), "unknown");
    }
}
