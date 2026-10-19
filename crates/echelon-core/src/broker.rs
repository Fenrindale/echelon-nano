use crate::{
    error::{AppError, AppResult},
    policy::{Capability, Claims, Role, ToolRequest},
    server::AppState,
    store::audit,
    worker::{InputFile, Job},
    workspace::{self, TaskContract, UrlGrant, bad, db, missing},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretInput {
    pub tenant_id: Uuid,
    pub id: Option<Uuid>,
    pub expected_version: Option<u64>,
    pub name: String,
    pub purpose: String,
    pub allowed_urls: Vec<String>,
    #[serde(default = "auth_header")]
    pub header: String,
    pub value: String,
}
fn auth_header() -> String {
    "Authorization".into()
}
#[derive(Serialize, Deserialize)]
struct Secret {
    value: String,
    header: String,
}
pub async fn save_secret(s: &AppState, r: SecretInput) -> AppResult<Value> {
    if !workspace::nonempty(&r.name, 160)
        || !workspace::nonempty(&r.purpose, 1000)
        || r.allowed_urls.is_empty()
        || r.allowed_urls.len() > 16
        || !workspace::nonempty(&r.value, 8192)
        || r.value.chars().any(char::is_control)
        || !matches!(r.header.as_str(), "Authorization" | "X-API-Key")
    {
        return Err(bad());
    }
    for text in &r.allowed_urls {
        let u = url::Url::parse(text).map_err(|_| bad())?;
        if !matches!(u.scheme(), "http" | "https")
            || u.host_str().is_none()
            || u.query().is_some()
            || !u.username().is_empty()
            || u.password().is_some()
        {
            return Err(bad());
        }
    }
    let id = r.id.unwrap_or_else(Uuid::new_v4);
    let expected = r.expected_version.unwrap_or(0);
    let mut tx = s.pool.begin().await.map_err(db)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('secret:' || $1::text,0))")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let old: Option<i64> =
        sqlx::query_scalar("SELECT version FROM secret_refs WHERE tenant_id=$1 AND id=$2")
            .bind(r.tenant_id)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
    if old.map_or(0, |v| v as u64) != expected || (r.id.is_some() && old.is_none()) {
        return Err(AppError::new(
            axum::http::StatusCode::CONFLICT,
            "secret_version_conflict",
        ));
    }
    let version = s
        .vault
        .put(
            &format!("usersecrets/{id}"),
            &Secret {
                value: r.value,
                header: r.header,
            },
            expected,
        )
        .await?;
    sqlx::query("INSERT INTO secret_refs(id,tenant_id,name,purpose,allowed_urls,version) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(id) DO UPDATE SET name=EXCLUDED.name,purpose=EXCLUDED.purpose,allowed_urls=EXCLUDED.allowed_urls,version=EXCLUDED.version,enabled=true")
        .bind(id).bind(r.tenant_id).bind(r.name).bind(r.purpose).bind(json!(r.allowed_urls)).bind(version as i64).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    audit(
        &s.pool,
        Some(r.tenant_id),
        None,
        "operator",
        "secret.rotated",
        "completed",
        json!({"secret_ref":id,"version":version}),
    )
    .await?;
    Ok(json!({"secret_ref":id,"version":version}))
}
pub async fn secret_list(pool: &PgPool, tenant: Uuid) -> AppResult<Value> {
    let rows=sqlx::query("SELECT id,name,purpose,allowed_urls,version,enabled FROM secret_refs WHERE tenant_id=$1 ORDER BY name LIMIT 100").bind(tenant).fetch_all(pool).await.map_err(db)?;
    Ok(
        json!({"secrets":rows.iter().map(|r|json!({"id":r.get::<Uuid,_>("id"),"name":r.get::<String,_>("name"),"purpose":r.get::<String,_>("purpose"),"allowed_urls":r.get::<Value,_>("allowed_urls"),"version":r.get::<i64,_>("version"),"enabled":r.get::<bool,_>("enabled")})).collect::<Vec<_>>()}),
    )
}
fn loopback_or_special(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            v.is_loopback()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_multicast()
                || v == Ipv4Addr::BROADCAST
                || v.octets()[0] == 0
        }
        IpAddr::V6(v) => {
            v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || (v.segments()[0] & 0xffc0) == 0xfe80
                || v.to_ipv4_mapped()
                    .is_some_and(|v| loopback_or_special(IpAddr::V4(v)))
        }
    }
}
fn public(ip: IpAddr) -> bool {
    if loopback_or_special(ip) {
        return false;
    }
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_private()
                || v.is_documentation()
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || (o[0] == 198 && (18..=19).contains(&o[1]))
                || o[0] >= 224
                || (o[0] == 192 && o[1] == 0 && o[2] == 0))
        }
        IpAddr::V6(v) => {
            (v.segments()[0] & 0xfe00) != 0xfc00
                && !(v.segments()[0] == 0x2001 && v.segments()[1] == 0x0db8)
        }
    }
}
pub async fn pinned_client(grant: &UrlGrant) -> AppResult<(reqwest::Client, url::Url)> {
    let url = url::Url::parse(&grant.url).map_err(|_| bad())?;
    let host = url.host_str().ok_or_else(bad)?;
    if matches!(
        host,
        "core"
            | "postgres"
            | "vault"
            | "worker"
            | "localhost"
            | "host.docker.internal"
            | "gateway.docker.internal"
    ) {
        return Err(AppError::forbidden("broker_protected_target"));
    }
    let port = url.port_or_known_default().ok_or_else(bad)?;
    let addresses: Vec<_> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| AppError::bad("broker_dns_failed"))?
        .collect();
    if addresses.is_empty()
        || addresses
            .iter()
            .any(|a| loopback_or_special(a.ip()) || (!grant.allow_private && !public(a.ip())))
    {
        return Err(AppError::forbidden("broker_network_scope_denied"));
    }
    for protected in [
        "core",
        "postgres",
        "vault",
        "worker",
        "host.docker.internal",
        "gateway.docker.internal",
    ] {
        if let Ok(protected_addresses) = tokio::net::lookup_host((protected, 8080)).await
            && protected_addresses
                .into_iter()
                .any(|p| addresses.iter().any(|a| a.ip() == p.ip()))
        {
            return Err(AppError::forbidden("broker_protected_target"));
        }
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(25))
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|_| AppError::internal("broker_client_failed"))?;
    Ok((client, url))
}
pub async fn order_policy(pool: &PgPool, a: &Claims) -> AppResult<TaskContract> {
    if a.role != Role::Agent {
        return Err(AppError::forbidden("role_denied"));
    }
    let value:Value=sqlx::query_scalar("SELECT contract->'tool_policy' FROM work_orders WHERE tenant_id=$1 AND task_id=$2 AND id=$3 AND status='ready'").bind(a.tenant_id).bind(a.task_id).bind(a.work_order_id).fetch_optional(pool).await.map_err(db)?.ok_or_else(missing)?;
    if value.is_null() {
        return Ok(TaskContract::default());
    }
    let mut pinned: TaskContract = serde_json::from_value(value).map_err(|_| bad())?;
    let current = workspace::task_contract(pool, a).await?;
    pinned
        .allowed_tools
        .retain(|t| current.allowed_tools.contains(t));
    pinned.urls.retain(|u| {
        current.urls.iter().any(|v| {
            v.url == u.url
                && (!u.allow_private || v.allow_private)
                && (!u.allow_http || v.allow_http)
        })
    });
    pinned
        .secret_refs
        .retain(|id| current.secret_refs.contains(id));
    pinned
        .input_artifacts
        .retain(|id| current.input_artifacts.contains(id));
    Ok(pinned)
}
pub async fn invoke(s: &AppState, a: &Claims, req: &ToolRequest) -> AppResult<Value> {
    let policy = order_policy(&s.pool, a).await?;
    let tool = if req.tool == Capability::ExternalFetch {
        "external.fetch"
    } else {
        "shell.exec"
    };
    if !policy.allowed_tools.iter().any(|t| t == tool) {
        return Err(AppError::forbidden("work_tool_denied"));
    }
    let key: Uuid = serde_json::from_value(req.input["request_id"].clone()).map_err(|_| bad())?;
    let hash = workspace::digest(
        &serde_json::to_vec(&json!({"tool":tool,"input":req.input})).map_err(|_| bad())?,
    );
    let mut tx = s.pool.begin().await.map_err(db)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("tool:{}:{key}", a.work_order_id.ok_or_else(bad)?))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    if let Some(row)=sqlx::query("SELECT request_hash,status,result FROM tool_receipts WHERE tenant_id=$1 AND task_id=$2 AND work_order_id=$3 AND request_key=$4").bind(a.tenant_id).bind(a.task_id).bind(a.work_order_id).bind(key).fetch_optional(&mut *tx).await.map_err(db)? {
        if row.get::<String,_>("request_hash")!=hash {return Err(AppError::new(axum::http::StatusCode::CONFLICT,"tool_idempotency_conflict"));}
        if row.get::<String,_>("status")=="completed" {return Ok(row.get("result"));}
        // These two adapters are observational GET and ephemeral sandbox execution only.
        // Unknown external mutations must never be added to this reconciliation path.
    }
    sqlx::query("INSERT INTO tool_receipts(tenant_id,task_id,work_order_id,request_key,request_hash,tool,status) VALUES($1,$2,$3,$4,$5,$6,'pending') ON CONFLICT DO NOTHING").bind(a.tenant_id).bind(a.task_id).bind(a.work_order_id).bind(key).bind(hash).bind(tool).execute(&mut *tx).await.map_err(db)?;
    let result = if req.tool == Capability::ExternalFetch {
        fetch(s, a, &policy, &req.input, key).await?
    } else {
        shell(s, a, &policy, &req.input, key).await?
    };
    sqlx::query("UPDATE tool_receipts SET status='completed',result=$5 WHERE tenant_id=$1 AND task_id=$2 AND work_order_id=$3 AND request_key=$4").bind(a.tenant_id).bind(a.task_id).bind(a.work_order_id).bind(key).bind(&result).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(result)
}
async fn fetch(
    s: &AppState,
    a: &Claims,
    p: &TaskContract,
    input: &Value,
    key: Uuid,
) -> AppResult<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Fetch {
        request_id: Uuid,
        url: String,
        secret_ref: Option<Uuid>,
    }
    let f: Fetch = serde_json::from_value(input.clone()).map_err(|_| bad())?;
    let _ = f.request_id;
    let grant = p
        .urls
        .iter()
        .find(|g| g.url == f.url)
        .ok_or_else(|| AppError::forbidden("work_url_denied"))?;
    let (client, url) = pinned_client(grant).await?;
    let mut request = client.get(url);
    let mut secret = None;
    if let Some(id) = f.secret_ref {
        if !p.secret_refs.contains(&id) {
            return Err(AppError::forbidden("work_secret_denied"));
        }
        let version:Option<i64>=sqlx::query_scalar("SELECT version FROM secret_refs WHERE tenant_id=$1 AND id=$2 AND enabled AND allowed_urls @> $3::jsonb").bind(a.tenant_id).bind(id).bind(json!([f.url])).fetch_optional(&s.pool).await.map_err(db)?;
        if version.is_none() {
            return Err(AppError::forbidden("work_secret_denied"));
        }
        let record = s
            .vault
            .get::<Secret>(&format!("usersecrets/{id}"))
            .await?
            .ok_or_else(missing)?;
        if Some(record.version as i64) != version {
            return Err(AppError::forbidden("secret_version_unconfirmed"));
        }
        request = request.header(&record.value.header, &record.value.value);
        secret = Some(record.value.value);
        audit(
            &s.pool,
            Some(a.tenant_id),
            Some(a.task_id),
            "agent",
            "secret.used",
            "authorized",
            json!({"secret_ref":id,"work_order_id":a.work_order_id}),
        )
        .await?;
    }
    let response = request
        .send()
        .await
        .map_err(|_| AppError::bad("broker_request_failed"))?;
    let status = response.status().as_u16();
    let mut stream = response.bytes_stream();
    let mut bytes = vec![];
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| AppError::bad("broker_response_interrupted"))?;
        if bytes.len() + chunk.len() > 65536 {
            return Err(AppError::bad("broker_response_budget_exceeded"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = redact_response(&String::from_utf8_lossy(&bytes), secret.as_deref());
    let artifact=workspace::put_artifact(&s.pool,a,&json!({"request_id":workspace::effect_key(key,"fetch"),"name":"fetched.txt","media_type":"text/plain","content":text}),false).await?;
    Ok(
        json!({"status":status,"url":f.url,"content":text,"artifact":artifact["artifact"],"sanitized_sha256":workspace::digest(text.as_bytes())}),
    )
}
async fn shell(
    s: &AppState,
    a: &Claims,
    p: &TaskContract,
    input: &Value,
    key: Uuid,
) -> AppResult<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Shell {
        request_id: Uuid,
        script: String,
        #[serde(default)]
        outputs: Vec<String>,
        #[serde(default = "tool_timeout")]
        timeout_seconds: u64,
    }
    let f: Shell = serde_json::from_value(input.clone()).map_err(|_| bad())?;
    let _ = f.request_id;
    let mut files = vec![];
    for id in &p.input_artifacts {
        let file = workspace::artifact(&s.pool, a, *id).await?;
        files.push(InputFile {
            name: file["artifact"]["name"].as_str().ok_or_else(bad)?.into(),
            content: file["content"]
                .as_str()
                .ok_or_else(|| AppError::bad("worker_input_not_utf8"))?
                .into(),
        });
    }
    let job = Job {
        id: key,
        script: f.script,
        files,
        outputs: f.outputs,
        timeout_seconds: f.timeout_seconds,
    };
    let token = tokio::fs::read_to_string("/run/secrets/worker-token")
        .await
        .map_err(|_| AppError::internal("worker_not_configured"))?;
    let reply = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(40))
        .build()
        .map_err(|_| AppError::internal("worker_client_failed"))?
        .post("http://worker:9090/jobs")
        .bearer_auth(token.trim())
        .json(&job)
        .send()
        .await
        .map_err(|_| AppError::bad("worker_unavailable"))?;
    if !reply.status().is_success() {
        return Err(AppError::bad("worker_job_rejected"));
    }
    let mut result: Value = reply
        .json()
        .await
        .map_err(|_| AppError::bad("worker_response_invalid"))?;
    let mut artifacts = vec![];
    for output in result["outputs"].as_array().ok_or_else(bad)? {
        let name = output["name"].as_str().ok_or_else(bad)?;
        let artifact=workspace::put_artifact(&s.pool,a,&json!({"request_id":workspace::effect_key(key,name),"name":name,"media_type":"text/plain","content":output["content"]}),false).await?;
        artifacts.push(artifact["artifact"].clone());
    }
    result.as_object_mut().ok_or_else(bad)?.remove("outputs");
    result["artifacts"] = json!(artifacts);
    Ok(result)
}
fn tool_timeout() -> u64 {
    20
}
fn redact_response(text: &str, secret: Option<&str>) -> String {
    let mut text = text.to_owned();
    if let Some(value) = secret {
        for part in [Some(value), value.strip_prefix("Bearer ")]
            .into_iter()
            .flatten()
            .filter(|v| !v.is_empty())
        {
            let escaped = serde_json::to_string(part).unwrap_or_default();
            for variant in [
                part.to_owned(),
                STANDARD.encode(part),
                url::form_urlencoded::byte_serialize(part.as_bytes()).collect::<String>(),
                escaped.trim_matches('"').to_owned(),
            ] {
                if !variant.is_empty() {
                    text = text.replace(&variant, "[REDACTED]");
                }
            }
        }
    }
    fn fields(v: &mut Value) {
        match v {
            Value::Object(m) => {
                for (k, v) in m {
                    if matches!(
                        k.to_ascii_lowercase().as_str(),
                        "authorization"
                            | "x-api-key"
                            | "access_token"
                            | "refresh_token"
                            | "api_key"
                            | "password"
                            | "client_secret"
                            | "cookie"
                            | "set-cookie"
                    ) {
                        *v = json!("[REDACTED]");
                    } else {
                        fields(v);
                    }
                }
            }
            Value::Array(a) => {
                for v in a {
                    fields(v);
                }
            }
            _ => {}
        }
    }
    if let Ok(mut value) = serde_json::from_str::<Value>(&text) {
        fields(&mut value);
        text = value.to_string();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn broker_echoes_are_redacted_before_storage() {
        let raw = "owned synthetic credential";
        let echo=json!({"echo":raw,"encoded":STANDARD.encode(raw),"refresh_token":"other sensitive value","data":{"password":"hidden","count":4}}).to_string();
        let safe = redact_response(&echo, Some(raw));
        assert!(!safe.contains(raw));
        assert!(!safe.contains("other sensitive value"));
        assert!(!safe.contains("hidden"));
        assert!(safe.contains("4"));
    }
    #[test]
    fn network_scope_rejects_special_addresses() {
        for s in [
            "127.0.0.1",
            "169.254.169.254",
            "0.0.0.0",
            "::1",
            "::ffff:127.0.0.1",
            "10.0.0.1",
            "100.64.0.1",
            "192.0.2.1",
            "fc00::1",
        ] {
            assert!(!public(s.parse().unwrap()));
        }
        assert!(public("93.184.216.34".parse().unwrap()));
    }
}
