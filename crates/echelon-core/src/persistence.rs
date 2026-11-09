//! Recovery policy for model generation only. Never wrap business tool execution in this policy.
use crate::error::{AppError, AppResult};
use std::{future::Future, time::Duration};

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
        return AppError::bad("inference_context_overflow");
    }
    let permanent_quota = matches!(
        value["error"]["code"].as_str(),
        Some("insufficient_quota" | "billing_hard_limit_reached")
    );
    status_error(status, permanent_quota)
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
    let code = value["response"]["error"]["code"]
        .as_str()
        .or_else(|| value["error"]["code"].as_str())
        .or_else(|| value["code"].as_str());
    match code {
        Some("context_length_exceeded" | "input_too_long") => "inference_context_overflow",
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
            ("private-unknown-value", "inference_failed"),
        ] {
            for event in [
                serde_json::json!({"response":{"error":{"code":code}}}),
                serde_json::json!({"error":{"code":code}}),
            ] {
                assert_eq!(stream_failure(&event), expected);
            }
        }
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
