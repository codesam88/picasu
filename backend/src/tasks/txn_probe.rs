//! Observability probes for cache-DB write slots and shard read guards.
//!
//! Observational only: these never change control flow or return values.
//! Gated behind the existing `PICASU_OBSERVER_STATS` opt-in (no new env var).

use crate::tasks::observe;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Db {
    Cache,
    Tree,
    Expire,
}

static REGISTRY: Mutex<Option<HashMap<Db, (&'static str, Instant)>>> = Mutex::new(None);

pub struct SlotGuard {
    db: Db,
    site: &'static str,
    start: Instant,
    holder_site: Option<&'static str>,
    holder_start: Option<Instant>,
    acquired_at: Cell<Option<Instant>>,
}

pub fn slot_guard(db: Db, site: &'static str) -> SlotGuard {
    let (holder_site, holder_start) = if observe::observer_stats() {
        let guard = REGISTRY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard
            .as_ref()
            .and_then(|m| m.get(&db).copied())
            .map_or((None, None), |(s, i)| (Some(s), Some(i)))
    } else {
        (None, None)
    };
    SlotGuard {
        db,
        site,
        start: Instant::now(),
        holder_site,
        holder_start,
        acquired_at: Cell::new(None),
    }
}

impl SlotGuard {
    pub fn acquired(&self) {
        if !observe::observer_stats() {
            self.acquired_at.set(Some(Instant::now()));
            return;
        }
        let wait_ms = u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX);
        let holder = self.holder_site.unwrap_or("none");
        let held_ms = self.holder_start.map_or(wait_ms, |i| {
            u64::try_from(i.elapsed().as_millis()).unwrap_or(u64::MAX)
        });
        log::warn!(
            "slot wait db={:?} waiter={} holder={} held_ms={}",
            self.db,
            self.site,
            holder,
            held_ms
        );
        let mut guard = REGISTRY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(self.db, (self.site, Instant::now()));
        self.acquired_at.set(Some(Instant::now()));
    }
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        if !observe::observer_stats() {
            return;
        }
        let Some(acquired) = self.acquired_at.get() else {
            return;
        };
        let held_ms = u64::try_from(acquired.elapsed().as_millis()).unwrap_or(u64::MAX);
        if held_ms > 50 {
            log::warn!(
                "slot hold db={:?} site={} held_ms={}",
                self.db,
                self.site,
                held_ms
            );
        }
        let mut guard = REGISTRY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(map) = guard.as_mut()
            && map.get(&self.db).is_some_and(|(s, _)| *s == self.site)
        {
            map.remove(&self.db);
        }
    }
}

pub struct GuardProbe {
    site: &'static str,
    start: Instant,
    threshold_ms: u64,
}

pub fn guard_probe(site: &'static str) -> GuardProbe {
    GuardProbe {
        site,
        start: Instant::now(),
        threshold_ms: 20,
    }
}

impl Drop for GuardProbe {
    fn drop(&mut self) {
        if !observe::observer_stats() {
            return;
        }
        let held_ms = u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX);
        if held_ms > self.threshold_ms {
            log::warn!("guard hold site={} held_ms={}", self.site, held_ms);
        }
    }
}
