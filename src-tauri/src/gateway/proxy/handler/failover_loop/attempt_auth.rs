//! Usage: Auth header injection and body cleaning for a single attempt.
//!
//! Centralizes the logic for building request headers, injecting
//! provider-specific authentication, and cleaning the request body
//! before sending upstream.

use super::*;
use super::attempt_executor::RetryLoopState;
use super::provider_iterator::PreparedProvider;
use crate::gateway::proxy::request_context::RequestContext;

/// Inject authentication headers based on provider type and auth mode.
///
/// Returns `Err(FailoverAttempt)` when OAuth header injection fails
/// (the attempt should be pushed to the attempts list and the retry
/// loop should break).
pub(super) fn inject_auth(
    ctx: CommonCtx<'_>,
    input: &RequestContext,
    prepared: &PreparedProvider,
    retry_state: &RetryLoopState,
    retry_index: u32,
    attempt_index: u32,
    attempt_started_ms: u128,
    circuit_before: &crate::circuit_breaker::CircuitSnapshot,
    headers: &mut HeaderMap,
) -> Result<(), FailoverAttempt> {
    // Always clear all auth headers (fail-closed).
    headers.remove(header::AUTHORIZATION);
    headers.remove("x-api-key");
    headers.remove("x-goog-api-key");
    headers.remove("x-goog-api-client");

    let upstream_cli_key = if prepared.cx2cc_active {
        prepared
            .cx2cc_source
            .as_ref()
            .map(|(_, source_cli_key)| source_cli_key.as_str())
            .unwrap_or("codex")
    } else {
        input.cli_key.as_str()
    };
    strip_incompatible_protocol_headers(input.cli_key.as_str(), upstream_cli_key, headers);

    if prepared.oauth_adapter.is_some() {
        inject_oauth_auth(prepared, input, attempt_index, retry_index,
            attempt_started_ms, circuit_before, headers)?;
    } else {
        inject_standard_auth(ctx, input, prepared, retry_state, retry_index, headers);
    }

    if prepared.use_codex_chatgpt_backend {
        maybe_inject_codex_chatgpt_headers(headers, prepared.codex_chatgpt_account_id.as_deref());
    }
    if prepared.strip_request_content_encoding {
        headers.remove(header::CONTENT_ENCODING);
    }

    Ok(())
}

