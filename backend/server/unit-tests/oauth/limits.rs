use std::time::{Duration, Instant};

use super::{AttemptCount, Limits, MAX_KEY_BYTES, MAX_TRACKED_ENTRIES};

#[test]
fn allows_attempts_up_to_the_fixed_window_limit() {
    let limits = Limits::default();
    let now = Instant::now();
    let maximum: AttemptCount = 2;
    let window = Duration::from_secs(30);

    assert!(limits.allow_at("login", "client", maximum, window, now));
    assert!(limits.allow_at("login", "client", maximum, window, now));
    assert!(!limits.allow_at("login", "client", maximum, window, now));
}

#[test]
fn keeps_buckets_independent_for_the_same_key() {
    let limits = Limits::default();
    let now = Instant::now();
    let maximum = 1;
    let window = Duration::from_secs(30);

    assert!(limits.allow_at("login", "client", maximum, window, now));
    assert!(limits.allow_at("token", "client", maximum, window, now));
    assert!(!limits.allow_at("login", "client", maximum, window, now));
    assert!(!limits.allow_at("token", "client", maximum, window, now));
}

#[test]
fn expired_counter_starts_a_new_window_without_waiting() {
    let limits = Limits::default();
    let now = Instant::now();
    let window = Duration::from_secs(10);

    assert!(limits.allow_at("login", "client", 1, window, now));
    assert!(!limits.allow_at("login", "client", 1, window, now));
    assert!(limits.allow_at("login", "client", 1, window, now + window));
}

#[test]
fn capacity_and_key_length_are_bounded() {
    let limits = Limits::default();
    let now = Instant::now();
    let window = Duration::from_secs(120);
    let oversized_key = "k".repeat(MAX_KEY_BYTES + 1);

    assert!(!limits.allow_at("login", &oversized_key, 1, window, now));
    assert!(limits.allow_at("login", "k".repeat(MAX_KEY_BYTES).as_str(), 1, window, now));

    for index in 1..MAX_TRACKED_ENTRIES {
        let key = index.to_string();
        assert!(limits.allow_at("login", &key, 1, window, now));
    }

    assert!(!limits.allow_at("login", "one-too-many", 1, window, now));
}

#[test]
fn capacity_sweep_prunes_expired_entries() {
    let limits = Limits::default();
    let now = Instant::now();
    let window = Duration::from_secs(1);

    for index in 0..MAX_TRACKED_ENTRIES {
        let key = index.to_string();
        assert!(limits.allow_at("login", &key, 1, window, now));
    }
    assert!(!limits.allow_at("login", "new", 1, window, now));
    assert!(limits.allow_at("login", "new", 1, window, now + Duration::from_secs(60),));
}
