use crate::{
    error::{AppError, AppResult},
    policy::{Claims, Role},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub fn db(_: sqlx::Error) -> AppError {
    AppError::internal("workspace_database_failed")
}
pub fn bad() -> AppError {
    AppError::bad("invalid_workspace_input")
}
pub fn missing() -> AppError {
    AppError::new(axum::http::StatusCode::NOT_FOUND, "workspace_not_found")
}
pub fn nonempty(s: &str, n: usize) -> bool {
    !s.trim().is_empty() && s.len() <= n
}
pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub fn effect_key(run: Uuid, call: &str) -> Uuid {
    let bytes = Sha256::digest(format!("{run}:{call}").as_bytes());
    let mut key = [0u8; 16];
    key.copy_from_slice(&bytes[..16]);
    Uuid::from_bytes(key)
}
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 240
        && !name.starts_with('/')
        && !name.contains('\\')
        && !name.contains(':')
        && !name.chars().any(char::is_control)
        && name
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TaskContract {
    pub model_access: String,
    pub kind: String,
    pub output_format: String,
    pub completion_conditions: Vec<String>,
    pub constraints: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub urls: Vec<UrlGrant>,
    pub secret_refs: Vec<Uuid>,
    pub input_artifacts: Vec<Uuid>,
    pub verification: String,
    pub max_work_orders: u32,
}
#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UrlGrant {
    pub url: String,
    #[serde(default)]
    pub allow_private: bool,
    #[serde(default)]
    pub allow_http: bool,
}
impl TaskContract {
    pub fn validate(&self) -> AppResult<()> {
        if !matches!(self.kind.as_str(), "" | "work" | "administration" | "dream") {
            return Err(bad());
        }
        if !matches!(
            self.model_access.as_str(),
            "" | "external_allowed" | "blocked"
        ) {
            return Err(bad());
        }
        if self.output_format.len() > 500
            || self.verification.len() > 2000
            || self.completion_conditions.len() > 16
            || self.constraints.len() > 16
            || self
                .completion_conditions
                .iter()
                .chain(&self.constraints)
                .any(|s| !nonempty(s, 1000))
            || self.urls.len() > 16
            || self.secret_refs.len() > 8
            || self.input_artifacts.len() > 16
            || self.max_work_orders > 8
            || self.allowed_tools.iter().any(|s| {
                !matches!(
                    s.as_str(),
                    "external.fetch" | "shell.exec" | "artifact.write"
                )
            })
        {
            return Err(bad());
        }
        for grant in &self.urls {
            let url = url::Url::parse(&grant.url).map_err(|_| bad())?;
            if grant.url.len() > 2048
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
                || (url.scheme() != "https" && !(url.scheme() == "http" && grant.allow_http))
                || url.query_pairs().any(|(k, _)| {
                    matches!(
                        k.to_ascii_lowercase().as_str(),
                        "token" | "secret" | "password" | "api_key" | "access_token"
                    )
                })
            {
                return Err(bad());
            }
        }
        Ok(())
    }
}
pub async fn task_contract(pool: &PgPool, a: &Claims) -> AppResult<TaskContract> {
    let value: Value =
        sqlx::query_scalar("SELECT contract FROM tasks WHERE tenant_id=$1 AND id=$2")
            .bind(a.tenant_id)
            .bind(a.task_id)
            .fetch_optional(pool)
            .await
            .map_err(db)?
            .ok_or_else(missing)?;
    serde_json::from_value(value).map_err(|_| bad())
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactInput {
    pub request_id: Uuid,
    pub name: String,
    #[serde(default = "text_type")]
    pub media_type: String,
    pub content: Option<String>,
    pub data_base64: Option<String>,
}
fn text_type() -> String {
    "text/plain".into()
}
pub async fn put_artifact(
    pool: &PgPool,
    a: &Claims,
    input: &Value,
    operator: bool,
) -> AppResult<Value> {
    let mut tx = pool.begin().await.map_err(db)?;
    let result = put_artifact_tx(&mut tx, a, input, operator).await?;
    tx.commit().await.map_err(db)?;
    Ok(result)
}
pub fn artifact_body(r: &ArtifactInput) -> AppResult<Vec<u8>> {
    if !safe_name(&r.name)
        || !nonempty(&r.media_type, 100)
        || r.media_type.chars().any(char::is_control)
    {
        return Err(bad());
    }
    let body = match (&r.content, &r.data_base64) {
        (Some(text), None) => text.as_bytes().to_vec(),
        (None, Some(data)) => STANDARD.decode(data).map_err(|_| bad())?,
        _ => return Err(bad()),
    };
    if body.len() > 1048576 {
        return Err(AppError::bad("artifact_budget_exceeded"));
    }
    Ok(body)
}
pub async fn put_artifact_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    a: &Claims,
    input: &Value,
    operator: bool,
) -> AppResult<Value> {
    if !operator && (a.role != Role::Agent || a.work_order_id.is_none()) {
        return Err(AppError::forbidden("role_denied"));
    }
    let r: ArtifactInput = serde_json::from_value(input.clone()).map_err(|_| bad())?;
    let body = artifact_body(&r)?;
    if operator {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("task-input-files:{}:{}", a.tenant_id, a.task_id))
            .execute(&mut **tx)
            .await
            .map_err(db)?;
        let valid: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tasks WHERE tenant_id=$1 AND id=$2)")
                .bind(a.tenant_id)
                .bind(a.task_id)
                .fetch_one(&mut **tx)
                .await
                .map_err(db)?;
        if !valid {
            return Err(missing());
        }
        let (count, bytes, duplicate): (i64,i64,bool) = sqlx::query_as("SELECT count(*),COALESCE(sum(octet_length(body)),0)::bigint,COALESCE(bool_or(name=$4),false) FROM artifacts WHERE tenant_id=$1 AND task_id=$2 AND work_order_id IS NULL AND request_key<>$3")
            .bind(a.tenant_id).bind(a.task_id).bind(r.request_id).bind(&r.name).fetch_one(&mut **tx).await.map_err(db)?;
        if duplicate {
            return Err(AppError::bad("attachment_duplicate"));
        }
        if count >= 16 || bytes + body.len() as i64 > 4 * 1048576 {
            return Err(AppError::bad("attachment_budget_exceeded"));
        }
    }
    let hash = digest(&body);
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO artifacts(id,tenant_id,task_id,work_order_id,request_key,name,media_type,sha256,body) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(tenant_id,task_id,request_key) DO NOTHING")
        .bind(id).bind(a.tenant_id).bind(a.task_id).bind(if operator{None}else{a.work_order_id}).bind(r.request_id).bind(&r.name).bind(&r.media_type).bind(&hash).bind(body).execute(&mut **tx).await.map_err(db)?;
    let row=sqlx::query("SELECT id,name,media_type,sha256,work_order_id,octet_length(body) AS bytes FROM artifacts WHERE tenant_id=$1 AND task_id=$2 AND request_key=$3")
        .bind(a.tenant_id).bind(a.task_id).bind(r.request_id).fetch_one(&mut **tx).await.map_err(db)?;
    if row.get::<String, _>("sha256") != hash
        || row.get::<String, _>("name") != r.name
        || row.get::<String, _>("media_type") != r.media_type
        || row.get::<Option<Uuid>, _>("work_order_id")
            != if operator { None } else { a.work_order_id }
    {
        return Err(AppError::new(
            axum::http::StatusCode::CONFLICT,
            "artifact_idempotency_conflict",
        ));
    }
    Ok(json!({"artifact":artifact_header(&row),"replayed":row.get::<Uuid,_>("id")!=id}))
}
pub fn artifact_header(row: &sqlx::postgres::PgRow) -> Value {
    json!({"id":row.get::<Uuid,_>("id"),"name":row.get::<String,_>("name"),"media_type":row.get::<String,_>("media_type"),"sha256":row.get::<String,_>("sha256"),"bytes":row.get::<i32,_>("bytes"),"work_order_id":row.get::<Option<Uuid>,_>("work_order_id")})
}
pub async fn artifact(pool: &PgPool, a: &Claims, id: Uuid) -> AppResult<Value> {
    if !matches!(a.role, Role::Handler | Role::Agent)
        || a.role == Role::Agent && a.work_order_id.is_none()
    {
        return Err(AppError::forbidden("role_denied"));
    }
    let row=sqlx::query("SELECT id,name,media_type,sha256,work_order_id,octet_length(body) AS bytes,body FROM artifacts WHERE id=$1 AND tenant_id=$2 AND task_id=$3")
        .bind(id).bind(a.tenant_id).bind(a.task_id).fetch_optional(pool).await.map_err(db)?.ok_or_else(missing)?;
    if a.role == Role::Agent && row.get::<Option<Uuid>, _>("work_order_id") != a.work_order_id {
        let permitted:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM work_orders WHERE tenant_id=$1 AND task_id=$2 AND id=$3 AND contract->'input_artifacts' @> $4::jsonb)")
            .bind(a.tenant_id).bind(a.task_id).bind(a.work_order_id).bind(json!([id])).fetch_one(pool).await.map_err(db)?;
        if !permitted {
            return Err(missing());
        }
    }
    let body: Vec<u8> = row.get("body");
    Ok(
        json!({"artifact":artifact_header(&row),"content":std::str::from_utf8(&body).ok(),"data_base64":STANDARD.encode(body)}),
    )
}
pub async fn read_artifact(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Read {
        artifact_id: Uuid,
        offset: Option<usize>,
        limit: Option<usize>,
    }
    let r: Read = serde_json::from_value(input.clone()).map_err(|_| bad())?;
    if r.offset.is_some_and(|v| v > 1048576) || r.limit.is_some_and(|v| v == 0 || v > 16000) {
        return Err(bad());
    }
    let value = artifact(pool, a, r.artifact_id).await?;
    if r.offset.is_none() && r.limit.is_none() {
        return Ok(value);
    }
    Ok(artifact_preview(
        value,
        r.offset.unwrap_or(0),
        r.limit.unwrap_or(4000),
    ))
}
fn artifact_preview(value: Value, offset: usize, limit: usize) -> Value {
    let content = value["content"].as_str();
    let preview = content.map(|s| s.chars().skip(offset).take(limit).collect::<String>());
    let end = offset + preview.as_ref().map_or(0, |s| s.chars().count());
    let more = content.is_some_and(|s| s.chars().count() > end);
    json!({"artifact":value["artifact"],"content":preview,"text_readable":content.is_some(),"truncated":more,"next_offset":if more {Some(end)}else{None}})
}
pub fn actor(tenant: Uuid, task: Uuid, role: Role) -> Claims {
    Claims {
        iss: "echelon-nano".into(),
        aud: "echelon-core".into(),
        sub: "operator-broker".into(),
        role,
        tenant_id: tenant,
        task_id: task,
        work_order_id: None,
        targets: vec![],
        iat: crate::unix_time(),
        exp: crate::unix_time() + 900,
        jti: Uuid::new_v4(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn previews_preserve_unicode_boundaries_without_dumping_binary_data() {
        let text = artifact_preview(
            json!({"artifact":{"id":"owned"},"content":"가😀나다","data_base64":"unused"}),
            1,
            2,
        );
        assert_eq!(text["content"], "😀나");
        assert_eq!(text["next_offset"], 3);
        assert_eq!(text["truncated"], true);
        assert!(text.get("data_base64").is_none());
        let binary = artifact_preview(
            json!({"artifact":{},"content":null,"data_base64":"AP8="}),
            0,
            4000,
        );
        assert_eq!(binary["text_readable"], false);
        assert!(binary["content"].is_null());
        assert!(binary.get("data_base64").is_none());
    }
    #[test]
    fn paths_and_contracts_are_bounded() {
        for name in ["../secret", "/etc/passwd", "a/../b", "a\\b", "a:b", ""] {
            assert!(!safe_name(name));
        }
        assert!(safe_name("src/main.py"));
        let c = TaskContract {
            allowed_tools: vec!["vault.raw_read".into()],
            ..Default::default()
        };
        assert!(c.validate().is_err());
        assert_eq!(effect_key(Uuid::nil(), "x"), effect_key(Uuid::nil(), "x"));
    }
}
