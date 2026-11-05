use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub(super) type AttemptCount = u32;

const MAX_TRACKED_ENTRIES: usize = 4_096;
const MAX_KEY_BYTES: usize = 128;
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(super) struct Limits {
    state: Mutex<State>,
}

impl Limits {
    pub(super) fn allow(
        &self,
        bucket: &'static str,
        key: &str,
        maximum: AttemptCount,
        window: Duration,
    ) -> bool {
        self.allow_at(bucket, key, maximum, window, Instant::now())
    }

    fn allow_at(
        &self,
        bucket: &'static str,
        key: &str,
        maximum: AttemptCount,
        window: Duration,
        now: Instant,
    ) -> bool {
        if key.len() > MAX_KEY_BYTES || maximum == 0 || window.is_zero() {
            return false;
        }

        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        state.sweep_if_due(now);

        if let Some(counter) = state
            .counters
            .get_mut(bucket)
            .and_then(|counters| counters.get_mut(key))
        {
            return counter.allow_at(now, maximum, window);
        }

        if state.tracked_entries == MAX_TRACKED_ENTRIES {
            if state.capacity_sweep_completed {
                return false;
            }
            state.sweep_expired(now);
            if state.tracked_entries == MAX_TRACKED_ENTRIES {
                return false;
            }
        }

        let counters = state.counters.entry(bucket).or_default();
        counters.insert(key.to_owned(), Counter::new(now, window));
        state.tracked_entries += 1;
        state.last_sweep.get_or_insert(now);
        true
    }
}

#[derive(Default)]
struct State {
    counters: HashMap<&'static str, HashMap<String, Counter>>,
    tracked_entries: usize,
    last_sweep: Option<Instant>,
    capacity_sweep_completed: bool,
}

impl State {
    fn sweep_if_due(&mut self, now: Instant) {
        let due = self
            .last_sweep
            .is_some_and(|last_sweep| now.saturating_duration_since(last_sweep) >= SWEEP_INTERVAL);
        if due {
            self.sweep_expired(now);
        }
    }

    fn sweep_expired(&mut self, now: Instant) {
        let mut tracked_entries = 0;
        self.counters.retain(|_, counters| {
            counters.retain(|_, counter| !counter.expired_at(now));
            tracked_entries += counters.len();
            !counters.is_empty()
        });
        self.tracked_entries = tracked_entries;
        self.last_sweep = Some(now);
        self.capacity_sweep_completed = self.tracked_entries == MAX_TRACKED_ENTRIES;
    }
}

struct Counter {
    started_at: Instant,
    attempts: AttemptCount,
    window: Duration,
}

impl Counter {
    fn new(now: Instant, window: Duration) -> Self {
        Self {
            started_at: now,
            attempts: 1,
            window,
        }
    }

    fn allow_at(&mut self, now: Instant, maximum: AttemptCount, window: Duration) -> bool {
        if self.window != window || self.expired_at(now) {
            self.started_at = now;
            self.attempts = 0;
            self.window = window;
        }
        if self.attempts >= maximum {
            return false;
        }
        self.attempts += 1;
        true
    }

    fn expired_at(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started_at) >= self.window
    }
}

#[cfg(test)]
#[path = "../../unit-tests/oauth/limits.rs"]
mod tests;
