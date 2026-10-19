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
    if !matches!(
        h.get(header::HOST).and_then(|v| v.to_str().ok()),
        Some("127.0.0.1:18431" | "localhost:18431")
    ) || h.get("sec-fetch-site").and_then(|v| v.to_str().ok()) == Some("cross-site")
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
        if let Some(origin) = h.get(header::ORIGIN)
            && !matches!(
                origin.to_str().ok(),
                Some("http://127.0.0.1:18431" | "http://localhost:18431")
            )
        {
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
