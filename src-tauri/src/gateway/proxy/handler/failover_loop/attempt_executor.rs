//! Usage: Single attempt execution (build request, send upstream, return result).
//!
//! Encapsulates URL construction, header assembly, auth injection, body
//! cleaning, and the upstream send for one retry attempt.

use super::*;
use super::provider_iterator::PreparedProvider;

/// Mutable per-provider state that persists across retries within one provider.
pub(super) struct RetryLoopState {
    pub(super) claude_api_key_bearer_fallback: bool,
    pub(super) oauth_reactive_refreshed_once: bool,
    pub(super) thinking_signature_rectifier_retried: bool,
    pub(super) thinking_budget_rectifier_retried: bool,
}

impl RetryLoopState {
    pub(super) fn new() -> Self {
        Self {
            claude_api_key_bearer_fallback: false,
            oauth_reactive_refreshed_once: false,
            thinking_signature_rectifier_retried: false,
            thinking_budget_rectifier_retried: false,
        }
    }
}

/// Result of building + sending one attempt.
pub(super) enum AttemptSendOutcome {
    Response(reqwest::Response),
    Timeout,
    ReqwestError(reqwest::Error),
    /// URL build failure already recorded; caller should apply the returned LoopControl.
    UrlBuildFailed(LoopControl),
    /// OAuth adapter injection failed; break out of retry loop for this provider.
    OAuthInjectFailed,
}

