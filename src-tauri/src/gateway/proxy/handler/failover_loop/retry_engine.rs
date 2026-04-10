//! Usage: Retry decision engine (error classification -> retry same / switch / abort).
//!
//! Processes the outcome of a single attempt and decides the next action:
//! continue retrying the same provider, switch to the next provider, or
//! return a final response to the client.

use super::*;
use super::attempt_executor::{AttemptSendOutcome, RetryLoopState};
use super::provider_iterator::PreparedProvider;

/// Run the inner retry loop for a single prepared provider.
///
/// Returns `Some(Response)` if a final response was produced (success or
/// terminal error); returns `None` when all retries for this provider are
/// exhausted and the outer loop should try the next provider.
pub(super) async fn run_retry_loop(
    ctx: CommonCtx<'_>,
    input: &super::super::super::request_context::RequestContext,
    abort_guard: &mut super::super::super::abort_guard::RequestAbortGuard,
    prepared: &mut PreparedProvider,
    attempts: &mut Vec<FailoverAttempt>,
    failed_provider_ids: &mut HashSet<i64>,
    last_error_category: &mut Option<&'static str>,
    last_error_code: &mut Option<&'static str>,
    circuit_snapshot: &mut crate::circuit_breaker::CircuitSnapshot,
) -> Option<Response> {
    let mut retry_state = RetryLoopState::new();

    for retry_index in 1..=prepared.provider_max_attempts {
        let attempt_index = attempts.len().saturating_add(1) as u32;

        let send_outcome = attempt_executor::execute_attempt(
            ctx, input, abort_guard, prepared, &mut retry_state,
            retry_index, attempt_index, attempts, failed_provider_ids,
            last_error_category, last_error_code, circuit_snapshot,
        )
        .await;

        let ctrl = dispatch_outcome(
            ctx, input, abort_guard, prepared, &mut retry_state,
            retry_index, attempt_index, send_outcome, attempts,
            failed_provider_ids, last_error_category, last_error_code,
            circuit_snapshot,
        )
        .await;

        match ctrl {
            LoopControl::ContinueRetry => continue,
            LoopControl::BreakRetry => break,
            LoopControl::Return(resp) => return Some(resp),
        }
    }

    None
}

/// Dispatch one attempt outcome to the appropriate handler and return
/// a `LoopControl` for the retry loop.
#[allow(clippy::too_many_arguments)]
async fn dispatch_outcome(
    ctx: CommonCtx<'_>,
    input: &super::super::super::request_context::RequestContext,
    abort_guard: &mut super::super::super::abort_guard::RequestAbortGuard,
    prepared: &mut PreparedProvider,
    retry_state: &mut RetryLoopState,
    retry_index: u32,
    attempt_index: u32,
    send_outcome: AttemptSendOutcome,
    attempts: &mut Vec<FailoverAttempt>,
    failed_provider_ids: &mut HashSet<i64>,
    last_error_category: &mut Option<&'static str>,
    last_error_code: &mut Option<&'static str>,
    circuit_snapshot: &mut crate::circuit_breaker::CircuitSnapshot,
) -> LoopControl {
    match send_outcome {
        AttemptSendOutcome::UrlBuildFailed(ctrl) => ctrl,
        AttemptSendOutcome::OAuthInjectFailed => LoopControl::BreakRetry,
        AttemptSendOutcome::Response(resp) => {
            response_router::route_response(
                ctx, input, abort_guard, prepared, retry_state,
                retry_index, attempt_index, resp, attempts,
                failed_provider_ids, last_error_category,
                last_error_code, circuit_snapshot,
            )
            .await
        }
        AttemptSendOutcome::Timeout => {
            let (attempt_ctx, provider_ctx) =
                build_error_contexts(input, prepared, attempt_index, retry_index);
            let loop_state = LoopState::new(
                attempts, failed_provider_ids, last_error_category,
                last_error_code, circuit_snapshot, abort_guard,
            );
            send_timeout::handle_timeout(ctx, provider_ctx, attempt_ctx, loop_state).await
        }
        AttemptSendOutcome::ReqwestError(err) => {
            let (attempt_ctx, provider_ctx) =
                build_error_contexts(input, prepared, attempt_index, retry_index);
            let loop_state = LoopState::new(
                attempts, failed_provider_ids, last_error_category,
                last_error_code, circuit_snapshot, abort_guard,
            );
            upstream_error::handle_reqwest_error(
                ctx, provider_ctx, attempt_ctx, loop_state, err,
            )
            .await
        }
    }
}

/// Build `AttemptCtx` and `ProviderCtx` for error-path handling (timeout / reqwest error).
fn build_error_contexts<'a>(
    input: &super::super::super::request_context::RequestContext,
    prepared: &'a PreparedProvider,
    attempt_index: u32,
    retry_index: u32,
) -> (AttemptCtx<'a>, ProviderCtx<'a>) {
    let attempt_started_ms = input.started.elapsed().as_millis();
    let attempt_started = Instant::now();
    let attempt_ctx = AttemptCtx {
        attempt_index,
        retry_index,
        attempt_started_ms,
        attempt_started,
        circuit_before: &prepared.circuit_snapshot,
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
    (attempt_ctx, provider_ctx)
}
