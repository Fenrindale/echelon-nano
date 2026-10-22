//! Credential bytes stay inside the core/Vault boundary, never in role tool results.
use crate::{
    broker,
    error::{AppError, AppResult},
    policy::Claims,
    server::AppState,
    workspace::{self, CredentialBinding, TaskContract, UrlGrant, bad, db, missing},
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;
use uuid::Uuid;

fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .enumerate()
            .all(|(i, c)| c == b'_' || c.is_ascii_alphabetic() || i > 0 && c.is_ascii_digit())
}
fn secret_name(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    [
        "PASSWORD",
        "SECRET",
        "TOKEN",
        "ACCESS_KEY",
        "API_KEY",
        "PRIVATE_KEY",
    ]
    .iter()
    .any(|v| name.contains(v))
        || name.ends_with("_PASS")
}
pub fn sensitive(name: &str, text: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "env.txt"
        || name == ".env"
        || name.ends_with(".env")
        || name.starts_with(".env.")
        || text.contains("PRIVATE KEY-----")
        || parse(text).is_ok_and(|entries| entries.keys().any(|key| secret_name(key)))
}
// This is a literal parser, not a shell or dotenv environment expander.
fn parse(text: &str) -> AppResult<BTreeMap<String, String>> {
    let mut entries = BTreeMap::new();
    if text.len() > 1048576 {
        return Err(AppError::bad("credential_input_too_large"));
    }
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let (name, value) = line
            .split_once('=')
            .ok_or_else(|| AppError::bad("credential_format_unsupported"))?;
        let name = name.trim();
        if !identifier(name) || entries.contains_key(name) || entries.len() >= 256 {
            return Err(AppError::bad("credential_assignment_invalid"));
        }
        let value = value.trim();
        let value = if value.starts_with(['\'', '"']) {
            let quote = value.chars().next().ok_or_else(bad)?;
            if value.len() < 2 || !value.ends_with(quote) {
                return Err(AppError::bad("credential_format_unsupported"));
            }
            &value[1..value.len() - 1]
        } else {
            value.split_once(" #").map_or(value, |(v, _)| v).trim_end()
        };
        if value.len() > 16384 || value.chars().any(char::is_control) {
            return Err(AppError::bad("credential_value_unsupported"));
        }
        entries.insert(name.to_string(), value.to_string());
    }
    if entries.is_empty() {
        return Err(AppError::bad("credential_assignments_missing"));
    }
    Ok(entries)
}
fn manifest(entries: &BTreeMap<String, String>) -> Value {
    json!({"format":"literal_env","entry_names":entries.keys().collect::<Vec<_>>(),"entry_count":entries.len(),
        "empty_names":entries.iter().filter(|(_,v)|v.is_empty()).map(|(k,_)|k).collect::<Vec<_>>(),
        "sensitive_field_count":entries.keys().filter(|k|secret_name(k)).count(),"targets":targets(entries),"values_withheld":true})
}
fn target_url(value: &str) -> Option<String> {
    let url = url::Url::parse(value).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none())
    .then(|| value.trim_end_matches('/').to_string())
}
fn targets(entries: &BTreeMap<String, String>) -> Vec<Value> {
    entries
        .iter()
        .filter_map(|(key, value)| {
            let upper = key.to_ascii_uppercase();
            if secret_name(key)
                || !(upper.ends_with("URL") || upper.ends_with("HOST") || upper.ends_with("URI"))
            {
                return None;
            }
            let url = target_url(value)?;
            if entries.iter().any(|(name, secret)| {
                secret_name(name) && secret.len() >= 4 && url.contains(secret)
            }) {
                return None;
            }
            let kind = if upper.contains("JENKINS") {
                Some("jenkins")
            } else if upper.contains("GITLAB") {
                Some("gitlab")
            } else if upper.contains("DATABRICKS") {
                Some("databricks")
            } else {
                None
            };
            Some(json!({"target_variable":key,"base_url":url,"kind":kind}))
        })
        .take(16)
        .collect()
}
pub fn validate_binding(r: &CredentialBinding) -> AppResult<()> {
    if !matches!(r.kind.as_str(), "gitlab" | "jenkins" | "databricks")
        || !identifier(&r.token_variable)
        || r.kind == "jenkins" && !identifier(&r.username_variable)
        || r.base_url.len() > 1024
        || target_url(&r.base_url).is_none()
        || r.base_url.starts_with("http:") && !r.allow_http
        || r.source_artifact_id.is_some() != r.target_variable.is_some()
        || r.target_variable
            .as_ref()
            .is_some_and(|v| !identifier(v) || secret_name(v))
    {
        return Err(AppError::bad("credential_binding_invalid"));
    }
    Ok(())
}
pub async fn validate_task_bindings(
    pool: &PgPool,
    tenant: Uuid,
    task: Uuid,
    policy: &TaskContract,
) -> AppResult<()> {
    if policy.credential_bindings.is_empty() {
        return Ok(());
    }
    let goal: String = sqlx::query_scalar("SELECT goal FROM tasks WHERE tenant_id=$1 AND id=$2")
        .bind(tenant)
        .bind(task)
        .fetch_one(pool)
        .await
        .map_err(db)?;
    for binding in &policy.credential_bindings {
        validate_binding(binding)?;
        if goal.split_whitespace().any(|part| {
            target_url(part.trim_matches(|c: char| "`\"'()<>,.;[]{}".contains(c)))
                .is_some_and(|url| url == binding.base_url.trim_end_matches('/'))
        }) {
            continue;
        }
        let source = binding
            .source_artifact_id
            .ok_or_else(|| AppError::forbidden("credential_target_not_in_task"))?;
        let input = policy.input_artifacts.contains(&source);
        let reusable:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM credential_bundles WHERE tenant_id=$1 AND id=ANY($2) AND source_artifact_id=$3)").bind(tenant).bind(&policy.credential_bundles).bind(source).fetch_one(pool).await.map_err(db)?;
        let row = sqlx::query("SELECT body,task_id FROM artifacts WHERE tenant_id=$1 AND id=$2")
            .bind(tenant)
            .bind(source)
            .fetch_optional(pool)
            .await
            .map_err(db)?
            .ok_or_else(missing)?;
        if !(input && row.get::<Uuid, _>("task_id") == task || reusable) {
            return Err(missing());
        }
        let bytes: Vec<u8> = row.get("body");
        let entries = parse(std::str::from_utf8(&bytes).map_err(|_| bad())?)?;
        if !targets(&entries).iter().any(|target| {
            target["target_variable"].as_str() == binding.target_variable.as_deref()
                && target["base_url"].as_str() == Some(binding.base_url.trim_end_matches('/'))
        }) {
            return Err(AppError::forbidden("credential_target_not_in_task"));
        }
    }
    Ok(())
}
pub fn model_view(value: &Value) -> Option<Value> {
    let name = value["artifact"]["name"].as_str().unwrap_or("");
    let text = value["content"].as_str().unwrap_or("");
    if !sensitive(name, text) {
        return None;
    }
    let info = parse(text)
        .map(|m| manifest(&m))
        .unwrap_or_else(|_| json!({"values_withheld":true,"format":"unsupported"}));
    Some(
        json!({"artifact":value["artifact"],"content":null,"text_readable":false,"truncated":false,"next_offset":null,
        "credential_material":true,"credential_manifest":info,"hint":"Secret values are withheld. Use credential.inspect and delegate credential.import; do not use shell or raw downloads to inspect credentials."}),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactRequest {
    artifact_id: Uuid,
    request_id: Option<Uuid>,
}
pub async fn inspect(s: &AppState, a: &Claims, input: &Value) -> AppResult<Value> {
    if let Some(bundle) = input.get("bundle_id") {
        if input.as_object().is_none_or(|o| o.len() != 1) {
            return Err(bad());
        }
        let id: Uuid = serde_json::from_value(bundle.clone()).map_err(|_| bad())?;
        if a.role == crate::policy::Role::Agent
            && !broker::order_policy(&s.pool, a)
                .await?
                .credential_bundles
                .contains(&id)
        {
            return Err(missing());
        }
        let row=sqlx::query("SELECT b.source_artifact_id,b.source_sha256,a.body FROM credential_bundles b JOIN artifacts a ON a.tenant_id=b.tenant_id AND a.id=b.source_artifact_id JOIN tasks t ON t.tenant_id=b.tenant_id AND t.id=b.source_task_id WHERE b.tenant_id=$1 AND b.id=$2 AND COALESCE(t.contract->>'model_access','')<>'blocked'").bind(a.tenant_id).bind(id).fetch_optional(&s.pool).await.map_err(db)?.ok_or_else(missing)?;
        let bytes: Vec<u8> = row.get("body");
        if workspace::digest(&bytes) != row.get::<String, _>("source_sha256") {
            return Err(AppError::bad("credential_version_unconfirmed"));
        }
        let entries = parse(std::str::from_utf8(&bytes).map_err(|_| bad())?)?;
        let configuration: BTreeMap<_, _> = entries
            .iter()
            .filter(|(k, _)| !secret_name(k) && (k.ends_with("_REGION") || k.ends_with("_MODE")))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        return Ok(
            json!({"bundle_id":id,"source_artifact_id":row.get::<Uuid,_>("source_artifact_id"),"manifest":manifest(&entries),"configuration":configuration}),
        );
    }
    let r: ArtifactRequest = serde_json::from_value(input.clone()).map_err(|_| bad())?;
    let value = workspace::artifact(&s.pool, a, r.artifact_id).await?;
    let text = value["content"]
        .as_str()
        .ok_or_else(|| AppError::bad("credential_format_unsupported"))?;
    Ok(json!({"artifact":value["artifact"],"manifest":manifest(&parse(text)?)}))
}
pub async fn mcp_environment(
    s: &AppState,
    tenant: Uuid,
    bindings: &[crate::mcp::EnvironmentBinding],
) -> AppResult<BTreeMap<String, String>> {
    let mut environment = BTreeMap::new();
    for binding in bindings {
        let row=sqlx::query("SELECT source_artifact_id,source_sha256,vault_version FROM credential_bundles WHERE tenant_id=$1 AND id=$2").bind(tenant).bind(binding.bundle_id).fetch_optional(&s.pool).await.map_err(db)?.ok_or_else(missing)?;
        let stored = s
            .vault
            .get::<Bundle>(&format!("usersecrets/{}", binding.bundle_id))
            .await?
            .ok_or_else(missing)?;
        if stored.value.tenant_id != tenant
            || stored.version as i64 != row.get::<i64, _>("vault_version")
            || stored.value.source_artifact_id != row.get::<Uuid, _>("source_artifact_id")
            || stored.value.source_sha256 != row.get::<String, _>("source_sha256")
        {
            return Err(AppError::bad("credential_version_unconfirmed"));
        }
        let value = stored
            .value
            .variables
            .get(&binding.variable)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::bad("mcp_credential_variable_missing"))?;
        if environment
            .insert(binding.name.clone(), value.clone())
            .is_some()
        {
            return Err(bad());
        }
    }
    Ok(environment)
}
#[derive(Serialize, Deserialize, PartialEq)]
struct Bundle {
    tenant_id: Uuid,
    source_artifact_id: Uuid,
    source_sha256: String,
    variables: BTreeMap<String, String>,
}
fn bundle_header(row: &sqlx::postgres::PgRow) -> Value {
    json!({"id":row.get::<Uuid,_>("id"),"name":row.get::<String,_>("name"),"source_task_id":row.get::<Uuid,_>("source_task_id"),
        "source_artifact_id":row.get::<Uuid,_>("source_artifact_id"),"source_sha256":row.get::<String,_>("source_sha256"),
        "entry_names":row.get::<Value,_>("entry_names"),"empty_names":row.get::<Value,_>("empty_names"),
        "vault_version":row.get::<i64,_>("vault_version"),"storage_verified":true,"authentication":"unverified"})
}
async fn storage_receipt(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    a: &Claims,
    id: Uuid,
    artifact: Uuid,
    version: u64,
    replayed: bool,
    request_id: Option<Uuid>,
) -> AppResult<Value> {
    let details = json!({"bundle_id":id,"artifact_id":artifact,"work_order_id":a.work_order_id,"vault_version":version,"vault_roundtrip_verified":true,"replayed":replayed,"request_id":request_id});
    let seq:i64=sqlx::query_scalar("INSERT INTO audit_records(tenant_id,task_id,actor_role,action,outcome,details) VALUES($1,$2,'agent',$3,'success',$4) RETURNING seq")
        .bind(a.tenant_id).bind(a.task_id).bind(if replayed{"credential.storage_verified"}else{"credential.imported"}).bind(&details).fetch_one(&mut **tx).await.map_err(db)?;
    Ok(json!({"audit_seq":seq,"details":details}))
}
pub async fn work_evidence(pool: &PgPool, a: &Claims, order: Uuid) -> AppResult<Value> {
    let imports=sqlx::query("SELECT seq,details,created_at::text AS observed_at FROM audit_records WHERE tenant_id=$1 AND task_id=$2 AND actor_role='agent' AND action IN ('credential.imported','credential.storage_verified') AND details->>'work_order_id'=$3 ORDER BY seq DESC LIMIT 32")
        .bind(a.tenant_id).bind(a.task_id).bind(order.to_string()).fetch_all(pool).await.map_err(db)?;
    let checks:Vec<Value>=sqlx::query_scalar("SELECT result FROM credential_checks WHERE tenant_id=$1 AND task_id=$2 AND work_order_id=$3 UNION ALL SELECT result FROM task_credential_checks WHERE tenant_id=$1 AND task_id=$2 AND work_order_id=$3 LIMIT 32")
        .bind(a.tenant_id).bind(a.task_id).bind(order).fetch_all(pool).await.map_err(db)?;
    Ok(
        json!({"credential_imports":imports.iter().map(|row|json!({"audit_seq":row.get::<i64,_>("seq"),"details":row.get::<Value,_>("details"),"observed_at":row.get::<String,_>("observed_at")})).collect::<Vec<_>>(),"credential_checks":checks,"source":"server_records"}),
    )
}
pub async fn import(s: &AppState, a: &Claims, input: &Value) -> AppResult<Value> {
    let policy = broker::order_policy(&s.pool, a).await?;
    if !policy
        .allowed_tools
        .iter()
        .any(|t| t == "credential.import")
    {
        return Err(AppError::forbidden("work_tool_denied"));
    }
    let r: ArtifactRequest = serde_json::from_value(input.clone()).map_err(|_| bad())?;
    if !policy.input_artifacts.contains(&r.artifact_id) {
        return Err(missing());
    }
    let value = workspace::artifact(&s.pool, a, r.artifact_id).await?;
    let variables = parse(
        value["content"]
            .as_str()
            .ok_or_else(|| AppError::bad("credential_format_unsupported"))?,
    )?;
    let info = manifest(&variables);
    let id = workspace::effect_key(r.artifact_id, "credential-bundle");
    let expected = Bundle {
        tenant_id: a.tenant_id,
        source_artifact_id: r.artifact_id,
        source_sha256: value["artifact"]["sha256"].as_str().ok_or_else(bad)?.into(),
        variables,
    };
    let path = format!("usersecrets/{id}");
    let mut tx = s.pool.begin().await.map_err(db)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("credential-import:{}:{id}", a.tenant_id))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let existing = sqlx::query("SELECT * FROM credential_bundles WHERE tenant_id=$1 AND id=$2")
        .bind(a.tenant_id)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;
    let version = match s.vault.get::<Bundle>(&path).await? {
        Some(stored) if stored.value == expected => stored.version,
        Some(_) => return Err(AppError::bad("credential_version_unconfirmed")),
        None if existing.is_some() => return Err(AppError::bad("credential_vault_record_missing")),
        None => s.vault.put(&path, &expected, 0).await?,
    };
    let proof = s.vault.get::<Bundle>(&path).await?.ok_or_else(missing)?;
    if proof.version != version || proof.value != expected {
        return Err(AppError::bad("credential_roundtrip_failed"));
    }
    if let Some(row) = existing {
        if row.get::<i64, _>("vault_version") != version as i64 {
            return Err(AppError::bad("credential_version_unconfirmed"));
        }
        let receipt =
            storage_receipt(&mut tx, a, id, r.artifact_id, version, true, r.request_id).await?;
        tx.commit().await.map_err(db)?;
        return Ok(
            json!({"bundle":bundle_header(&row),"replayed":true,"vault_roundtrip_verified":true,"storage_receipt":receipt}),
        );
    }
    let row=sqlx::query("INSERT INTO credential_bundles(id,tenant_id,source_task_id,source_artifact_id,source_sha256,name,entry_names,empty_names,vault_version) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING *")
        .bind(id).bind(a.tenant_id).bind(a.task_id).bind(r.artifact_id).bind(&expected.source_sha256).bind(value["artifact"]["name"].as_str().ok_or_else(bad)?)
        .bind(&info["entry_names"]).bind(&info["empty_names"]).bind(version as i64).fetch_one(&mut *tx).await.map_err(db)?;
    let receipt =
        storage_receipt(&mut tx, a, id, r.artifact_id, version, false, r.request_id).await?;
    tx.commit().await.map_err(db)?;
    Ok(
        json!({"bundle":bundle_header(&row),"replayed":false,"vault_roundtrip_verified":true,"storage_receipt":receipt}),
    )
}
pub async fn catalog(pool: &PgPool, tenant: Uuid) -> AppResult<Value> {
    let bundles = sqlx::query(
        "SELECT * FROM credential_bundles WHERE tenant_id=$1 ORDER BY created_at DESC LIMIT 32",
    )
    .bind(tenant)
    .fetch_all(pool)
    .await
    .map_err(db)?;
    let connectors=sqlx::query("SELECT to_jsonb(c) AS value FROM credential_connectors c WHERE tenant_id=$1 ORDER BY name LIMIT 32").bind(tenant).fetch_all(pool).await.map_err(db)?;
    let checks=sqlx::query("SELECT result,created_at::text AS checked_at FROM (SELECT result,created_at FROM credential_checks WHERE tenant_id=$1 UNION ALL SELECT result,created_at FROM task_credential_checks WHERE tenant_id=$1) checks ORDER BY created_at DESC LIMIT 100").bind(tenant).fetch_all(pool).await.map_err(db)?;
    Ok(
        json!({"bundles":bundles.iter().map(bundle_header).collect::<Vec<_>>(),"connectors":connectors.iter().map(|r|r.get::<Value,_>("value")).collect::<Vec<_>>(),"checks":checks.iter().map(|r|json!({"result":r.get::<Value,_>("result"),"checked_at":r.get::<String,_>("checked_at")})).collect::<Vec<_>>(),
        "supported_identity_connectors":["gitlab","jenkins","databricks"],"scope":"Handler selects Task-local identity bindings from the user goal or selected configuration inputs. Workspace connection profiles are optional. Work Orders freeze the selected scope; Vault storage is separate from service authentication."}),
    )
}
pub async fn model_catalog(pool: &PgPool, tenant: Uuid) -> AppResult<Value> {
    let mut value = catalog(pool, tenant).await?;
    let checks = value
        .as_object_mut()
        .ok_or_else(bad)?
        .remove("checks")
        .unwrap_or(json!([]));
    let restricted: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM tasks WHERE tenant_id=$1 AND contract->>'model_access'='blocked'",
    )
    .bind(tenant)
    .fetch_all(pool)
    .await
    .map_err(db)?;
    if let Some(bundles) = value["bundles"].as_array_mut() {
        bundles.retain(|b| {
            serde_json::from_value::<Uuid>(b["source_task_id"].clone())
                .is_ok_and(|id| !restricted.contains(&id))
        });
        bundles.truncate(8);
        for bundle in bundles {
            let count = bundle["entry_names"].as_array().map_or(0, Vec::len);
            bundle["entry_count"] = json!(count);
            if let Some(names) = bundle["entry_names"].as_array_mut() {
                names.truncate(32);
            }
            if let Some(names) = bundle["empty_names"].as_array_mut() {
                names.truncate(32);
            }
        }
    }
    if let Some(connectors) = value["connectors"].as_array_mut() {
        connectors.retain(|c| c["enabled"] == true);
        connectors.truncate(16);
    }
    let ids: Vec<Value> = value["bundles"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|b| b["id"].clone())
        .collect();
    value["recent_bindings"]=json!(checks.as_array().into_iter().flatten().filter_map(|check| {
        let result=&check["result"];
        (ids.contains(&result["bundle_id"]) && result["binding_snapshot"].is_object()).then(||json!({"bundle_id":result["bundle_id"],"binding":result["binding_snapshot"]}))
    }).take(8).collect::<Vec<_>>());
    Ok(value)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorInput {
    pub tenant_id: Uuid,
    pub id: Option<Uuid>,
    pub expected_version: Option<i64>,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    #[serde(default)]
    pub username_variable: String,
    pub token_variable: String,
    #[serde(default)]
    pub allow_private: bool,
    #[serde(default)]
    pub allow_http: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
}
fn yes() -> bool {
    true
}
pub async fn save_connector(s: &AppState, r: ConnectorInput) -> AppResult<Value> {
    let url = url::Url::parse(&r.base_url).map_err(|_| bad())?;
    if !workspace::nonempty(&r.name, 160)
        || !matches!(r.kind.as_str(), "gitlab" | "jenkins" | "databricks")
        || !identifier(&r.token_variable)
        || !r.token_variable.to_ascii_uppercase().contains("TOKEN")
        || r.kind == "jenkins" && !identifier(&r.username_variable)
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
        || r.base_url.len() > 1024
        || !(url.scheme() == "https" || url.scheme() == "http" && r.allow_http)
    {
        return Err(AppError::bad("credential_connector_invalid"));
    }
    let id = r.id.unwrap_or_else(Uuid::new_v4);
    let mut tx = s.pool.begin().await.map_err(db)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("credential-connector:{id}"))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let version: Option<i64> = sqlx::query_scalar(
        "SELECT version FROM credential_connectors WHERE tenant_id=$1 AND id=$2",
    )
    .bind(r.tenant_id)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?;
    if version.unwrap_or(0) != r.expected_version.unwrap_or(0)
        || r.id.is_some() && version.is_none()
    {
        return Err(AppError::bad("credential_connector_changed"));
    }
    let next = version.unwrap_or(0) + 1;
    sqlx::query("INSERT INTO credential_connectors(id,tenant_id,name,kind,base_url,username_variable,token_variable,allow_private,allow_http,enabled,version) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT(id) DO UPDATE SET name=EXCLUDED.name,kind=EXCLUDED.kind,base_url=EXCLUDED.base_url,username_variable=EXCLUDED.username_variable,token_variable=EXCLUDED.token_variable,allow_private=EXCLUDED.allow_private,allow_http=EXCLUDED.allow_http,enabled=EXCLUDED.enabled,version=EXCLUDED.version")
        .bind(id).bind(r.tenant_id).bind(r.name).bind(r.kind).bind(r.base_url.trim_end_matches('/')).bind(r.username_variable).bind(r.token_variable).bind(r.allow_private).bind(r.allow_http).bind(r.enabled).bind(next).execute(&mut *tx).await.map_err(db)?;
    sqlx::query("INSERT INTO audit_records(tenant_id,actor_role,action,outcome,details) VALUES($1,'operator','credential.connector_registered','success',$2)").bind(r.tenant_id).bind(json!({"connector_id":id,"version":next})).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(json!({"connector_id":id,"version":next}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Check {
    bundle_id: Uuid,
    connector_id: Option<Uuid>,
    binding_index: Option<usize>,
    request_id: Option<Uuid>,
}
pub async fn check(s: &AppState, a: &Claims, input: &Value) -> AppResult<Value> {
    let p = broker::order_policy(&s.pool, a).await?;
    if !p.allowed_tools.iter().any(|t| t == "credential.check") {
        return Err(AppError::forbidden("work_tool_denied"));
    }
    let r: Check = serde_json::from_value(input.clone()).map_err(|_| bad())?;
    if let Some(index) = r.binding_index {
        if r.connector_id.is_some() {
            return Err(bad());
        }
        return check_binding(s, a, &p, &r, index).await;
    }
    let connector_id = r.connector_id.ok_or_else(bad)?;
    let grant = p
        .credential_connectors
        .iter()
        .find(|g| g.id == connector_id)
        .ok_or_else(|| AppError::forbidden("credential_connector_not_granted"))?;
    let bundle = sqlx::query("SELECT * FROM credential_bundles WHERE tenant_id=$1 AND id=$2")
        .bind(a.tenant_id)
        .bind(r.bundle_id)
        .fetch_optional(&s.pool)
        .await
        .map_err(db)?
        .ok_or_else(missing)?;
    let own_input = bundle.get::<Uuid, _>("source_task_id") == a.task_id
        && p.input_artifacts
            .contains(&bundle.get("source_artifact_id"));
    if !own_input && !p.credential_bundles.contains(&r.bundle_id) {
        return Err(missing());
    }
    let mut tx = s.pool.begin().await.map_err(db)?;
    let connector = sqlx::query(
        "SELECT * FROM credential_connectors WHERE tenant_id=$1 AND id=$2 AND enabled FOR SHARE",
    )
    .bind(a.tenant_id)
    .bind(connector_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?
    .ok_or_else(missing)?;
    if connector.get::<i64, _>("version") != grant.version {
        return Err(AppError::bad("credential_connector_changed"));
    }
    let lock = format!(
        "credential-check:{:?}:{}:{}:{}",
        a.work_order_id, r.bundle_id, connector_id, grant.version
    );
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    if let Some(result)=sqlx::query_scalar::<_,Value>("SELECT result FROM credential_checks WHERE tenant_id=$1 AND work_order_id=$2 AND bundle_id=$3 AND connector_id=$4 AND connector_version=$5").bind(a.tenant_id).bind(a.work_order_id).bind(r.bundle_id).bind(connector_id).bind(grant.version).fetch_optional(&mut *tx).await.map_err(db)? {return Ok(result);}
    let stored = s
        .vault
        .get::<Bundle>(&format!("usersecrets/{}", r.bundle_id))
        .await?
        .ok_or_else(missing)?;
    if stored.value.tenant_id != a.tenant_id
        || stored.version != bundle.get::<i64, _>("vault_version") as u64
    {
        return Err(AppError::bad("credential_version_unconfirmed"));
    }
    let kind: String = connector.get("kind");
    let base: String = connector.get("base_url");
    let suffix = match kind.as_str() {
        "gitlab" => "api/v4/user",
        "jenkins" => "whoAmI/api/json",
        "databricks" => "api/2.0/preview/scim/v2/Me",
        _ => return Err(bad()),
    };
    let url = format!("{base}/{suffix}");
    let url_grant = UrlGrant {
        url,
        allow_private: connector.get("allow_private"),
        allow_http: connector.get("allow_http"),
    };
    let token = stored
        .value
        .variables
        .get(&connector.get::<String, _>("token_variable"))
        .filter(|v| !v.is_empty());
    let user = stored
        .value
        .variables
        .get(&connector.get::<String, _>("username_variable"));
    let observation = if token.is_none() || kind == "jenkins" && user.is_none_or(|v| v.is_empty()) {
        json!({"status":"missing_fields","authenticated":false,"attempted":false})
    } else {
        identity_request(
            &url_grant,
            &kind,
            token.ok_or_else(bad)?,
            user.map(String::as_str),
        )
        .await
    };
    let id = Uuid::new_v4();
    let result = json!({"check_id":id,"bundle_id":r.bundle_id,"connector_id":connector_id,"connector_version":grant.version,"connector_snapshot":{"kind":kind,"base_url":base,"username_variable":connector.get::<String,_>("username_variable"),"token_variable":connector.get::<String,_>("token_variable"),"allow_private":url_grant.allow_private,"allow_http":url_grant.allow_http},"observation":observation});
    sqlx::query("INSERT INTO credential_checks(id,tenant_id,task_id,work_order_id,bundle_id,connector_id,connector_version,result) VALUES($1,$2,$3,$4,$5,$6,$7,$8)").bind(id).bind(a.tenant_id).bind(a.task_id).bind(a.work_order_id).bind(r.bundle_id).bind(connector_id).bind(grant.version).bind(&result).execute(&mut *tx).await.map_err(db)?;
    sqlx::query("INSERT INTO audit_records(tenant_id,task_id,actor_role,action,outcome,details) VALUES($1,$2,'agent','credential.checked',$3,$4)").bind(a.tenant_id).bind(a.task_id).bind(observation["status"].as_str().unwrap_or("unverified")).bind(json!({"check_id":id,"bundle_id":r.bundle_id,"connector_id":connector_id,"request_id":r.request_id})).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(result)
}
async fn check_binding(
    s: &AppState,
    a: &Claims,
    p: &TaskContract,
    r: &Check,
    index: usize,
) -> AppResult<Value> {
    let binding = p
        .credential_bindings
        .get(index)
        .ok_or_else(|| AppError::forbidden("credential_binding_not_in_work_order"))?;
    let current = workspace::task_contract(&s.pool, a).await?;
    if current
        .credential_bindings
        .get(index)
        .is_none_or(|v| json!(v) != json!(binding))
    {
        return Err(AppError::forbidden("credential_binding_changed"));
    }
    validate_task_bindings(&s.pool, a.tenant_id, a.task_id, &current).await?;
    let row=sqlx::query("SELECT b.* FROM credential_bundles b JOIN tasks t ON t.tenant_id=b.tenant_id AND t.id=b.source_task_id WHERE b.tenant_id=$1 AND b.id=$2 AND COALESCE(t.contract->>'model_access','')<>'blocked'").bind(a.tenant_id).bind(r.bundle_id).fetch_optional(&s.pool).await.map_err(db)?.ok_or_else(missing)?;
    if !(row.get::<Uuid, _>("source_task_id") == a.task_id
        && p.input_artifacts.contains(&row.get("source_artifact_id"))
        || p.credential_bundles.contains(&r.bundle_id))
    {
        return Err(missing());
    }
    let fingerprint = workspace::digest(&serde_json::to_vec(binding).map_err(|_| bad())?);
    let mut tx = s.pool.begin().await.map_err(db)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!(
            "task-identity:{:?}:{}:{index}:{fingerprint}",
            a.work_order_id, r.bundle_id
        ))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    if let Some(result)=sqlx::query_scalar::<_,Value>("SELECT result FROM task_credential_checks WHERE tenant_id=$1 AND work_order_id=$2 AND bundle_id=$3 AND binding_index=$4 AND binding_sha256=$5").bind(a.tenant_id).bind(a.work_order_id).bind(r.bundle_id).bind(index as i32).bind(&fingerprint).fetch_optional(&mut *tx).await.map_err(db)?{return Ok(result);}
    let stored = s
        .vault
        .get::<Bundle>(&format!("usersecrets/{}", r.bundle_id))
        .await?
        .ok_or_else(missing)?;
    if stored.value.tenant_id != a.tenant_id
        || stored.version != row.get::<i64, _>("vault_version") as u64
    {
        return Err(AppError::bad("credential_version_unconfirmed"));
    }
    let token = stored
        .value
        .variables
        .get(&binding.token_variable)
        .filter(|v| !v.is_empty());
    let user = stored.value.variables.get(&binding.username_variable);
    let suffix = match binding.kind.as_str() {
        "gitlab" => "api/v4/user",
        "jenkins" => "whoAmI/api/json",
        "databricks" => "api/2.0/preview/scim/v2/Me",
        _ => return Err(bad()),
    };
    let observation =
        if token.is_none() || binding.kind == "jenkins" && user.is_none_or(|v| v.is_empty()) {
            json!({"status":"missing_fields","authenticated":false,"attempted":false})
        } else {
            identity_request(
                &UrlGrant {
                    url: format!("{}/{suffix}", binding.base_url.trim_end_matches('/')),
                    allow_private: binding.allow_private,
                    allow_http: binding.allow_http,
                },
                &binding.kind,
                token.ok_or_else(bad)?,
                user.map(String::as_str),
            )
            .await
        };
    let id = Uuid::new_v4();
    let result = json!({"check_id":id,"bundle_id":r.bundle_id,"binding_index":index,"binding_sha256":fingerprint,"binding_snapshot":binding,"observation":observation});
    sqlx::query("INSERT INTO task_credential_checks(id,tenant_id,task_id,work_order_id,bundle_id,binding_index,binding_sha256,result) VALUES($1,$2,$3,$4,$5,$6,$7,$8)").bind(id).bind(a.tenant_id).bind(a.task_id).bind(a.work_order_id).bind(r.bundle_id).bind(index as i32).bind(&fingerprint).bind(&result).execute(&mut *tx).await.map_err(db)?;
    sqlx::query("INSERT INTO audit_records(tenant_id,task_id,actor_role,action,outcome,details) VALUES($1,$2,'agent','credential.checked',$3,$4)").bind(a.tenant_id).bind(a.task_id).bind(observation["status"].as_str().unwrap_or("unverified")).bind(json!({"check_id":id,"bundle_id":r.bundle_id,"binding_index":index,"binding_sha256":fingerprint,"request_id":r.request_id})).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(result)
}
async fn identity_request(grant: &UrlGrant, kind: &str, token: &str, user: Option<&str>) -> Value {
    let Ok((client, url)) = broker::pinned_client(grant).await else {
        return json!({"status":"target_blocked","authenticated":false,"attempted":false});
    };
    let request = client.get(url);
    let request = match kind {
        "gitlab" => request.header("PRIVATE-TOKEN", token),
        "jenkins" => request.basic_auth(user.unwrap_or(""), Some(token)),
        _ => request.bearer_auth(token),
    };
    let Ok(response) = request.send().await else {
        return json!({"status":"unavailable","authenticated":false,"attempted":true});
    };
    let status = response.status().as_u16();
    if status != 200 {
        return json!({"status":if status==401{"authentication_rejected"}else if status==403{"access_denied"}else{"unverified"},"http_status":status,"authenticated":false,"attempted":true});
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            return json!({"status":"response_interrupted","authenticated":false,"attempted":true});
        };
        if bytes.len() + chunk.len() > 65536 {
            return json!({"status":"response_too_large","authenticated":false,"attempted":true});
        }
        bytes.extend_from_slice(&chunk);
    }
    let identity: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let authenticated = identity_matches(kind, &identity);
    json!({"status":if authenticated{"verified"}else{"unverified"},"http_status":status,"authenticated":authenticated,"attempted":true})
}
fn identity_matches(kind: &str, value: &Value) -> bool {
    match kind {
        "gitlab" => {
            value["id"].as_u64().is_some_and(|v| v > 0)
                && value["username"].as_str().is_some_and(|v| !v.is_empty())
        }
        "jenkins" => {
            value["authenticated"] == true
                && value["name"]
                    .as_str()
                    .is_some_and(|v| !v.is_empty() && v != "anonymous")
        }
        "databricks" => {
            value["id"].as_str().is_some_and(|v| !v.is_empty())
                && value["userName"].as_str().is_some_and(|v| !v.is_empty())
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_bindings_use_configuration_targets_without_exposing_credentials() {
        let entries=parse("JENKINS_URL=https://ci.example/jenkins/\nJENKINS_USER=owned\nPAT=opaque\nOTHER_URL=https://host.example?token=private\nBAD_HOST=https://user:password@host.example\nAPI_TOKEN=hidden-token-value\nLEAK_URL=https://host.example/hidden-token-value\n").unwrap();
        let target_list = targets(&entries);
        assert_eq!(target_list.len(), 1);
        assert_eq!(target_list[0]["base_url"], "https://ci.example/jenkins");
        assert_eq!(target_list[0]["kind"], "jenkins");
        let mut binding:CredentialBinding=serde_json::from_value(json!({"kind":"jenkins","base_url":"https://ci.example/jenkins","username_variable":"JENKINS_USER","token_variable":"PAT"})).unwrap();
        assert!(validate_binding(&binding).is_ok());
        binding.kind = "ssh".into();
        assert!(validate_binding(&binding).is_err());
        binding.kind = "jenkins".into();
        binding.base_url = "https://user:password@ci.example".into();
        assert!(validate_binding(&binding).is_err());
        binding.base_url = "https://ci.example".into();
        binding.source_artifact_id = Some(Uuid::new_v4());
        assert!(validate_binding(&binding).is_err());
    }
    #[test]
    fn literal_env_is_bounded_and_never_executes_or_expands() {
        let values = parse(
            "# comment\nexport OWNED_TOKEN='$(do-not-run)${HOME}'\nUSER=demo # comment\nEMPTY=\n",
        )
        .unwrap();
        assert_eq!(values["OWNED_TOKEN"], "$(do-not-run)${HOME}");
        assert_eq!(values["USER"], "demo");
        assert_eq!(manifest(&values)["empty_names"], json!(["EMPTY"]));
        for text in [
            "TOKEN=a\nTOKEN=b",
            "not an assignment",
            "A='unfinished",
            "A=bad\u{0000}",
        ] {
            assert!(parse(text).is_err());
        }
    }
    #[test]
    fn secret_previews_never_include_values_or_base64() {
        let value = json!({"artifact":{"name":"env.txt"},"content":"USER=owned\nOWNED_TOKEN=synthetic-sensitive-input\n# private comment","data_base64":"c2VjcmV0"});
        let masked = model_view(&value).unwrap();
        let serialized = masked.to_string();
        assert!(!serialized.contains("synthetic-sensitive-input"));
        assert!(!serialized.contains("private comment"));
        assert!(!serialized.contains("c2VjcmV0"));
        assert_eq!(masked["credential_manifest"]["entry_count"], 2);
        assert!(model_view(&json!({"artifact":{"name":"code.py"},"content":"import os\ntoken = os.environ.get('TOKEN')\nprint('no hardcoded values')"})).is_none());
        assert!(
            model_view(&json!({"artifact":{"name":"report.csv"},"content":"state,count\ndone,7"}))
                .is_none()
        );
        assert!(
            model_view(&json!({"artifact":{"name":"notes.txt"},"content":"API_TOKEN=opaque"}))
                .is_some()
        );
    }
    #[test]
    fn success_requires_native_identity_evidence_not_just_http_200() {
        assert!(identity_matches(
            "jenkins",
            &json!({"authenticated":true,"name":"owned"})
        ));
        assert!(!identity_matches(
            "jenkins",
            &json!({"authenticated":false,"name":"anonymous"})
        ));
        assert!(!identity_matches("gitlab", &json!({"message":"login"})));
        assert!(identity_matches(
            "gitlab",
            &json!({"id":1,"username":"owned"})
        ));
        assert!(identity_matches(
            "databricks",
            &json!({"id":"owned","userName":"owned"})
        ));
    }
}
