use crate::{
    auth::{Callback, OAuthService, StartLogin},
    bootstrap::{RuntimeKeys, secret_hash},
    config::Config,
    error::{AppError, AppResult},
    policy::{self, Capability, Claims, Role, ToolRequest},
    store::{VaultStore, audit},
    unix_time,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    middleware,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::{sync::Arc, time::Duration};
use subtle::ConstantTimeEq;
use uuid::Uuid;

pub struct AppState {
    pub pool: PgPool,
    pub vault: VaultStore,
    pub oauth: OAuthService,
    keys: RuntimeKeys,
    signing_key: Vec<u8>,
}
type Shared = Arc<AppState>;

fn bearer(headers: &HeaderMap) -> AppResult<&str> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|s| !s.is_empty() && s.len() < 8192)
        .ok_or_else(AppError::unauthorized)?;
    Ok(token)
}
fn operator(state: &AppState, headers: &HeaderMap) -> AppResult<()> {
    if !bool::from(
        secret_hash(bearer(headers)?)
            .as_bytes()
            .ct_eq(state.keys.operator_hash.as_bytes()),
    ) {
        return Err(AppError::unauthorized());
    }
    Ok(())
}
fn actor(state: &AppState, headers: &HeaderMap) -> AppResult<Claims> {
    policy::verify(&state.signing_key, bearer(headers)?)
}

pub async fn run(config: Config) -> anyhow::Result<()> {
    config.validate()?;
    let token = tokio::fs::read_to_string(&config.vault_token_file).await?;
    let vault = VaultStore::new(config.vault_url.clone(), token.trim().into())?;
    let keys = vault
        .get::<RuntimeKeys>("runtime/keys")
        .await?
        .ok_or_else(|| anyhow::anyhow!("bootstrap required"))?
        .value;
    let identity = vault
        .get::<Value>("runtime/identity")
        .await?
        .ok_or_else(|| anyhow::anyhow!("bootstrap required"))?
        .value;
    let host_id = identity["host_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("host identity missing"))?
        .to_owned();
    let signing_key = URL_SAFE_NO_PAD.decode(&keys.role_signing_key)?;
    anyhow::ensure!(signing_key.len() >= 32, "invalid role signing key");
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect_with(config.database_options().await?)
        .await?;
    sqlx::migrate!("../../migrations").run(&pool).await?;
    let oauth = OAuthService::new(vault.clone(), pool.clone(), host_id, config.callback_uri)?;
    let state = Arc::new(AppState {
        pool,
        vault: vault.clone(),
        oauth,
        keys,
        signing_key,
    });
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1200));
        loop {
            tick.tick().await;
            if vault.renew().await.is_err() {
                tracing::warn!(event = "vault_renew_failed");
            }
        }
    });
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(&config.listen).await?;
    tracing::info!(
        event = "server_started",
        version = env!("CARGO_PKG_VERSION")
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("signal handler");
        tokio::select! {_ = tokio::signal::ctrl_c()=>{},_ = terminate.recv()=>{}}
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/auth/callback", get(callback))
        .route("/v1/auth/start", post(start_login))
        .route("/v1/auth/attempts/{id}", get(attempt))
        .route("/v1/auth/connections", get(connections))
        .route("/v1/auth/connections/{id}/refresh", post(refresh))
        .route("/v1/auth/connections/{id}/models", get(models))
        .route("/v1/auth/connections/{id}/probe", post(probe))
        .route("/v1/operator/tasks", post(create_task))
        .route("/v1/operator/role-tokens", post(issue_token))
        .route("/v1/operator/policy", get(matrix))
        .route("/v1/operator/audit", get(operator_audit))
        .route("/v1/whoami", get(whoami))
        .route("/v1/permissions/check", post(check))
        .route("/v1/tools/invoke", post(invoke))
        .layer(DefaultBodyLimit::max(65536))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}
