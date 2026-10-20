use crate::{
    bootstrap::{random_secret, secret_hash},
    error::{AppError, AppResult},
    server::AppState,
    workspace::{bad, db},
};
use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use sqlx::Row;
use std::sync::Arc;
use subtle::ConstantTimeEq;

pub fn origins(extra: &str) -> anyhow::Result<Vec<String>> {
    let mut origins = vec![
        "http://127.0.0.1:18431".into(),
        "http://localhost:18431".into(),
    ];
    for value in extra.split(',').map(str::trim).filter(|v| !v.is_empty()) {
        let uri = url::Url::parse(value)?;
        anyhow::ensure!(
            matches!(uri.scheme(), "http" | "https")
                && uri.host_str().is_some_and(|host| !host.contains('*'))
                && uri.path() == "/"
                && uri.query().is_none()
                && uri.fragment().is_none()
                && uri.username().is_empty()
                && uri.password().is_none(),
            "invalid UI origin"
        );
        let origin = uri.origin().ascii_serialization();
        if !origins.contains(&origin) {
            origins.push(origin);
        }
    }
    Ok(origins)
}
fn allowed_host(origins: &[String], headers: &HeaderMap) -> bool {
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    origins
        .iter()
        .any(|origin| origin.split_once("://").map(|(_, authority)| authority) == host)
}
fn allowed_origin(origins: &[String], headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    origin.to_str().ok().is_some_and(|value| {
        origins.iter().any(|known| known == value)
            && value.split_once("://").map(|(_, authority)| authority)
                == headers.get(header::HOST).and_then(|v| v.to_str().ok())
    })
}
pub fn cookie(h: &HeaderMap) -> Option<&str> {
    h.get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|p| p.trim().strip_prefix("echelon_session="))
        .filter(|s| s.len() <= 200)
}
pub async fn session(s: &AppState, h: &HeaderMap) -> AppResult<Value> {
    let token = cookie(h).ok_or_else(AppError::unauthorized)?;
    let row=sqlx::query("SELECT csrf,expires_at::text AS expires_at FROM ui_sessions WHERE token_hash=$1 AND expires_at>now()").bind(secret_hash(token)).fetch_optional(&s.pool).await.map_err(db)?.ok_or_else(AppError::unauthorized)?;
    Ok(
        json!({"authenticated":true,"csrf":row.get::<String,_>("csrf"),"expires_at":row.get::<String,_>("expires_at")}),
    )
}
pub async fn open_session(s: &AppState, h: &HeaderMap) -> AppResult<Response> {
    // The local beta deliberately has no operator login. The browser receives
    // a session automatically; role tokens and CSRF remain separate controls.
    if !allowed_host(&s.ui_origins, h)
        || !allowed_origin(&s.ui_origins, h)
        || h.get("sec-fetch-site").and_then(|v| v.to_str().ok()) == Some("cross-site")
    {
        return Err(AppError::forbidden("origin_denied"));
    }
    if let Ok(existing) = session(s, h).await {
        return Ok(Json(existing).into_response());
    }
    let token = random_secret();
    let csrf = random_secret();
    sqlx::query("INSERT INTO ui_sessions(token_hash,csrf,expires_at) VALUES($1,$2,now()+interval '12 hours')").bind(secret_hash(&token)).bind(&csrf).execute(&s.pool).await.map_err(db)?;
    let mut response = Json(json!({"authenticated":true,"csrf":csrf})).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        format!("echelon_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200")
            .parse()
            .map_err(|_| bad())?,
    );
    Ok(response)
}
pub async fn logout(s: &AppState, h: &HeaderMap) -> AppResult<Response> {
    if let Some(token) = cookie(h) {
        sqlx::query("DELETE FROM ui_sessions WHERE token_hash=$1")
            .bind(secret_hash(token))
            .execute(&s.pool)
            .await
            .map_err(db)?;
    }
    let mut response = Json(json!({"authenticated":false})).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        "echelon_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"
            .parse()
            .unwrap(),
    );
    Ok(response)
}
pub async fn csrf(State(s): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    let headers = request.headers().clone();
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let check = async {
        let h = &headers;
        if !allowed_origin(&s.ui_origins, h) {
            return Err(AppError::forbidden("origin_denied"));
        }
        if cookie(h).is_some()
            && !matches!(method, Method::GET | Method::HEAD)
            && !h.contains_key(header::AUTHORIZATION)
            && path != "/v1/ui/session"
        {
            let session = session(&s, h).await?;
            let expected = session["csrf"]
                .as_str()
                .ok_or_else(AppError::unauthorized)?;
            let actual = h
                .get("x-echelon-csrf")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            if !bool::from(
                secret_hash(expected)
                    .as_bytes()
                    .ct_eq(secret_hash(actual).as_bytes()),
            ) {
                return Err(AppError::new(StatusCode::FORBIDDEN, "csrf_denied"));
            }
        }
        Ok::<(), AppError>(())
    }
    .await;
    match check {
        Ok(()) => next.run(request).await,
        Err(e) => e.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_origins_are_exact_and_remain_same_origin() {
        let allowed = origins("http://192.0.2.10:18431/, http://192.0.2.10:18431").unwrap();
        assert_eq!(allowed.len(), 3);
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "192.0.2.10:18431".parse().unwrap());
        headers.insert(header::ORIGIN, "http://192.0.2.10:18431".parse().unwrap());
        assert!(allowed_host(&allowed, &headers));
        assert!(allowed_origin(&allowed, &headers));
        headers.insert(header::ORIGIN, "http://127.0.0.1:18431".parse().unwrap());
        assert!(!allowed_origin(&allowed, &headers));
        headers.insert(header::HOST, "foreign.example:18431".parse().unwrap());
        assert!(!allowed_host(&allowed, &headers));
        for invalid in [
            "https://*.example",
            "http://user:password@example.com",
            "http://example.com/path",
            "http://example.com?query=1",
            "ftp://example.com",
        ] {
            assert!(origins(invalid).is_err());
        }
    }
}