/// Build request headers, inject auth, clean body, send upstream, and return
/// the raw outcome. The caller (retry engine / response router) handles the
/// result.
#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_attempt(
    ctx: CommonCtx<'_>,
    input: &super::super::super::request_context::RequestContext,
    abort_guard: &mut super::super::super::abort_guard::RequestAbortGuard,
    prepared: &mut PreparedProvider,
    retry_state: &mut RetryLoopState,
    retry_index: u32,
    attempt_index: u32,
    attempts: &mut Vec<FailoverAttempt>,
    failed_provider_ids: &mut HashSet<i64>,
    last_error_category: &mut Option<&'static str>,
    last_error_code: &mut Option<&'static str>,
    circuit_snapshot: &mut crate::circuit_breaker::CircuitSnapshot,
) -> AttemptSendOutcome {
    let attempt_started_ms = input.started.elapsed().as_millis();
    let circuit_before = prepared.circuit_snapshot.clone();

    // --- Build URL ---
    let url = match build_url(ctx, input, prepared, attempt_index, retry_index,
        attempt_started_ms, &circuit_before, attempts, failed_provider_ids,
        last_error_category, last_error_code, circuit_snapshot, abort_guard,
    ).await {
        Ok(u) => u,
        Err(outcome) => return outcome,
    };

    // --- Emit "started" attempt event ---
    emit_started_event(input, prepared, attempt_index, retry_index,
        attempt_started_ms, &circuit_before, abort_guard);

    // --- Build headers + inject auth ---
    let mut headers = input.base_headers.clone();
    ensure_cli_required_headers(&input.cli_key, &mut headers);
    if input.cli_key == "claude" {
        mark_internal_forwarded_request(&mut headers);
    }
    codex_session_id_completion::inject_session_headers_if_needed(
        &mut headers,
        prepared.cx2cc_codex_session_id.as_deref(),
    );

    if let Err(failed_attempt) = attempt_auth::inject_auth(
        ctx, input, prepared, retry_state, retry_index, attempt_index,
        attempt_started_ms, &circuit_before, &mut headers,
    ) {
        attempts.push(failed_attempt);
        return AttemptSendOutcome::OAuthInjectFailed;
    }

    // --- Clean body + send upstream ---
    let cleaned_body = attempt_auth::clean_body(input, prepared);

    let send_result = send::send_upstream(
        ctx, input.req_method.clone(), url, headers, cleaned_body,
    )
    .await;

    match send_result {
        send::SendResult::Ok(resp) => AttemptSendOutcome::Response(resp),
        send::SendResult::Timeout => AttemptSendOutcome::Timeout,
        send::SendResult::Err(err) => AttemptSendOutcome::ReqwestError(err),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn build_url(
    ctx: CommonCtx<'_>,
    input: &super::super::super::request_context::RequestContext,
    prepared: &PreparedProvider,
    attempt_index: u32,
    retry_index: u32,
    attempt_started_ms: u128,
    circuit_before: &crate::circuit_breaker::CircuitSnapshot,
    attempts: &mut Vec<FailoverAttempt>,
    failed_provider_ids: &mut HashSet<i64>,
    last_error_category: &mut Option<&'static str>,
    last_error_code: &mut Option<&'static str>,
    circuit_snapshot: &mut crate::circuit_breaker::CircuitSnapshot,
    abort_guard: &mut super::super::super::abort_guard::RequestAbortGuard,
) -> Result<reqwest::Url, AttemptSendOutcome> {
    match build_target_url(
        &prepared.provider_base_url_base,
        &prepared.upstream_forwarded_path,
        prepared.upstream_query.as_deref(),
    ) {
        Ok(u) => Ok(u),
        Err(err) => {
            tracing::warn!(
                trace_id = %input.trace_id,
                cli_key = %input.cli_key,
                provider_id = prepared.provider_id,
                provider_name = %prepared.provider_name_base,
                base_url = %prepared.provider_base_url_base,
                forwarded_path = %prepared.upstream_forwarded_path,
                "build_target_url failed: {err}"
            );
            let category = ErrorCategory::SystemError;
            let error_code = GatewayErrorCode::InternalError.as_str();
            let decision = FailoverDecision::SwitchProvider;
            let outcome = format!(
                "build_target_url_error: category={} code={} decision={} err={err}",
                category.as_str(), error_code, decision.as_str(),
            );
            let attempt_started = Instant::now();
            let attempt_ctx = AttemptCtx {
                attempt_index, retry_index, attempt_started_ms, attempt_started,
                circuit_before,
                gemini_oauth_response_mode: prepared.gemini_oauth_response_mode,
                cx2cc_active: prepared.cx2cc_active,
                anthropic_stream_requested: prepared.anthropic_stream_requested,
            };
            let provider_ctx = ProviderCtx {
                provider_id: prepared.provider_id,
                provider_name_base: &prepared.provider_name_base,
                provider_base_url_base: &prepared.provider_base_url_base,
                provider_index: prepared.provider_index,
                session_reuse: prepared.session_reuse,
                stream_idle_timeout_seconds: prepared.stream_idle_timeout_seconds,
            };
            let loop_state = LoopState::new(
                attempts, failed_provider_ids, last_error_category,
                last_error_code, circuit_snapshot, abort_guard,
            );
            let ctrl = record_system_failure_and_decide_no_cooldown(
                RecordSystemFailureArgs {
                    ctx, provider_ctx, attempt_ctx, loop_state,
                    status: None, error_code, decision, outcome,
                    reason: format!("invalid base_url: {err}"),
                },
            )
            .await;
            Err(AttemptSendOutcome::UrlBuildFailed(ctrl))
        }
    }
}

fn emit_started_event(
    input: &super::super::super::request_context::RequestContext,
    prepared: &PreparedProvider,
    attempt_index: u32,
    retry_index: u32,
    attempt_started_ms: u128,
    circuit_before: &crate::circuit_breaker::CircuitSnapshot,
    abort_guard: &mut super::super::super::abort_guard::RequestAbortGuard,
) {
    let started_attempt = FailoverAttempt {
        provider_id: prepared.provider_id,
        provider_name: prepared.provider_name_base.clone(),
        base_url: prepared.provider_base_url_base.clone(),
        outcome: "started".to_string(),
        status: None,
        provider_index: Some(prepared.provider_index),
        retry_index: Some(retry_index),
        session_reuse: prepared.session_reuse,
        error_category: None,
        error_code: None,
        decision: None,
        reason: None,
        selection_method: dc::selection_method(
            prepared.provider_index, retry_index, prepared.session_reuse,
        ),
        reason_code: None,
        attempt_started_ms: Some(attempt_started_ms),
        attempt_duration_ms: Some(0),
        circuit_state_before: Some(circuit_before.state.as_str()),
        circuit_state_after: None,
        circuit_failure_count: Some(circuit_before.failure_count),
        circuit_failure_threshold: Some(circuit_before.failure_threshold),
    };
    abort_guard.capture_in_flight_attempt(&started_attempt);
    if input.observe_request {
        emit_attempt_event(
            &input.state.app,
            GatewayAttemptEvent {
                trace_id: input.trace_id.clone(),
                cli_key: input.cli_key.clone(),
                session_id: input.session_id.clone(),
                method: input.method_hint.clone(),
                path: input.forwarded_path.clone(),
                query: input.query.clone(),
                requested_model: input.requested_model.clone(),
                attempt_index,
                provider_id: prepared.provider_id,
                session_reuse: prepared.session_reuse,
                provider_name: prepared.provider_name_base.clone(),
                base_url: prepared.provider_base_url_base.clone(),
                outcome: "started".to_string(),
                status: None,
                attempt_started_ms,
                attempt_duration_ms: 0,
                circuit_state_before: Some(circuit_before.state.as_str()),
                circuit_state_after: None,
                circuit_failure_count: Some(circuit_before.failure_count),
                circuit_failure_threshold: Some(circuit_before.failure_threshold),
            },
        );
    }
}
