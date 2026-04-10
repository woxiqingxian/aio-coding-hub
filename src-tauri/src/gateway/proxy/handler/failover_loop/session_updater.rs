//! Usage: Session binding update helpers.
//!
//! Centralizes the logic for binding session state after a provider attempt
//! succeeds. The session manager is responsible for TTL-based expiry; there
//! is no explicit "unbind on failure" today -- failed providers are skipped
//! via `failed_provider_ids` in the failover loop, and stale bindings expire
//! naturally.

use crate::gateway::manager::GatewayAppState;
use crate::gateway::util::now_unix_seconds;

/// Bind a successful provider to the session so subsequent requests from the
/// same CLI session are routed to the same provider.
#[allow(dead_code)]
pub(super) fn bind_session_on_success(
    state: &GatewayAppState,
    cli_key: &str,
    session_id: Option<&str>,
    provider_id: i64,
    sort_mode_id: Option<i64>,
) {
    if let Some(session_id) = session_id {
        let now_unix = now_unix_seconds() as i64;
        state
            .session
            .bind_success(cli_key, session_id, provider_id, sort_mode_id, now_unix);
    }
}
