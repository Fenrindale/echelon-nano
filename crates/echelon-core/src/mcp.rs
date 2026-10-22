//! Workspace MCP installations and Work Order scoped invocation. Secrets never enter model views.
use crate::{
    broker, credentials,
    error::{AppError, AppResult},
    policy::{Claims, Role},
    server::{AppState, operator},
    store::audit,
    workspace::{self, TaskContract, UrlGrant, bad, db, missing},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post},
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentBinding {
    pub name: String,
    pub bundle_id: Uuid,
    pub variable: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Spec {
    pub transport: String,
    pub package: String,
    pub package_version: String,
    pub executable: String,
    pub args: Vec<String>,
    pub url: String,
    pub allow_private: bool,
    pub allow_http: bool,
    pub environment: Vec<EnvironmentBinding>,
}
impl Default for Spec {
    fn default() -> Self {
        Self {
            transport: "npm".into(),
            package: String::new(),
            package_version: String::new(),
            executable: String::new(),
            args: vec![],
            url: String::new(),
            allow_private: false,
            allow_http: false,
            environment: vec![],
        }
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub server_id: Uuid,
    pub version: i64,
    pub tools: Vec<String>,
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
}
impl Spec {
    fn validate(&self) -> AppResult<()> {
        for argument in &self.args {
            let flag = argument
                .split('=')
                .next()
                .unwrap_or("")
                .trim_start_matches('-')
                .replace('-', "_")
                .to_lowercase();
            if matches!(
                flag.as_str(),
                "password"
                    | "token"
                    | "api_key"
                    | "secret"
                    | "authorization"
                    | "access_token"
                    | "refresh_token"
                    | "client_secret"
            ) {
                return Err(AppError::bad("mcp_credentials_require_vault"));
            }
            if let Ok(url) = url::Url::parse(argument)
                && (!url.username().is_empty()
                    || url.password().is_some()
                    || url.query_pairs().any(|(key, _)| {
                        matches!(
                            key.as_ref(),
                            "token" | "password" | "api_key" | "secret" | "access_token"
                        )
                    }))
            {
                return Err(AppError::bad("mcp_credentials_require_vault"));
            }
        }
        if !matches!(self.transport.as_str(), "npm" | "python" | "http")
            || self.args.len() > 32
            || self
                .args
                .iter()
                .any(|a| a.len() > 2048 || a.chars().any(char::is_control))
            || self.environment.len() > 32
            || self.environment.iter().any(|e| {
                !identifier(&e.name)
                    || !identifier(&e.variable)
                    || [
                        "PATH",
                        "HOME",
                        "TMPDIR",
                        "PYTHONPATH",
                        "NODE_OPTIONS",
                        "LD_PRELOAD",
                        "LD_LIBRARY_PATH",
                    ]
                    .contains(&e.name.as_str())
            })
        {
            return Err(bad());
        }
        if self.transport == "http" {
            TaskContract {
                kind: "work".into(),
                urls: vec![self.grant()],
                ..Default::default()
            }
            .validate()?;
            if !self.package.is_empty() || !self.executable.is_empty() || !self.args.is_empty() {
                return Err(bad());
            }
        } else if self.package.is_empty()
            || self.package.len() > 200
            || !self
                .package
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"@/_.-".contains(&c))
            || !self
                .package_version
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_digit)
            || self.package_version.len() > 80
            || !self
                .package_version
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".+_-".contains(&c))
            || !identifier(&self.executable)
            || !self.url.is_empty()
        {
            return Err(bad());
        }
        Ok(())
    }
    fn grant(&self) -> UrlGrant {
        UrlGrant {
            url: self.url.clone(),
            allow_private: self.allow_private,
            allow_http: self.allow_http,
        }
    }
}
#[derive(Deserialize)]
struct Tenant {
    tenant_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Save {
    tenant_id: Uuid,
    id: Option<Uuid>,
    expected_version: Option<i64>,
    name: String,
    enabled: bool,
    spec: Spec,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    tenant_id: Uuid,
    expected_version: i64,
}
type Shared = Arc<AppState>;
pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/v1/operator/mcp", get(list).post(save))
        .route("/v1/operator/mcp/{id}/install", post(install))
}
async fn list(
    State(s): State<Shared>,
    h: HeaderMap,
    Query(q): Query<Tenant>,
) -> AppResult<Json<Value>> {
    operator(&s, &h).await?;
    Ok(Json(catalog(&s.pool, q.tenant_id, false).await?))
}
pub async fn catalog(pool: &PgPool, tenant: Uuid, compact: bool) -> AppResult<Value> {
    let rows=sqlx::query("SELECT id,name,version,enabled,spec,tools,status,error_code FROM mcp_servers WHERE tenant_id=$1 ORDER BY name LIMIT 64").bind(tenant).fetch_all(pool).await.map_err(db)?;
    Ok(
        json!({"servers":rows.iter().filter(|r|!compact || r.get::<bool,_>("enabled")).map(|r|{let tools:Value=r.get("tools");json!({"id":r.get::<Uuid,_>("id"),"name":r.get::<String,_>("name"),"version":r.get::<i64,_>("version"),"enabled":r.get::<bool,_>("enabled"),"status":r.get::<String,_>("status"),"error_code":r.get::<Option<String>,_>("error_code"),"spec":if compact {Value::Null}else{r.get::<Value,_>("spec")},"tools":if compact {json!(tools.as_array().into_iter().flatten().map(|t|json!({"name":t["name"],"description":t["description"].as_str().unwrap_or("").chars().take(160).collect::<String>()})).collect::<Vec<_>>())}else{tools}})}).collect::<Vec<_>>()}),
    )
}
async fn save(
    State(s): State<Shared>,
    h: HeaderMap,
    Json(r): Json<Save>,
) -> AppResult<Json<Value>> {
    operator(&s, &h).await?;
    r.spec.validate()?;
    if !workspace::nonempty(&r.name, 120) {
        return Err(bad());
    }
    credentials::mcp_environment(&s, r.tenant_id, &r.spec.environment).await?;
    let id = r.id.unwrap_or_else(Uuid::new_v4);
    let spec = serde_json::to_value(&r.spec).map_err(|_| bad())?;
    let mut tx = s.pool.begin().await.map_err(db)?;
    let version = if r.id.is_some() {
        sqlx::query_scalar::<_,i64>("UPDATE mcp_servers SET name=$3,enabled=$4,spec=$5,tools='[]',version=version+1,status='configured',error_code=NULL,updated_at=now() WHERE tenant_id=$1 AND id=$2 AND version=$6 RETURNING version").bind(r.tenant_id).bind(id).bind(&r.name).bind(r.enabled).bind(&spec).bind(r.expected_version).fetch_optional(&mut *tx).await.map_err(db)?.ok_or_else(||AppError::bad("mcp_version_conflict"))?
    } else {
        sqlx::query(
            "INSERT INTO mcp_servers(id,tenant_id,name,enabled,spec) VALUES($1,$2,$3,$4,$5)",
        )
        .bind(id)
        .bind(r.tenant_id)
        .bind(&r.name)
        .bind(r.enabled)
        .bind(&spec)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        1
    };
    tx.commit().await.map_err(db)?;
    audit(
        &s.pool,
        Some(r.tenant_id),
        None,
        "operator",
        "mcp.configured",
        "success",
        json!({"server_id":id,"version":version}),
    )
    .await?;
    Ok(Json(
        json!({"id":id,"version":version,"status":"configured"}),
    ))
}
async fn install(
    State(s): State<Shared>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(r): Json<Operation>,
) -> AppResult<Json<Value>> {
    operator(&s, &h).await?;
    let spec: Value = sqlx::query_scalar(
        "SELECT spec FROM mcp_servers WHERE tenant_id=$1 AND id=$2 AND version=$3 AND enabled",
    )
    .bind(r.tenant_id)
    .bind(id)
    .bind(r.expected_version)
    .fetch_optional(&s.pool)
    .await
    .map_err(db)?
    .ok_or_else(missing)?;
    let spec: Spec = serde_json::from_value(spec).map_err(|_| bad())?;
    let env = credentials::mcp_environment(&s, r.tenant_id, &spec.environment).await?;
    let result = transport(&spec, &env, "discover", None, None).await;
    let tools = match result {
        Ok(v) => {
            validate_tools(&v["tools"])?;
            scrub(v["tools"].clone(), &env)
        }
        Err(e) => {
            sqlx::query("UPDATE mcp_servers SET status='failed',error_code=$4 WHERE tenant_id=$1 AND id=$2 AND version=$3").bind(r.tenant_id).bind(id).bind(r.expected_version).bind(e.code).execute(&s.pool).await.map_err(db)?;
            return Err(e);
        }
    };
    let mut tx = s.pool.begin().await.map_err(db)?;
    let updated=sqlx::query("UPDATE mcp_servers SET tools=$4,status='ready',error_code=NULL,updated_at=now() WHERE tenant_id=$1 AND id=$2 AND version=$3 AND enabled").bind(r.tenant_id).bind(id).bind(r.expected_version).bind(&tools).execute(&mut *tx).await.map_err(db)?;
    if updated.rows_affected() != 1 {
        return Err(AppError::bad("mcp_version_conflict"));
    }
    sqlx::query("INSERT INTO mcp_server_revisions(server_id,version,spec,tools) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING").bind(id).bind(r.expected_version).bind(json!(spec)).bind(&tools).execute(&mut *tx).await.map_err(db)?;
    let frozen: Value = sqlx::query_scalar(
        "SELECT tools FROM mcp_server_revisions WHERE server_id=$1 AND version=$2",
    )
    .bind(id)
    .bind(r.expected_version)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    if frozen != tools {
        return Err(AppError::bad("mcp_tools_changed_reconfigure_required"));
    }
    tx.commit().await.map_err(db)?;
    audit(&s.pool,Some(r.tenant_id),None,"operator","mcp.installed","success",json!({"server_id":id,"version":r.expected_version,"tool_count":tools.as_array().map_or(0,Vec::len)})).await?;
    Ok(Json(json!({"id":id,"status":"ready","tools":tools})))
}
fn validate_tools(tools: &Value) -> AppResult<()> {
    let a = tools
        .as_array()
        .filter(|v| v.len() <= 256)
        .ok_or_else(bad)?;
    if a.iter().any(|t| {
        t["name"]
            .as_str()
            .is_none_or(|n| n.is_empty() || n.len() > 200)
            || !t["inputSchema"].is_object()
    }) || tools.to_string().len() > 262144
    {
        return Err(AppError::bad("mcp_tools_invalid"));
    }
    Ok(())
}
pub async fn validate_grants(pool: &PgPool, tenant: Uuid, grants: &[Grant]) -> AppResult<()> {
    if grants.len() > 16 {
        return Err(bad());
    }
    for g in grants {
        let tools:Option<Value>=sqlx::query_scalar("SELECT r.tools FROM mcp_server_revisions r JOIN mcp_servers s ON s.id=r.server_id WHERE s.tenant_id=$1 AND s.id=$2 AND s.enabled AND r.version=$3").bind(tenant).bind(g.server_id).bind(g.version).fetch_optional(pool).await.map_err(db)?;
        let tools = tools.ok_or_else(|| AppError::bad("mcp_not_ready"))?;
        if g.tools.is_empty()
            || g.tools.len() > 32
            || g.tools.iter().any(|n| {
                !tools
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|t| t["name"] == *n)
            })
        {
            return Err(AppError::bad("mcp_tool_not_installed"));
        }
    }
    Ok(())
}
pub async fn work_evidence(pool: &PgPool, a: &Claims, order: Uuid) -> AppResult<Value> {
    let rows=sqlx::query("SELECT server_id,server_version,tool,status,result,request_key FROM mcp_call_receipts WHERE tenant_id=$1 AND task_id=$2 AND work_order_id=$3 ORDER BY created_at DESC LIMIT 32").bind(a.tenant_id).bind(a.task_id).bind(order).fetch_all(pool).await.map_err(db)?;
    Ok(json!(rows.iter().map(|r|json!({"server_id":r.get::<Uuid,_>("server_id"),"version":r.get::<i64,_>("server_version"),"tool":r.get::<String,_>("tool"),"status":r.get::<String,_>("status"),"request_id":r.get::<Uuid,_>("request_key"),"result":r.get::<Option<Value>,_>("result")})).collect::<Vec<_>>()))
}
pub async fn inspect(s: &AppState, a: &Claims, input: &Value) -> AppResult<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Read {
        server_id: Uuid,
    }
    let r: Read = serde_json::from_value(input.clone()).map_err(|_| bad())?;
    let row=sqlx::query("SELECT id,name,version,status,tools FROM mcp_servers WHERE tenant_id=$1 AND id=$2 AND enabled").bind(a.tenant_id).bind(r.server_id).fetch_optional(&s.pool).await.map_err(db)?.ok_or_else(missing)?;
    Ok(
        json!({"id":r.server_id,"name":row.get::<String,_>("name"),"version":row.get::<i64,_>("version"),"status":row.get::<String,_>("status"),"tools":row.get::<Value,_>("tools")}),
    )
}
pub async fn granted_tools(pool: &PgPool, tenant: Uuid, grants: &[Grant]) -> AppResult<Value> {
    validate_grants(pool, tenant, grants).await?;
    let mut schemas = vec![];
    for grant in grants {
        let tools: Value = sqlx::query_scalar(
            "SELECT tools FROM mcp_server_revisions WHERE server_id=$1 AND version=$2",
        )
        .bind(grant.server_id)
        .bind(grant.version)
        .fetch_one(pool)
        .await
        .map_err(db)?;
        schemas.push(json!({"server_id":grant.server_id,"version":grant.version,"tools":tools.as_array().into_iter().flatten().filter(|t|t["name"].as_str().is_some_and(|n|grant.tools.iter().any(|g|g==n))).collect::<Vec<_>>()}));
    }
    Ok(json!(schemas))
}
pub async fn call(s: &AppState, a: &Claims, input: &Value) -> AppResult<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Call {
        server_id: Uuid,
        tool: String,
        arguments: Value,
        request_id: Uuid,
    }
    if a.role != Role::Agent {
        return Err(AppError::forbidden("role_denied"));
    }
    let r: Call = serde_json::from_value(input.clone()).map_err(|_| bad())?;
    if !r.arguments.is_object() || r.arguments.to_string().len() > 32768 {
        return Err(bad());
    }
    let policy = broker::order_policy(&s.pool, a).await?;
    let grant = policy
        .mcp_tools
        .iter()
        .find(|g| g.server_id == r.server_id && g.tools.contains(&r.tool))
        .filter(|_| policy.allowed_tools.iter().any(|t| t == "mcp.call"))
        .ok_or_else(|| AppError::forbidden("mcp_tool_denied"))?;
    validate_grants(&s.pool, a.tenant_id, std::slice::from_ref(grant)).await?;
    let spec: Value = sqlx::query_scalar(
        "SELECT spec FROM mcp_server_revisions WHERE server_id=$1 AND version=$2",
    )
    .bind(grant.server_id)
    .bind(grant.version)
    .fetch_one(&s.pool)
    .await
    .map_err(db)?;
    let spec: Spec = serde_json::from_value(spec).map_err(|_| bad())?;
    let hash = workspace::digest(
        json!({"server":r.server_id,"version":grant.version,"tool":r.tool,"arguments":r.arguments})
            .to_string()
            .as_bytes(),
    );
    let env = credentials::mcp_environment(s, a.tenant_id, &spec.environment).await?;
    let inserted=sqlx::query("INSERT INTO mcp_call_receipts(tenant_id,task_id,work_order_id,request_key,request_hash,server_id,server_version,tool,status) VALUES($1,$2,$3,$4,$5,$6,$7,$8,'pending') ON CONFLICT DO NOTHING").bind(a.tenant_id).bind(a.task_id).bind(a.work_order_id).bind(r.request_id).bind(&hash).bind(r.server_id).bind(grant.version).bind(&r.tool).execute(&s.pool).await.map_err(db)?;
    if inserted.rows_affected() == 0 {
        let row=sqlx::query("SELECT request_hash,status,result FROM mcp_call_receipts WHERE tenant_id=$1 AND task_id=$2 AND work_order_id=$3 AND request_key=$4").bind(a.tenant_id).bind(a.task_id).bind(a.work_order_id).bind(r.request_id).fetch_optional(&s.pool).await.map_err(db)?.ok_or_else(||AppError::bad("mcp_outcome_unknown_do_not_retry"))?;
        if row.get::<String, _>("request_hash") != hash {
            return Err(AppError::bad("mcp_idempotency_conflict"));
        }
        if row.get::<String, _>("status") == "completed" {
            return Ok(row.get("result"));
        }
        return Err(AppError::bad("mcp_outcome_unknown_do_not_retry"));
    }

    let response = transport(&spec, &env, "call", Some(&r.tool), Some(&r.arguments)).await;
    let result = match response {
        Ok(v) => scrub(v, &env),
        Err(e) => {
            sqlx::query("UPDATE mcp_call_receipts SET status='unknown' WHERE tenant_id=$1 AND task_id=$2 AND work_order_id=$3 AND request_key=$4").bind(a.tenant_id).bind(a.task_id).bind(a.work_order_id).bind(r.request_id).execute(&s.pool).await.map_err(db)?;
            return Err(e);
        }
    };
    let value = json!({"server_id":r.server_id,"server_version":grant.version,"tool":r.tool,"request_id":r.request_id,"operation_ok":result["isError"]!=true,"result":result,"source":"server.mcp_receipt"});
    sqlx::query("UPDATE mcp_call_receipts SET status='completed',result=$5 WHERE tenant_id=$1 AND task_id=$2 AND work_order_id=$3 AND request_key=$4").bind(a.tenant_id).bind(a.task_id).bind(a.work_order_id).bind(r.request_id).bind(&value).execute(&s.pool).await.map_err(db)?;
    Ok(value)
}
fn scrub(value: Value, env: &BTreeMap<String, String>) -> Value {
    match value {
        Value::String(mut s) => {
            s = broker::redact_response(&s, None);
            for secret in env.values().filter(|s| s.len() >= 4) {
                s = broker::redact_response(&s, Some(secret));
            }
            Value::String(s)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(|v| scrub(v, env)).collect()),
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, v)| {
                    let normalized = k.to_ascii_lowercase().replace(['_', '-'], "");
                    let value = if matches!(
                        normalized.as_str(),
                        "password"
                            | "passphrase"
                            | "token"
                            | "accesstoken"
                            | "refreshtoken"
                            | "idtoken"
                            | "apikey"
                            | "authorization"
                            | "clientsecret"
                            | "privatekey"
                            | "setcookie"
                    ) {
                        json!("[REDACTED]")
                    } else {
                        scrub(v, env)
                    };
                    (k, value)
                })
                .collect(),
        ),
        other => other,
    }
}
async fn transport(
    spec: &Spec,
    env: &BTreeMap<String, String>,
    op: &str,
    tool: Option<&str>,
    arguments: Option<&Value>,
) -> AppResult<Value> {
    if spec.transport == "http" {
        return remote(spec, env, op, tool, arguments).await;
    }
    let token = tokio::fs::read_to_string("/run/secrets/worker-token")
        .await
        .map_err(|_| AppError::bad("mcp_host_unavailable"))?;
    let url =
        std::env::var("ECHELON_MCP_HOST_URL").unwrap_or_else(|_| "http://mcp-host:9091".into());
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(270))
        .build()
        .map_err(|_| bad())?;
    let response: Value = client
        .post(format!("{url}/invoke"))
        .bearer_auth(token.trim())
        .json(&json!({"spec":spec,"environment":env,"op":op,"tool":tool,"arguments":arguments}))
        .send()
        .await
        .map_err(|_| AppError::bad("mcp_transport_outcome_unknown"))?
        .error_for_status()
        .map_err(|_| AppError::bad("mcp_host_unavailable"))?
        .json()
        .await
        .map_err(|_| AppError::bad("mcp_response_invalid"))?;
    if response["ok"] != true {
        let code = match response["error"].as_str() {
            Some("mcp_package_install_failed") => "mcp_package_install_failed",
            Some("mcp_process_closed") => "mcp_process_closed",
            Some("mcp_timeout") => "mcp_timeout",
            Some("mcp_protocol_error") => "mcp_protocol_error",
            Some("mcp_package_invalid") => "mcp_package_invalid",
            Some("mcp_protocol_version_unsupported") => "mcp_protocol_version_unsupported",
            _ => "mcp_host_operation_failed",
        };
        return Err(AppError::bad(code));
    }
    Ok(response["result"].clone())
}
async fn remote(
    spec: &Spec,
    env: &BTreeMap<String, String>,
    op: &str,
    tool: Option<&str>,
    arguments: Option<&Value>,
) -> AppResult<Value> {
    let (client, url) = broker::pinned_client(&spec.grant()).await?;
    let mut session = None;
    let mut protocol = "2025-03-26".to_string();
    let init=http_rpc(&client,&url,env,&mut session,&protocol,json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":protocol,"capabilities":{},"clientInfo":{"name":"echelon-nano","version":"0.3"}}})).await?;
    protocol = init["protocolVersion"]
        .as_str()
        .ok_or_else(bad)?
        .to_string();
    if !["2025-03-26", "2025-06-18", "2025-11-25"].contains(&protocol.as_str()) {
        return Err(AppError::bad("mcp_protocol_version_unsupported"));
    }
    http_rpc(
        &client,
        &url,
        env,
        &mut session,
        &protocol,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await?;
    if op == "call" {
        return http_rpc(&client,&url,env,&mut session,&protocol,json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":arguments}})).await;
    }
    let mut tools = vec![];
    let mut cursor = Value::Null;
    for n in 2..18 {
        let page=http_rpc(&client,&url,env,&mut session,&protocol,json!({"jsonrpc":"2.0","id":n,"method":"tools/list","params":if cursor.is_null(){json!({})}else{json!({"cursor":cursor})}})).await?;
        tools.extend(page["tools"].as_array().ok_or_else(bad)?.iter().cloned());
        if tools.len() > 256 {
            return Err(bad());
        }
        cursor = page["nextCursor"].clone();
        if cursor.is_null() {
            return Ok(json!({"tools":tools}));
        }
    }
    Err(AppError::bad("mcp_tools_too_large"))
}
async fn http_rpc(
    client: &reqwest::Client,
    url: &url::Url,
    env: &BTreeMap<String, String>,
    session: &mut Option<String>,
    protocol: &str,
    message: Value,
) -> AppResult<Value> {
    let mut request = client
        .post(url.clone())
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", protocol)
        .json(&message);
    if let Some(s) = session.as_ref() {
        request = request.header("Mcp-Session-Id", s);
    }
    if let Some(token) = env.get("MCP_AUTH_TOKEN") {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|_| AppError::bad("mcp_transport_outcome_unknown"))?
        .error_for_status()
        .map_err(|_| AppError::bad("mcp_http_rejected"))?;
    if let Some(s) = response.headers().get("mcp-session-id") {
        *session = Some(s.to_str().map_err(|_| bad())?.to_string());
    }
    if message.get("id").is_none() {
        return Ok(Value::Null);
    }
    let sse = response
        .headers()
        .get("content-type")
        .and_then(|s| s.to_str().ok())
        .is_some_and(|s| s.starts_with("text/event-stream"));
    let mut bytes = vec![];
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| AppError::bad("mcp_transport_outcome_unknown"))?;
        bytes.extend_from_slice(&chunk);
        if bytes.len() > 1048576 {
            return Err(AppError::bad("mcp_response_too_large"));
        }
        if sse {
            let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
            for event in text.split("\n\n").take(text.matches("\n\n").count()) {
                let data = event
                    .lines()
                    .filter_map(|l| l.strip_prefix("data:").map(str::trim_start))
                    .collect::<Vec<_>>()
                    .join("\n");
                if let Ok(v) = serde_json::from_str::<Value>(&data)
                    && v["id"] == message["id"]
                {
                    return rpc_result(v);
                }
            }
        }
    }
    rpc_result(serde_json::from_slice(&bytes).map_err(|_| AppError::bad("mcp_response_invalid"))?)
}
fn rpc_result(v: Value) -> AppResult<Value> {
    if v.get("error").is_some() {
        return Err(AppError::bad("mcp_protocol_error"));
    }
    v.get("result")
        .cloned()
        .ok_or_else(|| AppError::bad("mcp_response_invalid"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_specs_are_data_not_shell() {
        let mut s = Spec {
            package: "@modelcontextprotocol/server-everything".into(),
            package_version: "1.0.0".into(),
            executable: "mcp-server-everything".into(),
            ..Default::default()
        };
        assert!(s.validate().is_ok());
        s.package_version = "latest; touch /tmp/x".into();
        assert!(s.validate().is_err());
    }
    #[test]
    fn credential_arguments_must_use_vault_bindings() {
        let mut s = Spec {
            package: "owned-example".into(),
            package_version: "1.0.0".into(),
            executable: "owned-example".into(),
            ..Default::default()
        };
        s.args = vec!["--api-key=owned-value".into()];
        assert_eq!(
            s.validate().unwrap_err().code,
            "mcp_credentials_require_vault"
        );
        s.args = vec!["https://example.com/mcp?token=owned-value".into()];
        assert!(s.validate().is_err());
    }
    #[test]
    fn secrets_are_scrubbed_from_nested_results() {
        let env = BTreeMap::from([("TOKEN".into(), "private-value".into())]);
        assert_eq!(
            scrub(json!({"content":[{"text":"echo private-value"}]}), &env)["content"][0]["text"],
            "echo [REDACTED]"
        );
    }
    #[test]
    fn generated_authentication_fields_are_redacted_without_environment_echoes() {
        let safe = scrub(
            json!({"accessToken":"owned-generated-value","content":[{"text":"{\"refresh_token\":\"owned-nested-value\",\"count\":2}"}]}),
            &BTreeMap::new(),
        );
        assert!(!safe.to_string().contains("owned-generated-value"));
        assert!(!safe.to_string().contains("owned-nested-value"));
        assert!(safe.to_string().contains('2'));
    }
}
