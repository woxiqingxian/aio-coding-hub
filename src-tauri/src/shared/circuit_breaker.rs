//! Usage: In-memory circuit breaker to protect providers from repeated failures.

use super::mutex_ext::MutexExt;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::mpsc;

const DEFAULT_FAILURE_THRESHOLD: u32 = 5;
const DEFAULT_OPEN_DURATION_SECS: i64 = 30 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl CircuitState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "CLOSED",
            Self::Open => "OPEN",
            Self::HalfOpen => "HALF_OPEN",
        }
    }

    pub fn from_str(raw: &str) -> Self {
        match raw {
            "OPEN" => Self::Open,
            "HALF_OPEN" => Self::HalfOpen,
            _ => Self::Closed,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub open_duration_secs: i64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: DEFAULT_FAILURE_THRESHOLD,
            open_duration_secs: DEFAULT_OPEN_DURATION_SECS,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CircuitSnapshot {
    pub state: CircuitState,
    pub failure_count: u32,
    pub failure_threshold: u32,
    pub open_until: Option<i64>,
    pub cooldown_until: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct CircuitTransition {
    pub prev_state: CircuitState,
    pub next_state: CircuitState,
    pub reason: &'static str,
    pub snapshot: CircuitSnapshot,
}

#[derive(Debug, Clone)]
pub struct CircuitChange {
    pub before: CircuitSnapshot,
    pub after: CircuitSnapshot,
    pub transition: Option<CircuitTransition>,
}

#[derive(Debug, Clone)]
pub struct CircuitCheck {
    pub allow: bool,
    pub after: CircuitSnapshot,
    pub transition: Option<CircuitTransition>,
}

#[derive(Debug, Clone)]
pub struct CircuitPersistedState {
    pub provider_id: i64,
    pub state: CircuitState,
    pub failure_count: u32,
    pub open_until: Option<i64>,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
struct ProviderHealth {
    state: CircuitState,
    failure_count: u32,
    open_until: Option<i64>,
    cooldown_until: Option<i64>,
    updated_at: i64,
}

impl ProviderHealth {
    fn closed(provider_id: i64, now_unix: i64) -> (i64, Self) {
        (
            provider_id,
            Self {
                state: CircuitState::Closed,
                failure_count: 0,
                open_until: None,
                cooldown_until: None,
                updated_at: now_unix,
            },
        )
    }
}

#[derive(Debug)]
pub struct CircuitBreaker {
    config: Mutex<CircuitBreakerConfig>,
    health: Mutex<HashMap<i64, ProviderHealth>>,
    persist_tx: Option<mpsc::Sender<CircuitPersistedState>>,
}

impl CircuitBreaker {
    pub fn new(
        config: CircuitBreakerConfig,
        initial: HashMap<i64, CircuitPersistedState>,
        persist_tx: Option<mpsc::Sender<CircuitPersistedState>>,
    ) -> Self {
        let mut map = HashMap::with_capacity(initial.len());
        for (provider_id, item) in initial {
            map.insert(
                provider_id,
                ProviderHealth {
                    state: item.state,
                    failure_count: item.failure_count,
                    open_until: item.open_until,
                    cooldown_until: None,
                    updated_at: item.updated_at,
                },
            );
        }

        Self {
            config: Mutex::new(config),
            health: Mutex::new(map),
            persist_tx,
        }
    }

    fn read_config(&self) -> CircuitBreakerConfig {
        self.config.lock_or_recover().clone()
    }

    /// Hot-reload circuit breaker configuration.
    /// Recalculates `open_until` for all currently OPEN circuits based on the new duration.
    pub fn update_config(&self, new_config: CircuitBreakerConfig) {
        let mut upserts: Vec<CircuitPersistedState> = Vec::new();

        let old_duration = {
            let guard = self.config.lock_or_recover();
            guard.open_duration_secs
        };
        let new_duration = new_config.open_duration_secs;

        {
            let mut cfg_guard = self.config.lock_or_recover();
            *cfg_guard = new_config;
        }

        if old_duration != new_duration {
            let mut guard = self.health.lock_or_recover();
            for (&provider_id, entry) in guard.iter_mut() {
                if entry.state == CircuitState::Open {
                    let new_open_until = entry.updated_at.saturating_add(new_duration);
                    entry.open_until = Some(new_open_until);
                    upserts.push(Self::persisted_from_health(provider_id, entry));
                }
            }
        }

        for item in upserts {
            self.try_persist(item);
        }
    }

    #[allow(dead_code)]
    pub fn snapshot(&self, provider_id: i64, now_unix: i64) -> CircuitSnapshot {
        let cfg = self.read_config();
        let mut guard = self.health.lock_or_recover();
        let entry = guard
            .entry(provider_id)
            .or_insert_with(|| ProviderHealth::closed(provider_id, now_unix).1);
        Self::snapshot_from_health(&cfg, entry)
    }

    pub fn should_allow(&self, provider_id: i64, now_unix: i64) -> CircuitCheck {
        let cfg = self.read_config();
        let mut upsert: Option<CircuitPersistedState> = None;
        let mut transition: Option<CircuitTransition> = None;

        let (after, allow) = {
            let mut guard = self.health.lock_or_recover();
            let entry = guard
                .entry(provider_id)
                .or_insert_with(|| ProviderHealth::closed(provider_id, now_unix).1);

            if let Some(until) = entry.cooldown_until {
                if now_unix >= until {
                    entry.cooldown_until = None;
                }
            }

            if entry.state == CircuitState::Open {
                let expired = entry.open_until.map(|t| now_unix >= t).unwrap_or(true);
                if expired {
                    let prev = entry.state;
                    entry.state = CircuitState::HalfOpen;
                    entry.open_until = None;
                    entry.updated_at = now_unix;

                    let t = CircuitTransition {
                        prev_state: prev,
                        next_state: entry.state,
                        reason: "OPEN_EXPIRED",
                        snapshot: Self::snapshot_from_health(&cfg, entry),
                    };

                    transition = Some(t);
                    upsert = Some(Self::persisted_from_health(provider_id, entry));
                }
            }

            let after = Self::snapshot_from_health(&cfg, entry);
            let cooldown_active = entry.cooldown_until.map(|t| now_unix < t).unwrap_or(false);
            let allow = entry.state != CircuitState::Open && !cooldown_active;
            (after, allow)
        };

        if let Some(item) = upsert {
            self.try_persist(item);
        }

        CircuitCheck {
            allow,
            after,
            transition,
        }
    }

    pub fn record_success(&self, provider_id: i64, now_unix: i64) -> CircuitChange {
        let cfg = self.read_config();
        let mut upsert: Option<CircuitPersistedState> = None;
        let mut transition: Option<CircuitTransition> = None;

        let (before, after) = {
            let mut guard = self.health.lock_or_recover();
            let entry = guard
                .entry(provider_id)
                .or_insert_with(|| ProviderHealth::closed(provider_id, now_unix).1);

            let before = Self::snapshot_from_health(&cfg, entry);

            match entry.state {
                CircuitState::Closed => {
                    entry.cooldown_until = None;
                    if entry.failure_count != 0 {
                        entry.failure_count = 0;
                        entry.updated_at = now_unix;
                        upsert = Some(Self::persisted_from_health(provider_id, entry));
                    }
                }
                CircuitState::HalfOpen => {
                    let prev = entry.state;
                    entry.state = CircuitState::Closed;
                    entry.failure_count = 0;
                    entry.cooldown_until = None;
                    entry.updated_at = now_unix;

                    let t = CircuitTransition {
                        prev_state: prev,
                        next_state: entry.state,
                        reason: "HALF_OPEN_SUCCESS",
                        snapshot: Self::snapshot_from_health(&cfg, entry),
                    };
                    transition = Some(t);
                    upsert = Some(Self::persisted_from_health(provider_id, entry));
                }
                CircuitState::Open => {}
            }

            let after = Self::snapshot_from_health(&cfg, entry);
            (before, after)
        };

        if let Some(item) = upsert {
            self.try_persist(item);
        }

        CircuitChange {
            before,
            after,
            transition,
        }
    }

    pub fn record_failure(&self, provider_id: i64, now_unix: i64) -> CircuitChange {
        let cfg = self.read_config();
        let mut upsert: Option<CircuitPersistedState> = None;
        let mut transition: Option<CircuitTransition> = None;

        let (before, after) = {
            let mut guard = self.health.lock_or_recover();
            let entry = guard
                .entry(provider_id)
                .or_insert_with(|| ProviderHealth::closed(provider_id, now_unix).1);

            let before = Self::snapshot_from_health(&cfg, entry);

            match entry.state {
                CircuitState::Closed => {
                    entry.failure_count = entry.failure_count.saturating_add(1);
                    entry.updated_at = now_unix;

                    if entry.failure_count >= cfg.failure_threshold {
                        let prev = entry.state;
                        entry.state = CircuitState::Open;
                        entry.open_until = Some(now_unix.saturating_add(cfg.open_duration_secs));

                        let after = Self::snapshot_from_health(&cfg, entry);
                        let t = CircuitTransition {
                            prev_state: prev,
                            next_state: entry.state,
                            reason: "FAILURE_THRESHOLD_REACHED",
                            snapshot: after.clone(),
                        };
                        transition = Some(t);
                    }

                    upsert = Some(Self::persisted_from_health(provider_id, entry));
                }
                CircuitState::HalfOpen => {
                    let prev = entry.state;
                    entry.state = CircuitState::Open;
                    entry.open_until = Some(now_unix.saturating_add(cfg.open_duration_secs));
                    entry.updated_at = now_unix;

                    let after = Self::snapshot_from_health(&cfg, entry);
                    let t = CircuitTransition {
                        prev_state: prev,
                        next_state: entry.state,
                        reason: "HALF_OPEN_FAILURE",
                        snapshot: after.clone(),
                    };
                    transition = Some(t);
                    upsert = Some(Self::persisted_from_health(provider_id, entry));
                }
                CircuitState::Open => {}
            }

            let after = Self::snapshot_from_health(&cfg, entry);
            (before, after)
        };

        if let Some(item) = upsert {
            self.try_persist(item);
        }

        CircuitChange {
            before,
            after,
            transition,
        }
    }

    fn snapshot_from_health(
        cfg: &CircuitBreakerConfig,
        health: &ProviderHealth,
    ) -> CircuitSnapshot {
        CircuitSnapshot {
            state: health.state,
            failure_count: health.failure_count,
            failure_threshold: cfg.failure_threshold,
            open_until: health.open_until,
            cooldown_until: health.cooldown_until,
        }
    }

    fn persisted_from_health(provider_id: i64, health: &ProviderHealth) -> CircuitPersistedState {
        CircuitPersistedState {
            provider_id,
            state: health.state,
            failure_count: health.failure_count,
            open_until: health.open_until,
            updated_at: health.updated_at,
        }
    }

    pub fn trigger_cooldown(
        &self,
        provider_id: i64,
        now_unix: i64,
        cooldown_secs: i64,
    ) -> CircuitSnapshot {
        let cfg = self.read_config();
        let cooldown_secs = cooldown_secs.max(0);
        if provider_id <= 0 || cooldown_secs == 0 {
            return self.snapshot(provider_id, now_unix);
        }

        let mut guard = self.health.lock_or_recover();
        let entry = guard
            .entry(provider_id)
            .or_insert_with(|| ProviderHealth::closed(provider_id, now_unix).1);

        let next_until = now_unix.saturating_add(cooldown_secs);
        entry.cooldown_until = Some(match entry.cooldown_until {
            Some(existing) => existing.max(next_until),
            None => next_until,
        });
        entry.updated_at = now_unix;

        Self::snapshot_from_health(&cfg, entry)
    }

    pub fn reset(&self, provider_id: i64, now_unix: i64) -> CircuitSnapshot {
        let cfg = self.read_config();
        if provider_id <= 0 {
            return CircuitSnapshot {
                state: CircuitState::Closed,
                failure_count: 0,
                failure_threshold: cfg.failure_threshold,
                open_until: None,
                cooldown_until: None,
            };
        }

        let (after, upsert) = {
            let mut guard = self.health.lock_or_recover();
            let entry = guard
                .entry(provider_id)
                .or_insert_with(|| ProviderHealth::closed(provider_id, now_unix).1);

            entry.state = CircuitState::Closed;
            entry.failure_count = 0;
            entry.open_until = None;
            entry.cooldown_until = None;
            entry.updated_at = now_unix;

            let after = Self::snapshot_from_health(&cfg, entry);
            let upsert = Self::persisted_from_health(provider_id, entry);
            (after, upsert)
        };

        self.try_persist(upsert);

        after
    }

    fn try_persist(&self, item: CircuitPersistedState) {
        if let Some(tx) = &self.persist_tx {
            let _ = tx.try_send(item);
        }
    }
}

#[cfg(test)]
mod tests;
