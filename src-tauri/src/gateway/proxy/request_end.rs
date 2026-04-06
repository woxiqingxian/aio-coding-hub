//! Usage: Shared helpers to emit request-end events and enqueue request logs consistently.

use super::logging::enqueue_request_log_with_backpressure;
use super::status_override;
use super::{spawn_enqueue_request_log_with_backpressure, RequestLogEnqueueArgs};
use crate::gateway::events::{emit_request_event, FailoverAttempt};
use crate::{db, request_logs};

pub(super) struct RequestEndDeps<'a> {
    pub(super) app: &'a tauri::AppHandle,
    pub(super) db: &'a db::Db,
    pub(super) log_tx: &'a tokio::sync::mpsc::Sender<request_logs::RequestLogInsert>,
}

impl<'a> RequestEndDeps<'a> {
    pub(super) fn new(
        app: &'a tauri::AppHandle,
        db: &'a db::Db,
        log_tx: &'a tokio::sync::mpsc::Sender<request_logs::RequestLogInsert>,
    ) -> Self {
        Self { app, db, log_tx }
    }
}

pub(super) struct RequestEndArgs<'a> {
    pub(super) deps: RequestEndDeps<'a>,
    pub(super) trace_id: &'a str,
    pub(super) cli_key: &'a str,
    pub(super) method: &'a str,
    pub(super) path: &'a str,
    pub(super) observe: bool,
    pub(super) query: Option<&'a str>,
    pub(super) excluded_from_stats: bool,
    pub(super) status: Option<u16>,
    pub(super) error_category: Option<&'static str>,
    pub(super) error_code: Option<&'static str>,
    pub(super) duration_ms: u128,
    pub(super) event_ttfb_ms: Option<u128>,
    pub(super) log_ttfb_ms: Option<u128>,
    pub(super) attempts: &'a [FailoverAttempt],
    pub(super) special_settings_json: Option<String>,
    pub(super) session_id: Option<String>,
    pub(super) requested_model: Option<String>,
    pub(super) created_at_ms: i64,
    pub(super) created_at: i64,
    pub(super) usage_metrics: Option<crate::usage::UsageMetrics>,
    pub(super) log_usage_metrics: Option<crate::usage::UsageMetrics>,
    pub(super) usage: Option<crate::usage::UsageExtract>,
}

struct PreparedRequestEnd<'a> {
    deps: RequestEndDeps<'a>,
    error_category: Option<&'static str>,
    event_ttfb_ms: Option<u128>,
    attempts: Vec<FailoverAttempt>,
    usage_metrics: Option<crate::usage::UsageMetrics>,
    log_args: RequestLogEnqueueArgs,
}

struct RequestEndPayloadParts {
    trace_id: String,
    cli_key: String,
    session_id: Option<String>,
    method: String,
    path: String,
    query: Option<String>,
    excluded_from_stats: bool,
    special_settings_json: Option<String>,
    status: Option<u16>,
    error_code: Option<&'static str>,
    duration_ms: u128,
    ttfb_ms: Option<u128>,
    attempts: Vec<FailoverAttempt>,
    attempts_json: Option<String>,
    requested_model: Option<String>,
    created_at_ms: i64,
    created_at: i64,
    usage_metrics: Option<crate::usage::UsageMetrics>,
    usage: Option<crate::usage::UsageExtract>,
    provider_chain_json: Option<String>,
    error_details_json: Option<String>,
}

fn serialize_attempts(attempts: &[FailoverAttempt]) -> String {
    if attempts.is_empty() {
        "[]".to_string()
    } else {
        serde_json::to_string(attempts).unwrap_or_else(|_| "[]".to_string())
    }
}

fn build_provider_chain_json(attempts: &[FailoverAttempt]) -> Option<String> {
    if attempts.is_empty() {
        return None;
    }
    let chain: Vec<serde_json::Value> = attempts
        .iter()
        .map(|a| {
            let mut obj = serde_json::Map::new();
            obj.insert("provider_id".into(), serde_json::json!(a.provider_id));
            obj.insert("provider_name".into(), serde_json::json!(a.provider_name));
            if let Some(status) = a.status {
                obj.insert("status".into(), serde_json::json!(status));
            }
            obj.insert("outcome".into(), serde_json::json!(a.outcome));
            if let Some(decision) = a.decision {
                obj.insert("decision".into(), serde_json::json!(decision));
            }
            if let Some(ref reason) = a.reason {
                obj.insert("reason".into(), serde_json::json!(reason));
            }
            if let Some(duration_ms) = a.attempt_duration_ms {
                obj.insert("duration_ms".into(), serde_json::json!(duration_ms));
            }
            serde_json::Value::Object(obj)
        })
        .collect();
    serde_json::to_string(&chain).ok()
}

