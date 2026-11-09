//! Recovery policy for model generation only. Never wrap business tool execution in this policy.
use crate::error::{AppError, AppResult};
use serde_json::{Value, json};
use std::{future::Future, time::Duration};

/// Safe, structural failure evidence. Never retain provider prose, input, output,
/// opaque reasoning, credentials or account identifiers in an error/event.
#[derive(Clone, Debug, serde::Serialize)]
pub struct FailureDiagnostic {
    source: &'static str,
    http_status: Option<u16>,
    event: &'static str,
    classification: &'static str,
    code_sha256: Option<String>,
    type_sha256: Option<String>,
    message_sha256: Option<String>,
    message_bytes: Option<usize>,
    usage: Value,
}
impl FailureDiagnostic {
    pub fn usage(&self) -> &Value {
        &self.usage
    }
}
fn error_body(value: &Value) -> &Value {
    value
        .pointer("/response/error")
        .filter(|v| v.is_object())
        .or_else(|| value.get("error").filter(|v| v.is_object()))
        .unwrap_or(value)
}
pub fn failure_diagnostic(value: &Value, status: Option<u16>) -> FailureDiagnostic {
    let error = error_body(value);
    let hash = |v: &Value| v.as_str().map(|s| crate::workspace::digest(s.as_bytes()));
    let usage = &value["response"]["usage"];
    let usage = if let Some(total) = usage["total_tokens"].as_u64() {
        json!({"total_tokens":total,"input_tokens":usage["input_tokens"].as_u64(),"output_tokens":usage["output_tokens"].as_u64()})
    } else {
        Value::Null
    };
    FailureDiagnostic {
        source: if status.is_some() { "http" } else { "sse" },
        http_status: status,
        event: match value["type"].as_str() {
            Some("response.failed") => "response.failed",
            Some("response.incomplete") => "response.incomplete",
            Some("response.completed") => "response.completed",
            Some("error") => "error",
            _ => "unknown",
        },
        classification: stream_failure(value),
        code_sha256: hash(&error["code"]),
        type_sha256: hash(&error["type"]),
        message_sha256: hash(&error["message"]),
        message_bytes: error["message"].as_str().map(str::len),
        usage,
    }
}

pub fn inference_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(300))
        .timeout(Duration::from_secs(900))
        .build()
}

pub fn transport_error(error: reqwest::Error) -> AppError {
    AppError::bad(if error.is_timeout() {
        "inference_timeout"
    } else if error.is_connect() {
        "provider_unavailable"
    } else {
        "inference_stream_interrupted"
    })
}

pub async fn rejected(response: reqwest::Response) -> AppError {
    let status = response.status().as_u16();
    // Inspect only a bounded error body and expose a fixed code, never provider text.
    let mut stream = response.bytes_stream();
    use futures_util::StreamExt;
    let mut body = Vec::new();
    while let Some(Ok(chunk)) = stream.next().await {
        if body.len() + chunk.len() > 16384 {
            break;
        }
        body.extend_from_slice(&chunk);
    }
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    if status == 413
        || matches!(
            value["error"]["code"].as_str(),
            Some("context_length_exceeded" | "input_too_long")
        )
    {
        return AppError::bad("inference_context_overflow")
            .with_diagnostic(failure_diagnostic(&value, Some(status)));
    }
    let permanent_quota = matches!(
        value["error"]["code"].as_str(),
        Some("insufficient_quota" | "billing_hard_limit_reached")
    );
    let code = stream_failure(&value);
    let error = if code != "inference_failed" {
        AppError::bad(code)
    } else {
        status_error(status, permanent_quota)
    };
    error.with_diagnostic(failure_diagnostic(&value, Some(status)))
}

fn status_error(status: u16, permanent_quota: bool) -> AppError {
    AppError::bad(match status {
        429 if permanent_quota => "provider_quota_exhausted",
        429 => "provider_rate_limited",
        408 | 500 | 502 | 503 | 504 => "provider_temporarily_unavailable",
        401 | 403 => "provider_authentication_rejected",
        _ => "inference_rejected",
    })
}