/// Clean request body (e.g. remove empty text blocks for Claude OAuth).
pub(super) fn clean_body(
    input: &RequestContext,
    prepared: &PreparedProvider,
) -> Bytes {
    if input.cli_key == "claude" && prepared.oauth_adapter.is_some() {
        if let Ok(mut json) =
            serde_json::from_slice::<serde_json::Value>(&prepared.upstream_body_bytes)
        {
            if let Some(messages) = json.get_mut("messages").and_then(|v| v.as_array_mut()) {
                for msg in messages {
                    if let Some(content) = msg.get_mut("content").and_then(|v| v.as_array_mut()) {
                        content.retain(|block| {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                !text.trim().is_empty()
                            } else {
                                true
                            }
                        });
                    }
                }
            }
            return serde_json::to_vec(&json)
                .unwrap_or_else(|_| prepared.upstream_body_bytes.to_vec())
                .into();
        }
    }
    prepared.upstream_body_bytes.clone()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn inject_oauth_auth(
    prepared: &PreparedProvider,
    input: &RequestContext,
    attempt_index: u32,
    retry_index: u32,
    attempt_started_ms: u128,
    circuit_before: &crate::circuit_breaker::CircuitSnapshot,
    headers: &mut HeaderMap,
) -> Result<(), FailoverAttempt> {
    let cred_trimmed = prepared.effective_credential.trim();
    tracing::debug!(
        provider_id = prepared.provider_id,
        cli_key = %input.cli_key,
        credential_len = cred_trimmed.len(),
        "injecting OAuth upstream headers"
    );
    match prepared.oauth_adapter {
        Some(adapter) => {
            if let Err(e) = adapter.inject_upstream_headers(headers, cred_trimmed) {
                tracing::warn!(
                    provider_id = prepared.provider_id,
                    cli_key = %input.cli_key,
                    "OAuth inject_upstream_headers failed, skipping provider: {e}"
                );
                return Err(FailoverAttempt {
                    provider_id: prepared.provider_id,
                    provider_name: prepared.provider_name_base.clone(),
                    base_url: prepared.provider_base_url_display.clone(),
                    outcome: format!("oauth_inject_failed: {e}"),
                    status: Some(500),
                    provider_index: Some(attempt_index),
                    retry_index: Some(retry_index),
                    session_reuse: None,
                    error_category: Some("auth"),
                    error_code: Some(GatewayErrorCode::InternalError.as_str()),
                    decision: Some("switch"),
                    reason: Some(format!("OAuth header injection failed: {e}")),
                    selection_method: None,
                    reason_code: None,
                    attempt_started_ms: Some(attempt_started_ms),
                    attempt_duration_ms: Some(0),
                    circuit_state_before: Some(circuit_before.state.as_str()),
                    circuit_state_after: None,
                    circuit_failure_count: Some(circuit_before.failure_count),
                    circuit_failure_threshold: Some(circuit_before.failure_threshold),
                });
            }
            Ok(())
        }
        None => {
            tracing::warn!(
                provider_id = prepared.provider_id,
                "oauth_adapter is None at injection point (should have been skipped earlier)"
            );
            Err(FailoverAttempt {
                provider_id: prepared.provider_id,
                provider_name: prepared.provider_name_base.clone(),
                base_url: prepared.provider_base_url_display.clone(),
                outcome: "oauth_adapter_missing".to_string(),
                status: Some(500),
                provider_index: Some(attempt_index),
                retry_index: Some(retry_index),
                session_reuse: None,
                error_category: Some("auth"),
                error_code: Some(GatewayErrorCode::InternalError.as_str()),
                decision: Some("switch"),
                reason: Some("OAuth adapter unexpectedly None".to_string()),
                selection_method: None,
                reason_code: None,
                attempt_started_ms: Some(attempt_started_ms),
                attempt_duration_ms: Some(0),
                circuit_state_before: Some(circuit_before.state.as_str()),
                circuit_state_after: None,
                circuit_failure_count: Some(circuit_before.failure_count),
                circuit_failure_threshold: Some(circuit_before.failure_threshold),
            })
        }
    }
}

fn inject_standard_auth(
    ctx: CommonCtx<'_>,
    input: &RequestContext,
    prepared: &PreparedProvider,
    retry_state: &RetryLoopState,
    retry_index: u32,
    headers: &mut HeaderMap,
) {
    let auth_cli_key = if prepared.cx2cc_active {
        "codex"
    } else {
        &input.cli_key
    };
    inject_provider_auth(auth_cli_key, prepared.effective_credential.trim(), headers);

    if !prepared.cx2cc_active && auth_cli_key == "claude" {
        if retry_state.claude_api_key_bearer_fallback {
            let value = format!("Bearer {}", prepared.effective_credential.trim());
            if let Ok(header_value) = HeaderValue::from_str(&value) {
                headers.remove("x-api-key");
                headers.insert(header::AUTHORIZATION, header_value);
            }
        }

        if retry_index == 1 || retry_state.claude_api_key_bearer_fallback {
            let mut settings = ctx.special_settings.lock_or_recover();
            settings.push(serde_json::json!({
                "type": "claude_auth_injection",
                "scope": "attempt",
                "providerId": prepared.provider_id,
                "providerName": prepared.provider_name_base.clone(),
                "retryAttemptNumber": retry_index,
                "mode": if retry_state.claude_api_key_bearer_fallback { "authorization_bearer" } else { "x_api_key" },
            }));
        }
    }
}