fn build_error_details_json(
    error_code: Option<&str>,
    attempts: &[FailoverAttempt],
) -> Option<String> {
    error_code?;
    let last_attempt = attempts.last()?;
    let mut obj = serde_json::Map::new();
    if let Some(error_code) = last_attempt.error_code {
        obj.insert("error_code".into(), serde_json::json!(error_code));
    }
    if let Some(ref reason) = last_attempt.reason {
        obj.insert("reason".into(), serde_json::json!(reason));
    }
    if let Some(error_category) = last_attempt.error_category {
        obj.insert("error_category".into(), serde_json::json!(error_category));
    }
    if let Some(status) = last_attempt.status {
        obj.insert("upstream_status".into(), serde_json::json!(status));
    }
    if obj.is_empty() {
        return None;
    }
    serde_json::to_string(&serde_json::Value::Object(obj)).ok()
}

fn build_request_end_payload(
    parts: RequestEndPayloadParts,
) -> (RequestLogEnqueueArgs, Vec<FailoverAttempt>) {
    let RequestEndPayloadParts {
        trace_id,
        cli_key,
        session_id,
        method,
        path,
        query,
        excluded_from_stats,
        special_settings_json,
        status,
        error_code,
        duration_ms,
        ttfb_ms,
        attempts,
        attempts_json,
        requested_model,
        created_at_ms,
        created_at,
        usage_metrics,
        usage,
        provider_chain_json,
        error_details_json,
    } = parts;

    let provider_chain_json = provider_chain_json.or_else(|| build_provider_chain_json(&attempts));
    let error_details_json =
        error_details_json.or_else(|| build_error_details_json(error_code, &attempts));
    let attempts_json = attempts_json.unwrap_or_else(|| serialize_attempts(&attempts));
    let log_args = RequestLogEnqueueArgs {
        trace_id,
        cli_key,
        session_id,
        method,
        path,
        query,
        excluded_from_stats,
        special_settings_json,
        status,
        error_code,
        duration_ms,
        ttfb_ms,
        attempts_json,
        requested_model,
        created_at_ms,
        created_at,
        usage_metrics,
        usage,
        provider_chain_json,
        error_details_json,
    };

    (log_args, attempts)
}