/// SSE failures need the same classification as HTTP rejections. Preserve only
/// known codes; provider prose can contain request/account material.
pub fn stream_failure(value: &serde_json::Value) -> &'static str {
    if value["response"]["incomplete_details"]["reason"] == "max_output_tokens" {
        return "inference_output_truncated";
    }
    if value["response"]["incomplete_details"]["reason"] == "content_filter" {
        return "inference_content_filtered";
    }
    let error = error_body(value);
    let code = error["code"].as_str().or_else(|| error["type"].as_str());
    match code {
        Some("context_length_exceeded" | "input_too_long") => "inference_context_overflow",
        Some("invalid_encrypted_content" | "invalid_encrypted_reasoning") => {
            "inference_context_invalid"
        }
        Some("content_filter" | "content_policy_violation") => "inference_content_filtered",
        Some("server_error" | "internal_error" | "overloaded" | "service_unavailable") => {
            "provider_temporarily_unavailable"
        }
        Some("rate_limit_exceeded" | "rate_limit_error") => "provider_rate_limited",
        Some("insufficient_quota" | "billing_hard_limit_reached" | "usage_limit_reached") => {
            "provider_quota_exhausted"
        }
        Some(
            "invalid_api_key" | "invalid_token" | "authentication_error" | "permission_denied",
        ) => "provider_authentication_rejected",
        Some(
            "invalid_request_error"
            | "invalid_request"
            | "unsupported_parameter"
            | "model_not_found",
        ) => "inference_rejected",
        _ => "inference_failed",
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Recovery {
    Retry(Duration),
    Rebuild,
    Stop,
}

pub fn context_failure(code: &str) -> bool {
    matches!(
        code,
        "inference_context_overflow"
            | "inference_input_budget_exceeded"
            | "inference_output_truncated"
            | "inference_context_invalid"
            | "inference_generation_recovery_exhausted"
    )
}
/// Only call after uncommitted generation fails, with no pending/batched tool actions.
/// A bounded context repair changes the next request instead of replaying a bad one.
pub fn recovery(code: &str, failures: u32, repairs: u32) -> Recovery {
    if context_failure(code) || (code == "inference_failed" && failures > 3) {
        return if repairs < 3 {
            Recovery::Rebuild
        } else {
            Recovery::Stop
        };
    }
    retry_delay(code, failures)
        .map(Recovery::Retry)
        .unwrap_or(Recovery::Stop)
}

pub fn retry_delay(code: &str, failures: u32) -> Option<Duration> {
    // A terminal generic generation failure has no committed model decision or
    // business effect. Give it a small recovery opportunity, not endless retry.
    if code == "inference_failed" {
        return (failures <= 3).then(|| Duration::from_secs(2u64.pow(failures.min(3))));
    }
    if !matches!(
        code,
        "inference_timeout"
            | "provider_unavailable"
            | "inference_stream_interrupted"
            | "inference_missing_completion"
            | "provider_rate_limited"
            | "provider_temporarily_unavailable"
    ) {
        return None;
    }
    Some(Duration::from_secs(
        (2u64.saturating_pow(failures.min(6))).clamp(2, 60),
    ))
}

/// Poll controls while cooling down; no next inference may start after pause/cancel/revocation.
pub async fn wait<F, Fut>(delay: Duration, mut boundary: F) -> AppResult<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = AppResult<()>>,
{
    let until = tokio::time::Instant::now() + delay;
    loop {
        boundary().await?;
        let remaining = until.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        tokio::time::sleep(remaining.min(Duration::from_secs(1))).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_only_uncommitted_transient_generation() {
        for code in [
            "inference_timeout",
            "inference_missing_completion",
            "inference_stream_interrupted",
            "provider_rate_limited",
            "provider_temporarily_unavailable",
        ] {
            assert_eq!(retry_delay(code, 1), Some(Duration::from_secs(2)));
            assert_eq!(retry_delay(code, 1000), Some(Duration::from_secs(60)));
        }
        for code in [
            "provider_authentication_rejected",
            "provider_quota_exhausted",
            "inference_stream_invalid",
            "database_failed",
            "worker_timeout",
            "inference_rejected",
            "inference_usage_missing",
        ] {
            assert!(retry_delay(code, 1).is_none(), "{code}");
        }
        assert!(retry_delay(status_error(429, true).code, 1).is_none());
        assert!(retry_delay(status_error(503, false).code, 1).is_some());
        assert!(retry_delay(status_error(403, false).code, 1).is_none());
        assert!(retry_delay("inference_failed", 1).is_some());
        assert!(retry_delay("inference_failed", 3).is_some());
        assert!(retry_delay("inference_failed", 4).is_none());
    }
    #[test]
    fn stream_errors_keep_safe_causes_and_do_not_retry_quota_or_auth() {
        for (code, expected) in [
            ("server_error", "provider_temporarily_unavailable"),
            ("rate_limit_exceeded", "provider_rate_limited"),
            ("insufficient_quota", "provider_quota_exhausted"),
            ("invalid_token", "provider_authentication_rejected"),
            ("context_length_exceeded", "inference_context_overflow"),
            ("invalid_encrypted_content", "inference_context_invalid"),
            ("content_policy_violation", "inference_content_filtered"),
            ("private-unknown-value", "inference_failed"),
        ] {
            for event in [
                serde_json::json!({"response":{"error":{"code":code}}}),
                serde_json::json!({"error":{"code":code}}),
                serde_json::json!({"code":code}),
            ] {
                assert_eq!(stream_failure(&event), expected);
            }
        }
    }
    #[test]
    fn repeated_generic_failure_repairs_context_instead_of_replaying_forever() {
        assert!(matches!(
            recovery("inference_failed", 3, 0),
            Recovery::Retry(_)
        ));
        assert_eq!(recovery("inference_failed", 4, 0), Recovery::Rebuild);
        assert_eq!(recovery("inference_failed", 4, 2), Recovery::Rebuild);
        assert_eq!(recovery("inference_failed", 4, 3), Recovery::Stop);
        assert_eq!(
            recovery("inference_context_invalid", 1, 0),
            Recovery::Rebuild
        );
        for code in [
            "provider_quota_exhausted",
            "provider_authentication_rejected",
            "inference_rejected",
            "inference_content_filtered",
            "worker_timeout",
        ] {
            assert_eq!(recovery(code, 4, 0), Recovery::Stop);
        }
    }
    #[test]
    fn diagnostics_preserve_usage_and_structure_without_provider_material() {
        let event = json!({"type":"response.failed","response":{"error":{"code":"private-code-secret","type":"private-type-secret","message":"credential and customer prose"},"usage":{"total_tokens":42,"input_tokens":40,"output_tokens":2,"private":"secret"}}});
        let d = failure_diagnostic(&event, None);
        assert_eq!(d.usage()["total_tokens"], 42);
        let encoded = serde_json::to_string(&d).unwrap();
        for secret in [
            "private-code-secret",
            "private-type-secret",
            "credential",
            "customer",
            "prose",
        ] {
            assert!(!encoded.contains(secret));
        }
        assert_eq!(d.event, "response.failed");
        assert_eq!(d.classification, "inference_failed");
        assert_eq!(d.message_bytes, Some(29));
        assert!(d.message_sha256.is_some());
        assert_eq!(
            stream_failure(&json!({"error":{"type":"server_error"}})),
            "provider_temporarily_unavailable"
        );
        assert_eq!(
            stream_failure(&json!({"response":{"incomplete_details":{"reason":"content_filter"}}})),
            "inference_content_filtered"
        );
    }
    #[tokio::test]
    async fn pause_prevents_another_attempt_during_backoff() {
        let result = wait(Duration::from_secs(60), || async {
            Err(AppError::bad("paused"))
        })
        .await;
        assert_eq!(result.unwrap_err().code, "paused");
    }
}