async fn security_headers(request: axum::extract::Request, next: middleware::Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert("referrer-policy", "no-referrer".parse().unwrap());
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("content-security-policy","default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'".parse().unwrap());
    response
}
async fn home() -> Html<&'static str> {
    Html(include_str!("home.html"))
}
async fn health() -> Json<Value> {
    Json(json!({"status":"ok","version":env!("CARGO_PKG_VERSION")}))
}
async fn ready(State(state): State<Shared>) -> AppResult<Json<Value>> {
    sqlx::query("SELECT 1")
        .execute(&state.pool)
        .await
        .map_err(|_| AppError::new(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))?;
    if !state.vault.healthy().await {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "vault_unavailable",
        ));
    }
    Ok(Json(json!({"status":"ready","database":true,"vault":true})))
}
async fn start_login(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(req): Json<StartLogin>,
) -> AppResult<Json<crate::auth::LoginStart>> {
    operator(&s, &h)?;
    Ok(Json(s.oauth.start(req).await?))
}
async fn attempt(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> AppResult<Json<crate::auth::AttemptStatus>> {
    operator(&s, &h)?;
    Ok(Json(s.oauth.attempt(id).await?))
}
async fn callback(State(s): State<Shared>, Query(req): Query<Callback>) -> Response {
    match s.oauth.callback(req).await {
        Ok(_)=>Html("<html lang=\"ko\"><meta charset=\"utf-8\"><title>Echelon Nano</title><h1>ChatGPT 연결 완료</h1><p>이 창을 닫고 에셜론 나노 검증으로 돌아가세요.</p></html>").into_response(),
        Err(error)=>(error.status,Html("<html lang=\"ko\"><meta charset=\"utf-8\"><title>Echelon Nano</title><h1>연결을 완료하지 못했습니다</h1><p>에셜론 나노에서 상태를 확인하고 새 로그인 시도를 시작하세요.</p></html>")).into_response(),
    }
}
async fn connections(State(s): State<Shared>, h: HeaderMap) -> AppResult<Json<Value>> {
    operator(&s, &h)?;
    Ok(Json(s.oauth.connections().await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Refresh {
    expected_generation: u64,
}
async fn refresh(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<Refresh>,
) -> AppResult<Json<Value>> {
    operator(&s, &h)?;
    Ok(Json(
        s.oauth
            .refresh(&id, Some(req.expected_generation), true)
            .await?,
    ))
}
async fn models(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    operator(&s, &h)?;
    Ok(Json(s.oauth.models(&id).await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Probe {
    model: String,
}
async fn probe(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<Probe>,
) -> AppResult<Json<Value>> {
    operator(&s, &h)?;
    Ok(Json(s.oauth.probe(&id, &req.model).await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateTask {
    tenant_id: Uuid,
    tenant_name: String,
    goal: String,
}
async fn create_task(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(req): Json<CreateTask>,
) -> AppResult<Json<Value>> {
    operator(&s, &h)?;
    if req.tenant_name.len() > 200 || req.goal.len() > 4000 {
        return Err(AppError::bad("input_too_large"));
    }
    let id = Uuid::new_v4();
    let mut tx = s
        .pool
        .begin()
        .await
        .map_err(|_| AppError::internal("database_unavailable"))?;
    sqlx::query("INSERT INTO tenants(id,name) VALUES($1,$2) ON CONFLICT(id) DO NOTHING")
        .bind(req.tenant_id)
        .bind(req.tenant_name)
        .execute(&mut *tx)
        .await
        .map_err(|_| AppError::internal("task_create_failed"))?;
    sqlx::query("INSERT INTO tasks(id,tenant_id,goal) VALUES($1,$2,$3)")
        .bind(id)
        .bind(req.tenant_id)
        .bind(req.goal)
        .execute(&mut *tx)
        .await
        .map_err(|_| AppError::internal("task_create_failed"))?;
    tx.commit()
        .await
        .map_err(|_| AppError::internal("task_create_failed"))?;
    Ok(Json(json!({"task_id":id,"tenant_id":req.tenant_id})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IssueToken {
    role: Role,
    tenant_id: Uuid,
    task_id: Uuid,
    work_order_id: Option<Uuid>,
    #[serde(default)]
    targets: Vec<String>,
    #[serde(default = "default_ttl")]
    ttl_seconds: u64,
}
fn default_ttl() -> u64 {
    900
}
async fn issue_token(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(req): Json<IssueToken>,
) -> AppResult<Json<Value>> {
    operator(&s, &h)?;
    if req.ttl_seconds == 0
        || req.ttl_seconds > 900
        || req.targets.len() > 16
        || req.targets.iter().any(|s| s.len() > 500)
    {
        return Err(AppError::bad("invalid_grant"));
    }
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tasks WHERE id=$1 AND tenant_id=$2)")
            .bind(req.task_id)
            .bind(req.tenant_id)
            .fetch_one(&s.pool)
            .await
            .map_err(|_| AppError::internal("database_unavailable"))?;
    if !exists {
        return Err(AppError::bad("task_unknown"));
    }
    if req.role == Role::Agent {
        let id = req
            .work_order_id
            .ok_or_else(|| AppError::bad("work_order_required"))?;
        let target:Option<String>=sqlx::query_scalar("SELECT target FROM work_orders WHERE id=$1 AND task_id=$2 AND tenant_id=$3 AND status='ready'")
            .bind(id).bind(req.task_id).bind(req.tenant_id).fetch_optional(&s.pool).await.map_err(|_|AppError::internal("database_unavailable"))?;
        if target.is_none() || req.targets.iter().any(|s| Some(s) != target.as_ref()) {
            return Err(AppError::bad("work_order_scope_invalid"));
        }
    } else if req.work_order_id.is_some() || !req.targets.is_empty() {
        return Err(AppError::bad("invalid_grant"));
    }
    let claims = Claims {
        iss: "echelon-nano".into(),
        aud: "echelon-core".into(),
        sub: format!("{:?}", req.role).to_lowercase(),
        role: req.role,
        tenant_id: req.tenant_id,
        task_id: req.task_id,
        work_order_id: req.work_order_id,
        targets: req.targets,
        iat: unix_time(),
        exp: unix_time() + req.ttl_seconds,
        jti: Uuid::new_v4(),
    };
    Ok(Json(
        json!({"token":policy::sign(&s.signing_key,&claims)?,"expires_at":claims.exp}),
    ))
}
async fn whoami(State(s): State<Shared>, h: HeaderMap) -> AppResult<Json<Value>> {
    let a = actor(&s, &h)?;
    Ok(Json(
        json!({"role":a.role,"tenant_id":a.tenant_id,"task_id":a.task_id,"work_order_id":a.work_order_id,"targets":a.targets}),
    ))
}
async fn matrix(State(s): State<Shared>, h: HeaderMap) -> AppResult<Json<Value>> {
    operator(&s, &h)?;
    Ok(Json(
        json!({"roles":([Role::Analyst,Role::Handler,Role::Agent,Role::Master].map(|role|json!({
            "role":role,"capabilities":policy::ALL.into_iter().filter(|action|policy::role_allows(role,*action)).collect::<Vec<_>>()
        })))}),
    ))
}
async fn check(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(req): Json<ToolRequest>,
) -> AppResult<Json<Value>> {
    let a = actor(&s, &h)?;
    Ok(Json(match policy::authorize(&a, &req) {
        Ok(()) => json!({"allowed":true}),
        Err(e) => json!({"allowed":false,"code":e.code}),
    }))
}
async fn invoke(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(req): Json<ToolRequest>,
) -> AppResult<Json<Value>> {
    let a = actor(&s, &h)?;
    let role = format!("{:?}", a.role).to_lowercase();
    let action =
        serde_json::to_value(req.tool).map_err(|_| AppError::internal("capability_invalid"))?;
    if let Err(error) = policy::authorize(&a, &req) {
        audit(
            &s.pool,
            Some(a.tenant_id),
            Some(a.task_id),
            &role,
            action.as_str().unwrap_or("unknown"),
            "denied",
            json!({"code":error.code}),
        )
        .await?;
        return Err(error);
    }
    if a.role == Role::Agent {
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM work_orders WHERE id=$1 AND tenant_id=$2 AND task_id=$3 AND status='ready')")
            .bind(a.work_order_id).bind(a.tenant_id).bind(a.task_id).fetch_one(&s.pool).await.map_err(|_|AppError::internal("database_unavailable"))?;
        if !valid {
            return Err(AppError::forbidden("work_order_inactive"));
        }
    }
    let result = dispatch(&s, &a, &req).await;
    let outcome = match &result {
        Ok(_) => "completed",
        Err(e) if e.status == StatusCode::NOT_IMPLEMENTED => "not_implemented",
        _ => "failed",
    };
    audit(
        &s.pool,
        Some(a.tenant_id),
        Some(a.task_id),
        &role,
        action.as_str().unwrap_or("unknown"),
        outcome,
        json!({}),
    )
    .await?;
    result.map(Json)
}
async fn dispatch(s: &AppState, a: &Claims, req: &ToolRequest) -> AppResult<Value> {
    match req.tool {
        Capability::KnowledgeRead => {
            let rows=sqlx::query("SELECT id,category,title,version,truth_state,outcome FROM knowledge_items WHERE tenant_id=$1 ORDER BY created_at LIMIT 30")
                .bind(a.tenant_id).fetch_all(&s.pool).await.map_err(|_|AppError::internal("knowledge_read_failed"))?;
            Ok(
                json!({"items":rows.into_iter().map(|r|json!({"id":r.get::<Uuid,_>("id"),
                "category":r.get::<String,_>("category"),"title":r.get::<String,_>("title"),
                "version":r.get::<i32,_>("version"),"truth_state":r.get::<String,_>("truth_state"),
                "outcome":r.get::<String,_>("outcome")})).collect::<Vec<_>>()}),
            )
        }
        Capability::TaskPlan => {
            sqlx::query("UPDATE tasks SET plan=$1,status='ready' WHERE id=$2 AND tenant_id=$3")
                .bind(&req.input)
                .bind(a.task_id)
                .bind(a.tenant_id)
                .execute(&s.pool)
                .await
                .map_err(|_| AppError::internal("task_plan_failed"))?;
            Ok(json!({"planned":true}))
        }
        Capability::AgentDelegate => {
            let target = req.input["target"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 500)
                .ok_or_else(|| AppError::bad("target_required"))?;
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO work_orders(id,tenant_id,task_id,target) VALUES($1,$2,$3,$4)")
                .bind(id)
                .bind(a.tenant_id)
                .bind(a.task_id)
                .bind(target)
                .execute(&s.pool)
                .await
                .map_err(|_| AppError::internal("delegation_failed"))?;
            Ok(json!({"work_order_id":id,"status":"ready"}))
        }
        Capability::TaskObserve => {
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO observations(id,tenant_id,task_id,work_order_id,content) VALUES($1,$2,$3,$4,$5)")
                .bind(id).bind(a.tenant_id).bind(a.task_id).bind(a.work_order_id).bind(&req.input)
                .execute(&s.pool).await.map_err(|_|AppError::internal("observation_failed"))?;
            Ok(json!({"observation_id":id}))
        }
        Capability::AuditRead => {
            let rows=sqlx::query("SELECT seq,actor_role,action,outcome FROM audit_records WHERE tenant_id=$1 ORDER BY seq DESC LIMIT 50")
                .bind(a.tenant_id).fetch_all(&s.pool).await.map_err(|_|AppError::internal("audit_read_failed"))?;
            Ok(
                json!({"events":rows.into_iter().map(|r|json!({"seq":r.get::<i64,_>("seq"),
                "actor":r.get::<String,_>("actor_role"),"action":r.get::<String,_>("action"),"outcome":r.get::<String,_>("outcome")})).collect::<Vec<_>>()}),
            )
        }
        _ => Err(AppError::new(
            StatusCode::NOT_IMPLEMENTED,
            "adapter_not_implemented",
        )),
    }
}
async fn operator_audit(State(s): State<Shared>, h: HeaderMap) -> AppResult<Json<Value>> {
    operator(&s, &h)?;
    let rows=sqlx::query("SELECT action,outcome,count(*) AS count FROM audit_records GROUP BY action,outcome ORDER BY action,outcome")
        .fetch_all(&s.pool).await.map_err(|_|AppError::internal("database_unavailable"))?;
    Ok(Json(
        json!({"counts":rows.into_iter().map(|r|json!({"action":r.get::<String,_>("action"),
        "outcome":r.get::<String,_>("outcome"),"count":r.get::<i64,_>("count")})).collect::<Vec<_>>()}),
    ))
}