impl RequestLogEnqueueArgs {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::gateway) fn from_proxy_request_end_parts(
        trace_id: &str,
        cli_key: &str,
        session_id: Option<String>,
        method: &str,
        path: &str,
        query: Option<&str>,
        excluded_from_stats: bool,
        special_settings_json: Option<String>,
        status: Option<u16>,
        error_code: Option<&'static str>,
        duration_ms: u128,
        ttfb_ms: Option<u128>,
        attempts: &[FailoverAttempt],
        requested_model: Option<String>,
        created_at_ms: i64,
        created_at: i64,
        usage_metrics: Option<crate::usage::UsageMetrics>,
        usage: Option<crate::usage::UsageExtract>,
    ) -> (Self, Vec<FailoverAttempt>) {
        let status = status_override::effective_status(status, error_code);
        let excluded_from_stats = excluded_from_stats
            || super::is_claude_count_tokens_request(cli_key, path)
            || status_override::is_client_abort(error_code);

        build_request_end_payload(RequestEndPayloadParts {
            trace_id: trace_id.to_string(),
            cli_key: cli_key.to_string(),
            session_id,
            method: method.to_string(),
            path: path.to_string(),
            query: query.map(str::to_string),
            excluded_from_stats,
            special_settings_json,
            status,
            error_code,
            duration_ms,
            ttfb_ms,
            attempts: attempts.to_vec(),
            attempts_json: None,
            requested_model,
            created_at_ms,
            created_at,
            usage_metrics,
            usage,
            provider_chain_json: None,
            error_details_json: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::gateway) fn from_stream_request_end_parts(
        trace_id: String,
        cli_key: String,
        session_id: Option<String>,
        method: String,
        path: String,
        query: Option<String>,
        excluded_from_stats: bool,
        special_settings_json: Option<String>,
        status: u16,
        error_code: Option<&'static str>,
        duration_ms: u128,
        ttfb_ms: Option<u128>,
        attempts: Vec<FailoverAttempt>,
        attempts_json: String,
        requested_model: Option<String>,
        created_at_ms: i64,
        created_at: i64,
        usage: Option<crate::usage::UsageExtract>,
    ) -> (Self, Vec<FailoverAttempt>) {
        build_request_end_payload(RequestEndPayloadParts {
            trace_id,
            cli_key,
            session_id,
            method,
            path,
            query,
            excluded_from_stats: excluded_from_stats
                || status_override::is_client_abort(error_code),
            special_settings_json,
            status: status_override::effective_status(Some(status), error_code),
            error_code,
            duration_ms,
            ttfb_ms,
            attempts,
            attempts_json: Some(attempts_json),
            requested_model,
            created_at_ms,
            created_at,
            usage_metrics: None,
            usage,
            provider_chain_json: None,
            error_details_json: None,
        })
    }

    pub(in crate::gateway) fn emit_gateway_request_event(
        &self,
        app: &tauri::AppHandle,
        error_category: Option<&'static str>,
        event_ttfb_ms: Option<u128>,
        attempts: Vec<FailoverAttempt>,
        usage_metrics: Option<crate::usage::UsageMetrics>,
    ) {
        emit_request_event(
            app,
            self.trace_id.clone(),
            self.cli_key.clone(),
            self.session_id.clone(),
            self.method.clone(),
            self.path.clone(),
            self.query.clone(),
            self.requested_model.clone(),
            self.status,
            error_category,
            self.error_code,
            self.duration_ms,
            event_ttfb_ms,
            attempts,
            usage_metrics,
        );
    }
}

fn prepare_request_end(args: RequestEndArgs<'_>) -> PreparedRequestEnd<'_> {
    let (log_args, attempts) = RequestLogEnqueueArgs::from_proxy_request_end_parts(
        args.trace_id,
        args.cli_key,
        args.session_id,
        args.method,
        args.path,
        args.query,
        args.excluded_from_stats,
        args.special_settings_json,
        args.status,
        args.error_code,
        args.duration_ms,
        args.log_ttfb_ms,
        args.attempts,
        args.requested_model,
        args.created_at_ms,
        args.created_at,
        args.log_usage_metrics,
        args.usage,
    );

    PreparedRequestEnd {
        deps: args.deps,
        error_category: args.error_category,
        event_ttfb_ms: args.event_ttfb_ms,
        attempts,
        usage_metrics: args.usage_metrics,
        log_args,
    }
}

pub(super) async fn emit_request_event_and_enqueue_request_log(args: RequestEndArgs<'_>) {
    // Disk log: request ended with error (failure path only).
    if let Some(error_code) = args.error_code {
        tracing::warn!(
            trace_id = %args.trace_id,
            error_code = error_code,
            cli_key = %args.cli_key,
            status = ?args.status,
            duration_ms = %args.duration_ms,
            "gateway request completed with error"
        );
    }

    if !args.observe {
        return;
    }

    let PreparedRequestEnd {
        deps,
        error_category,
        event_ttfb_ms,
        attempts,
        usage_metrics,
        log_args,
    } = prepare_request_end(args);

    log_args.emit_gateway_request_event(
        deps.app,
        error_category,
        event_ttfb_ms,
        attempts,
        usage_metrics,
    );

    enqueue_request_log_with_backpressure(deps.app, deps.db, deps.log_tx, log_args).await;
}

pub(super) fn emit_request_event_and_spawn_request_log(args: RequestEndArgs<'_>) {
    // Disk log: request ended with error (failure path only).
    if let Some(error_code) = args.error_code {
        tracing::warn!(
            trace_id = %args.trace_id,
            error_code = error_code,
            cli_key = %args.cli_key,
            status = ?args.status,
            duration_ms = %args.duration_ms,
            "gateway request completed with error"
        );
    }

    if !args.observe {
        return;
    }

    let PreparedRequestEnd {
        deps,
        error_category,
        event_ttfb_ms,
        attempts,
        usage_metrics,
        log_args,
    } = prepare_request_end(args);

    log_args.emit_gateway_request_event(
        deps.app,
        error_category,
        event_ttfb_ms,
        attempts,
        usage_metrics,
    );

    spawn_enqueue_request_log_with_backpressure(
        deps.app.clone(),
        deps.db.clone(),
        deps.log_tx.clone(),
        log_args,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::proxy::GatewayErrorCode;

    fn sample_attempt() -> FailoverAttempt {
        FailoverAttempt {
            provider_id: 7,
            provider_name: "provider".to_string(),
            base_url: "https://example.com".to_string(),
            outcome: "success".to_string(),
            status: Some(200),
            provider_index: Some(1),
            retry_index: Some(1),
            session_reuse: Some(false),
            error_category: None,
            error_code: None,
            decision: None,
            reason: None,
            selection_method: None,
            reason_code: None,
            attempt_started_ms: Some(1),
            attempt_duration_ms: Some(2),
            circuit_state_before: None,
            circuit_state_after: None,
            circuit_failure_count: None,
            circuit_failure_threshold: None,
        }
    }

    #[test]
    fn proxy_request_end_parts_apply_count_tokens_exclusion_and_serialize_attempts() {
        let attempts = vec![sample_attempt()];
        let expected_attempts_json = serde_json::to_string(&attempts).unwrap();

        let (log_args, cloned_attempts) = RequestLogEnqueueArgs::from_proxy_request_end_parts(
            "trace-1",
            "claude",
            Some("session-1".to_string()),
            "POST",
            "/v1/messages/count_tokens",
            Some("a=1"),
            false,
            Some("{\"type\":\"x\"}".to_string()),
            Some(200),
            None,
            345,
            Some(12),
            &attempts,
            Some("claude-3-7".to_string()),
            100,
            200,
            Some(crate::usage::UsageMetrics::default()),
            None,
        );

        assert!(log_args.excluded_from_stats);
        assert_eq!(log_args.status, Some(200));
        assert_eq!(log_args.query.as_deref(), Some("a=1"));
        assert_eq!(log_args.attempts_json, expected_attempts_json);
        assert_eq!(cloned_attempts.len(), 1);
        assert_eq!(cloned_attempts[0].provider_id, 7);
    }

    #[test]
    fn stream_request_end_parts_keep_attempts_json_and_apply_abort_override() {
        let attempts = vec![sample_attempt()];

        let (log_args, cloned_attempts) = RequestLogEnqueueArgs::from_stream_request_end_parts(
            "trace-2".to_string(),
            "codex".to_string(),
            None,
            "POST".to_string(),
            "/v1/responses".to_string(),
            None,
            false,
            Some("{\"type\":\"client_abort\"}".to_string()),
            200,
            Some(GatewayErrorCode::StreamAborted.as_str()),
            678,
            Some(34),
            attempts,
            "[{\"cached\":true}]".to_string(),
            Some("gpt-5".to_string()),
            300,
            400,
            None,
        );

        assert!(log_args.excluded_from_stats);
        assert_eq!(log_args.status, Some(499));
        assert_eq!(log_args.attempts_json, "[{\"cached\":true}]");
        assert_eq!(
            log_args.special_settings_json.as_deref(),
            Some("{\"type\":\"client_abort\"}")
        );
        assert!(log_args.usage_metrics.is_none());
        assert_eq!(cloned_attempts.len(), 1);
        assert_eq!(cloned_attempts[0].provider_id, 7);
    }

    #[test]
    fn should_not_observe_non_messages_claude_request_end() {
        assert!(!super::super::should_observe_request(
            "claude",
            "/v1/messages/count_tokens"
        ));
        assert!(!super::super::should_observe_request("claude", "/v1/other"));
        assert!(super::super::should_observe_request(
            "claude",
            "/v1/messages"
        ));
        assert!(super::super::should_observe_request(
            "codex",
            "/v1/messages/count_tokens"
        ));
    }
}
