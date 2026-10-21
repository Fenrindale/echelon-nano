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

pub fn retry_delay(code: &str, failures: u32) -> Option<Duration> {
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
            "inference_failed",
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
