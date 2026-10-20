use crate::{
    broker, execution, maintenance,
    policy::{Capability, Role, ToolRequest},
    protocol,
    server::{AppState, invoke_as, operator},
    store::audit,
    ui_auth,
    workspace::{self, TaskContract, bad, db, missing, nonempty},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, header},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::{StreamExt, stream::BoxStream};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::{convert::Infallible, sync::Arc, time::Duration};
use uuid::Uuid;

type Shared = Arc<AppState>;
pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/v1/ui/session", get(session).post(open_session))
        .route("/v1/ui/logout", post(logout))
        .route("/v1/operator/tenants", get(tenants).post(create_tenant))
        .route("/v1/operator/tenants/{id}/workspace", post(workspace_open))
        .route("/v1/operator/tasks", get(tasks))
        .route(
            "/v1/operator/tasks/launch",
            post(launch_goal).layer(DefaultBodyLimit::max(6 * 1048576)),
        )
        .route("/v1/operator/tasks/{id}/launch", post(launch_task))
        .route("/v1/operator/tasks/{id}", get(task).patch(update_task))
        .route("/v1/operator/tasks/{id}/inputs", post(task_input))
        .route("/v1/operator/artifacts", post(upload))
        .route("/v1/operator/artifacts/{id}", get(artifact))
        .route("/v1/operator/artifacts/{id}/download", get(download))
        .route("/v1/operator/domain", post(domain))
        .route("/v1/operator/runs/{id}/control", post(control))
        .route("/v1/operator/protocols", get(protocols))
        .route("/v1/operator/protocols/{id}", get(protocol_detail))
        .route("/v1/operator/maintenance", get(proposals))
        .route("/v1/operator/maintenance/decide", post(decide))
        .route("/v1/operator/reviews", get(reviews))
        .route("/v1/operator/reviews/{id}/task", post(review_task))
        .route("/v1/operator/secrets", get(secrets).post(save_secret))
        .route("/v1/operator/secrets/{id}/disable", post(disable_secret))
        .route("/v1/operator/settings", get(settings).put(save_settings))
        .route("/v1/operator/metrics", get(metrics))
        .route("/v1/operator/providers", get(providers).post(save_provider))
        .route("/v1/operator/provider-models", get(provider_models))
        .route(
            "/v1/operator/providers/{id}/disable",
            post(disable_provider),
        )
        .route("/v1/operator/events", get(events))
        .route("/assets/{name}", get(asset))
}
async fn launch_goal(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(r): Json<crate::autonomy::Launch>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    let (value, spawn) = crate::autonomy::launch(&s, r).await?;
    if spawn {
        let id = serde_json::from_value(value["run_id"].clone()).map_err(|_| bad())?;
        tokio::spawn(execution::drive(s, id));
    }
    Ok(Json(value))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchTask {
    tenant_id: Uuid,
    request_id: Uuid,
    #[serde(default)]
    mode: Option<String>,
}
async fn launch_task(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(r): Json<LaunchTask>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    let managed: bool =
        sqlx::query_scalar("SELECT launch_key IS NOT NULL FROM tasks WHERE tenant_id=$1 AND id=$2")
            .bind(r.tenant_id)
            .bind(id)
            .fetch_optional(&s.pool)
            .await
            .map_err(db)?
            .ok_or_else(missing)?;
    let mode = r.mode.unwrap_or_else(|| "work".into());
    let (connection_id, model) = crate::autonomy::defaults(&s, r.tenant_id).await?;
    let c = execution::StartRun {
        request_id: r.request_id,
        tenant_id: r.tenant_id,
        task_id: id,
        connection_id,
        model,
        target: format!("task:{id}"),
        max_calls: 48,
        max_total_tokens: 300000,
        mode: mode.clone(),
        protocol_id: None,
        protocol_version: None,
        parameters: Value::Null,
        retain_knowledge: !managed && mode == "work",
        dream_after: !managed && mode == "work",
        prepared_pack_id: None,
        eager_context: false,
        auto_configure: managed,
    };
    let (value, spawn) = execution::start(&s, c).await?;
    if spawn {
        let run = serde_json::from_value(value["id"].clone()).map_err(|_| bad())?;
        tokio::spawn(execution::drive(s, run));
    }
    Ok(Json(value))
}
async fn open_session(State(s): State<Shared>, h: HeaderMap) -> crate::error::AppResult<Response> {
    ui_auth::open_session(&s, &h).await
}
async fn session(State(s): State<Shared>, h: HeaderMap) -> crate::error::AppResult<Json<Value>> {
    ui_auth::session(&s, &h).await.map(Json)
}
async fn logout(State(s): State<Shared>, h: HeaderMap) -> crate::error::AppResult<Response> {
    ui_auth::logout(&s, &h).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewTenant {
    name: String,
}
async fn tenants(State(s): State<Shared>, h: HeaderMap) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    let rows = sqlx::query("SELECT id,name FROM tenants ORDER BY name LIMIT 500")
        .fetch_all(&s.pool)
        .await
        .map_err(db)?;
    Ok(Json(
        json!({"tenants":rows.iter().map(|r|json!({"id":r.get::<Uuid,_>("id"),"name":r.get::<String,_>("name")})).collect::<Vec<_>>()}),
    ))
}
async fn create_tenant(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(r): Json<NewTenant>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    if !nonempty(&r.name, 160) {
        return Err(bad());
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO tenants(id,name) VALUES($1,$2)")
        .bind(id)
        .bind(r.name)
        .execute(&s.pool)
        .await
        .map_err(db)?;
    Ok(Json(json!({"id":id})))
}
async fn workspace_open(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    let mut tx = s.pool.begin().await.map_err(db)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("admin-task:{id}"))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tenants WHERE id=$1)")
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    if !exists {
        return Err(missing());
    }
    let old: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM tasks WHERE tenant_id=$1 AND contract->>'kind'='administration'",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?;
    let task = old.unwrap_or_else(Uuid::new_v4);
    if old.is_none() {
        sqlx::query("INSERT INTO tasks(id,tenant_id,goal,contract,status) VALUES($1,$2,'중앙 지식 관리','{\"kind\":\"administration\"}','ready')").bind(task).bind(id).execute(&mut *tx).await.map_err(db)?;
    }
    tx.commit().await.map_err(db)?;
    Ok(Json(json!({"tenant_id":id,"administration_task_id":task})))
}
#[derive(Deserialize)]
struct Scope {
    tenant_id: Uuid,
    task_id: Option<Uuid>,
}
#[derive(Deserialize)]
struct TaskQuery {
    tenant_id: Uuid,
    #[serde(default)]
    query: String,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    offset: i64,
}
async fn tasks(
    State(s): State<Shared>,
    h: HeaderMap,
    Query(q): Query<TaskQuery>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    if q.query.len() > 200 || !(0..=10000).contains(&q.offset) {
        return Err(bad());
    }
    let rows=sqlx::query("SELECT t.id,t.goal,t.status,t.archived,t.contract,t.created_at::text AS created_at,t.updated_at::text AS updated_at,(SELECT r.id FROM execution_runs r WHERE r.task_id=t.id ORDER BY r.created_at DESC LIMIT 1) AS run_id FROM tasks t WHERE t.tenant_id=$1 AND t.archived=$2 AND COALESCE(t.contract->>'kind','work')<>'administration' AND strpos(lower(t.goal),lower($3))>0 ORDER BY t.updated_at DESC,t.id LIMIT 100 OFFSET $4").bind(q.tenant_id).bind(q.archived).bind(q.query).bind(q.offset).fetch_all(&s.pool).await.map_err(db)?;
    Ok(Json(
        json!({"tasks":rows.iter().map(|r|json!({"id":r.get::<Uuid,_>("id"),"goal":r.get::<String,_>("goal"),"status":r.get::<String,_>("status"),"archived":r.get::<bool,_>("archived"),"contract":r.get::<Value,_>("contract"),"run_id":r.get::<Option<Uuid>,_>("run_id"),"created_at":r.get::<String,_>("created_at"),"updated_at":r.get::<String,_>("updated_at")})).collect::<Vec<_>>()}),
    ))
}
async fn task(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<Scope>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    let row=sqlx::query("SELECT id,goal,plan,status,contract,archived,launch_key,created_at::text AS created_at FROM tasks WHERE tenant_id=$1 AND id=$2").bind(q.tenant_id).bind(id).fetch_optional(&s.pool).await.map_err(db)?.ok_or_else(missing)?;
    let orders=sqlx::query("SELECT w.id,w.target,w.status,w.contract,r.result FROM work_orders w LEFT JOIN work_order_results r ON r.work_order_id=w.id WHERE w.tenant_id=$1 AND w.task_id=$2 ORDER BY w.created_at LIMIT 100").bind(q.tenant_id).bind(id).fetch_all(&s.pool).await.map_err(db)?;
    let files=sqlx::query("SELECT id,name,media_type,sha256,work_order_id,octet_length(body) AS bytes FROM artifacts WHERE tenant_id=$1 AND task_id=$2 ORDER BY created_at LIMIT 200").bind(q.tenant_id).bind(id).fetch_all(&s.pool).await.map_err(db)?;
    let runs=sqlx::query("SELECT id,status,calls,created_at::text AS created_at,error_code,state->'result' AS result FROM execution_runs WHERE tenant_id=$1 AND task_id=$2 ORDER BY created_at DESC LIMIT 30").bind(q.tenant_id).bind(id).fetch_all(&s.pool).await.map_err(db)?;
    let inputs=sqlx::query("SELECT id,content,created_at::text AS created_at FROM task_inputs WHERE tenant_id=$1 AND task_id=$2 ORDER BY created_at LIMIT 32").bind(q.tenant_id).bind(id).fetch_all(&s.pool).await.map_err(db)?;
    let receipts=sqlx::query("SELECT request_key,work_order_id,tool,status,result FROM tool_receipts WHERE tenant_id=$1 AND task_id=$2 ORDER BY created_at LIMIT 100").bind(q.tenant_id).bind(id).fetch_all(&s.pool).await.map_err(db)?;
    Ok(Json(
        json!({"id":id,"tenant_id":q.tenant_id,"goal":row.get::<String,_>("goal"),"status":row.get::<String,_>("status"),"plan":row.get::<Value,_>("plan"),"contract":row.get::<Value,_>("contract"),"automatic":row.get::<Option<Uuid>,_>("launch_key").is_some(),"archived":row.get::<bool,_>("archived"),"work_orders":orders.iter().map(|r|json!({"id":r.get::<Uuid,_>("id"),"target":r.get::<String,_>("target"),"status":r.get::<String,_>("status"),"contract":r.get::<Value,_>("contract"),"result":r.get::<Option<Value>,_>("result")})).collect::<Vec<_>>(),"artifacts":files.iter().map(workspace::artifact_header).collect::<Vec<_>>(),"runs":runs.iter().map(|r|json!({"id":r.get::<Uuid,_>("id"),"status":r.get::<String,_>("status"),"calls":r.get::<i32,_>("calls"),"error_code":r.get::<Option<String>,_>("error_code"),"result":r.get::<Option<Value>,_>("result")})).collect::<Vec<_>>(),"tool_receipts":receipts.iter().map(|r|json!({"request_id":r.get::<Uuid,_>("request_key"),"work_order_id":r.get::<Uuid,_>("work_order_id"),"tool":r.get::<String,_>("tool"),"status":r.get::<String,_>("status"),"result":r.get::<Option<Value>,_>("result")})).collect::<Vec<_>>(),"inputs":inputs.iter().map(|r|json!({"id":r.get::<Uuid,_>("id"),"content":r.get::<String,_>("content"),"created_at":r.get::<String,_>("created_at")})).collect::<Vec<_>>()}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateTask {
    tenant_id: Uuid,
    contract: Option<TaskContract>,
    goal: Option<String>,
    status: Option<String>,
    archived: Option<bool>,
    reason: String,
}
async fn update_task(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(r): Json<UpdateTask>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    if !nonempty(&r.reason, 1000) {
        return Err(bad());
    }
    let mut tx = s.pool.begin().await.map_err(db)?;
    let row =
        sqlx::query("SELECT status,contract FROM tasks WHERE tenant_id=$1 AND id=$2 FOR UPDATE")
            .bind(r.tenant_id)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?
            .ok_or_else(missing)?;
    let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM execution_runs WHERE task_id=$1 AND status IN ('queued','running'))").bind(id).fetch_one(&mut *tx).await.map_err(db)?;
    if active {
        return Err(crate::error::AppError::new(
            axum::http::StatusCode::CONFLICT,
            "pause_execution_before_edit",
        ));
    }
    if let Some(c) = &r.contract {
        c.validate()?;
        validate_contract(&s.pool, r.tenant_id, id, c).await?;
        if c.model_access == "blocked" {
            sqlx::query("WITH RECURSIVE restricted(id) AS (SELECT item_id FROM knowledge_revisions WHERE tenant_id=$1 AND source_task_id=$2 UNION SELECT e.source_id FROM knowledge_edges e JOIN restricted r ON r.id=e.target_id WHERE e.tenant_id=$1) UPDATE knowledge_items SET external_model_allowed=false WHERE tenant_id=$1 AND id IN (SELECT id FROM restricted)").bind(r.tenant_id).bind(id).execute(&mut *tx).await.map_err(db)?;
        }
    }
    if let Some(goal) = &r.goal
        && (row.get::<String, _>("status") != "draft" || !nonempty(goal, 4000))
    {
        return Err(bad());
    }
    if let Some(status) = &r.status {
        if !matches!(
            status.as_str(),
            "draft"
                | "ready"
                | "blocked"
                | "paused"
                | "waiting_input"
                | "completed"
                | "failed"
                | "cancelled"
        ) {
            return Err(bad());
        }
        if row.get::<String, _>("status") == "completed" && status != "completed" {
            return Err(bad());
        }
    }
    sqlx::query("UPDATE tasks SET contract=COALESCE($3,contract),goal=COALESCE($4,goal),status=COALESCE($5,status),archived=COALESCE($6,archived),updated_at=now() WHERE tenant_id=$1 AND id=$2").bind(r.tenant_id).bind(id).bind(r.contract.map(|c|json!(c))).bind(r.goal).bind(r.status).bind(r.archived).execute(&mut *tx).await.map_err(db)?;
    sqlx::query("INSERT INTO audit_records(tenant_id,task_id,actor_role,action,outcome,details) VALUES($1,$2,'operator','task.updated','completed',$3)").bind(r.tenant_id).bind(id).bind(json!({"reason":r.reason})).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(Json(json!({"updated":true})))
}
pub async fn validate_contract(
    pool: &sqlx::PgPool,
    tenant: Uuid,
    task: Uuid,
    c: &TaskContract,
) -> crate::error::AppResult<()> {
    for id in &c.input_artifacts {
        let valid: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM artifacts WHERE tenant_id=$1 AND task_id=$2 AND id=$3)",
        )
        .bind(tenant)
        .bind(task)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(db)?;
        if !valid {
            return Err(missing());
        }
    }
    for id in &c.secret_refs {
        let valid: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM secret_refs WHERE tenant_id=$1 AND id=$2 AND enabled)",
        )
        .bind(tenant)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(db)?;
        if !valid {
            return Err(missing());
        }
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    tenant_id: Uuid,
    content: String,
}
async fn task_input(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(r): Json<Input>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    if !nonempty(&r.content, 16000) {
        return Err(bad());
    }
    let input = Uuid::new_v4();
    sqlx::query("INSERT INTO task_inputs(id,tenant_id,task_id,content) VALUES($1,$2,$3,$4)")
        .bind(input)
        .bind(r.tenant_id)
        .bind(id)
        .bind(r.content)
        .execute(&s.pool)
        .await
        .map_err(db)?;
    audit(
        &s.pool,
        Some(r.tenant_id),
        Some(id),
        "operator",
        "task.input",
        "recorded",
        json!({"input_id":input}),
    )
    .await?;
    Ok(Json(json!({"input_id":input})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Upload {
    tenant_id: Uuid,
    task_id: Uuid,
    artifact: Value,
}
async fn upload(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(r): Json<Upload>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    let a = workspace::actor(r.tenant_id, r.task_id, Role::Handler);
    workspace::put_artifact(&s.pool, &a, &r.artifact, true)
        .await
        .map(Json)
}
async fn owned_artifact(s: &AppState, q: &Scope, id: Uuid) -> crate::error::AppResult<Value> {
    let task:Uuid=sqlx::query_scalar("SELECT task_id FROM artifacts WHERE tenant_id=$1 AND id=$2 AND ($3::uuid IS NULL OR task_id=$3)").bind(q.tenant_id).bind(id).bind(q.task_id).fetch_optional(&s.pool).await.map_err(db)?.ok_or_else(missing)?;
    workspace::artifact(
        &s.pool,
        &workspace::actor(q.tenant_id, task, Role::Handler),
        id,
    )
    .await
}
async fn artifact(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<Scope>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    owned_artifact(&s, &q, id).await.map(Json)
}
async fn download(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<Scope>,
) -> crate::error::AppResult<Response> {
    operator(&s, &h).await?;
    let a = owned_artifact(&s, &q, id).await?;
    let bytes = STANDARD
        .decode(a["data_base64"].as_str().ok_or_else(bad)?)
        .map_err(|_| bad())?;
    let name = a["artifact"]["name"]
        .as_str()
        .ok_or_else(bad)?
        .rsplit('/')
        .next()
        .ok_or_else(bad)?;
    let encoded: String = url::form_urlencoded::byte_serialize(name.as_bytes()).collect();
    let mut response = bytes.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/octet-stream".parse().unwrap(),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=artifact-{id}; filename*=UTF-8''{encoded}")
            .parse()
            .map_err(|_| bad())?,
    );
    Ok(response)
}
async fn domain(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(r): Json<ToolRequest>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    let role = match r.tool {
        Capability::KnowledgeRead
        | Capability::KnowledgeClassify
        | Capability::KnowledgePublish
        | Capability::ContextCurate => Role::Analyst,
        Capability::KnowledgeOptimize
        | Capability::ProtocolManage
        | Capability::ProtocolRead
        | Capability::AuditRead => Role::Master,
        Capability::KnowledgeSubmit
        | Capability::ContextRequest
        | Capability::ContextRead
        | Capability::TaskPlan
        | Capability::AgentDelegate
        | Capability::WorkOrderRead
        | Capability::ArtifactRead => Role::Handler,
        _ => {
            return Err(crate::error::AppError::forbidden(
                "operator_domain_tool_denied",
            ));
        }
    };
    let valid: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tasks WHERE tenant_id=$1 AND id=$2)")
            .bind(r.tenant_id)
            .bind(r.task_id)
            .fetch_one(&s.pool)
            .await
            .map_err(db)?;
    if !valid {
        return Err(missing());
    }
    invoke_as(&s, &workspace::actor(r.tenant_id, r.task_id, role), &r)
        .await
        .map(Json)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Control {
    action: String,
}
async fn control(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(r): Json<Control>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    if execution::control(&s.pool, id, &r.action).await? {
        tokio::spawn(execution::drive(s.clone(), id));
    }
    execution::read(&s.pool, id).await.map(Json)
}
#[derive(Deserialize)]
struct ProtocolQuery {
    tenant_id: Uuid,
    #[serde(default)]
    query: String,
    version: Option<i32>,
}
async fn protocols(
    State(s): State<Shared>,
    h: HeaderMap,
    Query(q): Query<ProtocolQuery>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    protocol::list(&s.pool, q.tenant_id, &q.query)
        .await
        .map(Json)
}
async fn protocol_detail(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<ProtocolQuery>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    protocol::load(&s.pool, q.tenant_id, id, q.version)
        .await
        .map(Json)
}
async fn proposals(
    State(s): State<Shared>,
    h: HeaderMap,
    Query(q): Query<Scope>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    maintenance::list(&s.pool, q.tenant_id).await.map(Json)
}
async fn decide(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(r): Json<maintenance::Decision>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    maintenance::decide(&s.pool, r).await.map(Json)
}
async fn reviews(
    State(s): State<Shared>,
    h: HeaderMap,
    Query(q): Query<Scope>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    let rows=sqlx::query("SELECT r.id,r.item_id,r.reason,w.task_id,t.status FROM review_requests r LEFT JOIN review_work_tasks w ON w.tenant_id=r.tenant_id AND w.review_id=r.id LEFT JOIN tasks t ON t.tenant_id=w.tenant_id AND t.id=w.task_id WHERE r.tenant_id=$1 ORDER BY r.created_at DESC LIMIT 100").bind(q.tenant_id).fetch_all(&s.pool).await.map_err(db)?;
    Ok(Json(
        json!({"reviews":rows.iter().map(|r|json!({"id":r.get::<Uuid,_>("id"),"item_id":r.get::<Option<Uuid>,_>("item_id"),"reason":r.get::<String,_>("reason"),"task_id":r.get::<Option<Uuid>,_>("task_id"),"status":r.get::<Option<String>,_>("status")})).collect::<Vec<_>>()}),
    ))
}
async fn review_task(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(q): Json<TenantOnly>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    let mut tx = s.pool.begin().await.map_err(db)?;
    let row = sqlx::query(
        "SELECT reason,item_id FROM review_requests WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(q.tenant_id)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?
    .ok_or_else(missing)?;
    if let Some(task) = sqlx::query_scalar::<_, Uuid>(
        "SELECT task_id FROM review_work_tasks WHERE tenant_id=$1 AND review_id=$2",
    )
    .bind(q.tenant_id)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?
    {
        return Ok(Json(json!({"task_id":task,"replayed":true})));
    }
    let task = Uuid::new_v4();
    let goal = format!(
        "Verify registered knowledge {:?}. Master review request: {}. Check actual authorized sources, preserve evidence and report uncertainty. Define the permitted sources and tools before execution.",
        row.get::<Option<Uuid>, _>("item_id"),
        row.get::<String, _>("reason")
    );
    sqlx::query("INSERT INTO tasks(id,tenant_id,goal,contract) VALUES($1,$2,$3,'{\"kind\":\"work\",\"output_format\":\"Verification report\",\"max_work_orders\":2,\"allowed_tools\":[\"artifact.write\"]}')").bind(task).bind(q.tenant_id).bind(goal).execute(&mut *tx).await.map_err(db)?;
    sqlx::query("INSERT INTO review_work_tasks(tenant_id,review_id,task_id) VALUES($1,$2,$3)")
        .bind(q.tenant_id)
        .bind(id)
        .bind(task)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    sqlx::query("INSERT INTO audit_records(tenant_id,task_id,actor_role,action,outcome,details) VALUES($1,$2,'operator','review.task.created','completed',$3)").bind(q.tenant_id).bind(task).bind(json!({"review_request_id":id})).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(Json(json!({"task_id":task})))
}
async fn secrets(
    State(s): State<Shared>,
    h: HeaderMap,
    Query(q): Query<Scope>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    broker::secret_list(&s.pool, q.tenant_id).await.map(Json)
}
async fn save_secret(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(r): Json<broker::SecretInput>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    broker::save_secret(&s, r).await.map(Json)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TenantOnly {
    tenant_id: Uuid,
}
async fn disable_secret(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(r): Json<TenantOnly>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    let rows = sqlx::query("UPDATE secret_refs SET enabled=false WHERE tenant_id=$1 AND id=$2")
        .bind(r.tenant_id)
        .bind(id)
        .execute(&s.pool)
        .await
        .map_err(db)?;
    if rows.rows_affected() != 1 {
        return Err(missing());
    }
    audit(
        &s.pool,
        Some(r.tenant_id),
        None,
        "operator",
        "secret.disabled",
        "completed",
        json!({"secret_ref":id}),
    )
    .await?;
    Ok(Json(json!({"disabled":true})))
}
async fn settings(
    State(s): State<Shared>,
    h: HeaderMap,
    Query(q): Query<Scope>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    let row = sqlx::query("SELECT accent,preferences FROM tenant_settings WHERE tenant_id=$1")
        .bind(q.tenant_id)
        .fetch_optional(&s.pool)
        .await
        .map_err(db)?;
    Ok(Json(match row {
        Some(r) => {
            json!({"accent":r.get::<String,_>("accent"),"preferences":r.get::<Value,_>("preferences")})
        }
        None => json!({"accent":"#78cfbf","preferences":{}}),
    }))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    tenant_id: Uuid,
    accent: String,
    preferences: Value,
}
async fn save_settings(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(r): Json<Settings>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    if r.accent.len() != 7
        || !r.accent.starts_with('#')
        || !r.accent[1..].bytes().all(|b| b.is_ascii_hexdigit())
        || !r.preferences.is_object()
        || serde_json::to_vec(&r.preferences).map_err(|_| bad())?.len() > 8000
    {
        return Err(bad());
    }
    sqlx::query("INSERT INTO tenant_settings(tenant_id,accent,preferences) VALUES($1,$2,$3) ON CONFLICT(tenant_id) DO UPDATE SET accent=EXCLUDED.accent,preferences=EXCLUDED.preferences").bind(r.tenant_id).bind(r.accent).bind(r.preferences).execute(&s.pool).await.map_err(db)?;
    Ok(Json(json!({"saved":true})))
}
async fn metrics(
    State(s): State<Shared>,
    h: HeaderMap,
    Query(q): Query<Scope>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    let rows=sqlx::query("SELECT e.role,count(*) AS calls,COALESCE(sum((e.details->'usage'->>'input_tokens')::bigint),0)::bigint AS input_tokens,COALESCE(sum((e.details->'usage'->>'output_tokens')::bigint),0)::bigint AS output_tokens FROM execution_events e JOIN execution_runs r ON r.id=e.run_id WHERE r.tenant_id=$1 AND ($2::uuid IS NULL OR r.task_id=$2) AND e.kind='model.completed' GROUP BY e.role ORDER BY e.role").bind(q.tenant_id).bind(q.task_id).fetch_all(&s.pool).await.map_err(db)?;
    Ok(Json(
        json!({"roles":rows.iter().map(|r|json!({"role":r.get::<String,_>("role"),"calls":r.get::<i64,_>("calls"),"input_tokens":r.get::<i64,_>("input_tokens"),"output_tokens":r.get::<i64,_>("output_tokens")})).collect::<Vec<_>>()}),
    ))
}
#[derive(Deserialize)]
struct Events {
    tenant_id: Uuid,
    task_id: Option<Uuid>,
    #[serde(default)]
    after: i64,
}
async fn events(
    State(s): State<Shared>,
    h: HeaderMap,
    Query(q): Query<Events>,
) -> crate::error::AppResult<Response> {
    operator(&s, &h).await?;
    let after = h
        .get("last-event-id")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(q.after);
    if after < 0 {
        return Err(bad());
    }
    let stream:BoxStream<'static, Result<Event, Infallible>>=futures_util::stream::unfold((s.clone(),h.clone(),q.tenant_id,q.task_id,after,Vec::<Value>::new()),|(s,h,tenant,task,mut after,mut buffer)|async move {
        if operator(&s,&h).await.is_err(){return None;} let pool=&s.pool;
        if buffer.is_empty() {
            let rows=sqlx::query("SELECT seq,task_id,actor,kind,data,created_at::text AS created_at FROM activity_events WHERE tenant_id=$1 AND ($2::uuid IS NULL OR task_id=$2) AND seq>$3 ORDER BY seq LIMIT 200").bind(tenant).bind(task).bind(after).fetch_all(pool).await;
            if let Ok(rows)=rows {buffer=rows.iter().map(|r|json!({"seq":r.get::<i64,_>("seq"),"task_id":r.get::<Option<Uuid>,_>("task_id"),"actor":r.get::<String,_>("actor"),"kind":r.get::<String,_>("kind"),"data":r.get::<Value,_>("data"),"created_at":r.get::<String,_>("created_at")})).collect();}else{return None;}
        }
        if buffer.is_empty(){tokio::time::sleep(Duration::from_millis(750)).await;return Some((Ok(Event::default().event("heartbeat").data("{}")),(s,h,tenant,task,after,buffer)));}
        let event=buffer.remove(0);after=event["seq"].as_i64().unwrap_or(after);
        Some((Ok(Event::default().id(after.to_string()).event("activity").data(event.to_string())),(s,h,tenant,task,after,buffer)))
    }).boxed();
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}
async fn asset(Path(name): Path<String>) -> crate::error::AppResult<Response> {
    if name.len() > 120
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        return Err(missing());
    }
    let bytes = tokio::fs::read(std::path::Path::new("/app/ui/assets").join(&name))
        .await
        .map_err(|_| missing())?;
    let mut response = bytes.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        if name.ends_with(".js") {
            "application/javascript"
        } else if name.ends_with(".css") {
            "text/css"
        } else {
            "application/octet-stream"
        }
        .parse()
        .unwrap(),
    );
    Ok(response)
}
async fn providers(
    State(s): State<Shared>,
    h: HeaderMap,
    Query(q): Query<Scope>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    crate::provider::list(&s, q.tenant_id).await.map(Json)
}
async fn save_provider(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(r): Json<crate::provider::Save>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    crate::provider::save(&s, r).await.map(Json)
}
#[derive(Deserialize)]
struct Models {
    tenant_id: Uuid,
    connection_id: String,
}
async fn provider_models(
    State(s): State<Shared>,
    h: HeaderMap,
    Query(q): Query<Models>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    crate::provider::models(&s, q.tenant_id, &q.connection_id)
        .await
        .map(Json)
}
async fn disable_provider(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(q): Json<TenantOnly>,
) -> crate::error::AppResult<Json<Value>> {
    operator(&s, &h).await?;
    let r =
        sqlx::query("UPDATE provider_connections SET enabled=false WHERE tenant_id=$1 AND id=$2")
            .bind(q.tenant_id)
            .bind(id)
            .execute(&s.pool)
            .await
            .map_err(db)?;
    if r.rows_affected() != 1 {
        return Err(missing());
    }
    Ok(Json(json!({"disabled":true})))
}
