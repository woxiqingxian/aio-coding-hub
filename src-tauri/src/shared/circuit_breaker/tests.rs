use super::*;

fn breaker() -> CircuitBreaker {
    CircuitBreaker::new(CircuitBreakerConfig::default(), HashMap::new(), None)
}

#[test]
fn closed_to_open_after_threshold() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    for i in 1..=DEFAULT_FAILURE_THRESHOLD {
        let change = cb.record_failure(pid, now + i as i64);
        if i < DEFAULT_FAILURE_THRESHOLD {
            assert_eq!(change.after.state, CircuitState::Closed);
        }
    }

    let snap = cb.snapshot(pid, now + 100);
    assert_eq!(snap.state, CircuitState::Open);
    assert!(snap.open_until.is_some());
}

#[test]
fn open_expires_to_half_open() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    for i in 1..=DEFAULT_FAILURE_THRESHOLD {
        cb.record_failure(pid, now + i as i64);
    }

    let snap = cb.snapshot(pid, now + 10);
    assert_eq!(snap.state, CircuitState::Open);
    let open_until = snap.open_until.expect("open_until");

    let check = cb.should_allow(pid, open_until);
    assert!(check.allow);
    assert_eq!(check.after.state, CircuitState::HalfOpen);
    assert!(check.transition.is_some());
    let t = check.transition.unwrap();
    assert_eq!(t.prev_state, CircuitState::Open);
    assert_eq!(t.next_state, CircuitState::HalfOpen);
    assert_eq!(t.reason, "OPEN_EXPIRED");
}

#[test]
fn half_open_success_transitions_to_closed() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    for i in 1..=DEFAULT_FAILURE_THRESHOLD {
        cb.record_failure(pid, now + i as i64);
    }

    let open_until = cb.snapshot(pid, now + 10).open_until.expect("open_until");
    cb.should_allow(pid, open_until); // transitions to HalfOpen

    let change = cb.record_success(pid, open_until + 1);
    assert_eq!(change.after.state, CircuitState::Closed);
    assert_eq!(change.after.failure_count, 0);
    assert!(change.transition.is_some());
    let t = change.transition.unwrap();
    assert_eq!(t.prev_state, CircuitState::HalfOpen);
    assert_eq!(t.next_state, CircuitState::Closed);
    assert_eq!(t.reason, "HALF_OPEN_SUCCESS");
}

#[test]
fn half_open_failure_transitions_back_to_open() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    for i in 1..=DEFAULT_FAILURE_THRESHOLD {
        cb.record_failure(pid, now + i as i64);
    }

    let open_until = cb.snapshot(pid, now + 10).open_until.expect("open_until");
    cb.should_allow(pid, open_until); // transitions to HalfOpen

    let change = cb.record_failure(pid, open_until + 1);
    assert_eq!(change.after.state, CircuitState::Open);
    assert!(change.after.open_until.is_some());
    assert!(change.transition.is_some());
    let t = change.transition.unwrap();
    assert_eq!(t.prev_state, CircuitState::HalfOpen);
    assert_eq!(t.next_state, CircuitState::Open);
    assert_eq!(t.reason, "HALF_OPEN_FAILURE");
}

#[test]
fn success_clears_failure_count() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    cb.record_failure(pid, now);
    let before = cb.snapshot(pid, now + 1);
    assert_eq!(before.failure_count, 1);

    cb.record_success(pid, now + 2);
    let after = cb.snapshot(pid, now + 3);
    assert_eq!(after.failure_count, 0);
    assert_eq!(after.state, CircuitState::Closed);
}

#[test]
fn reset_clears_open_and_cooldown() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    for i in 1..=DEFAULT_FAILURE_THRESHOLD {
        cb.record_failure(pid, now + i as i64);
    }

    let open = cb.snapshot(pid, now + 10);
    assert_eq!(open.state, CircuitState::Open);

    let reset = cb.reset(pid, now + 20);
    assert_eq!(reset.state, CircuitState::Closed);
    assert_eq!(reset.failure_count, 0);
    assert!(reset.open_until.is_none());
    assert!(reset.cooldown_until.is_none());

    let allow = cb.should_allow(pid, now + 21);
    assert!(allow.allow);
}

#[test]
fn reset_clears_half_open() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    for i in 1..=DEFAULT_FAILURE_THRESHOLD {
        cb.record_failure(pid, now + i as i64);
    }

    let open_until = cb.snapshot(pid, now + 10).open_until.expect("open_until");
    cb.should_allow(pid, open_until); // transitions to HalfOpen

    let snap = cb.snapshot(pid, open_until);
    assert_eq!(snap.state, CircuitState::HalfOpen);

    let reset = cb.reset(pid, open_until + 1);
    assert_eq!(reset.state, CircuitState::Closed);
    assert_eq!(reset.failure_count, 0);
}

#[test]
fn update_config_recalculates_open_until() {
    let cb = breaker(); // default: 30min open duration
    let pid = 1;
    let now = 1_000;

    // Trip the circuit breaker
    for i in 1..=DEFAULT_FAILURE_THRESHOLD {
        cb.record_failure(pid, now + i as i64);
    }

    let snap = cb.snapshot(pid, now + 10);
    assert_eq!(snap.state, CircuitState::Open);
    let original_open_until = snap.open_until.expect("open_until");
    // Default: open_until = updated_at + 30*60
    assert_eq!(
        original_open_until,
        (now + DEFAULT_FAILURE_THRESHOLD as i64) + DEFAULT_OPEN_DURATION_SECS
    );

    // Hot-reload config: reduce to 60 seconds
    cb.update_config(CircuitBreakerConfig {
        failure_threshold: DEFAULT_FAILURE_THRESHOLD,
        open_duration_secs: 60,
    });

    let snap_after = cb.snapshot(pid, now + 10);
    assert_eq!(snap_after.state, CircuitState::Open);
    let new_open_until = snap_after.open_until.expect("open_until");
    // New: open_until = updated_at + 60
    assert_eq!(
        new_open_until,
        (now + DEFAULT_FAILURE_THRESHOLD as i64) + 60
    );
    assert!(new_open_until < original_open_until);

    // Verify circuit expires at the new time
    let check = cb.should_allow(pid, new_open_until);
    assert!(check.allow);
    assert_eq!(check.after.state, CircuitState::HalfOpen);
}

#[test]
fn update_config_new_failures_use_new_duration() {
    let cb = CircuitBreaker::new(
        CircuitBreakerConfig {
            failure_threshold: 2,
            open_duration_secs: 600,
        },
        HashMap::new(),
        None,
    );
    let pid = 1;
    let now = 1_000;

    // Hot-reload to shorter duration BEFORE tripping
    cb.update_config(CircuitBreakerConfig {
        failure_threshold: 2,
        open_duration_secs: 30,
    });

    // Trip the circuit
    cb.record_failure(pid, now);
    cb.record_failure(pid, now + 1);

    let snap = cb.snapshot(pid, now + 2);
    assert_eq!(snap.state, CircuitState::Open);
    // open_until should use the new 30s duration, not the original 600s
    let open_until = snap.open_until.expect("open_until");
    assert_eq!(open_until, (now + 1) + 30);
}
