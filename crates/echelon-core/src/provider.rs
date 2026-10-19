use crate::{
    auth::{ModelReply, SseProbe},
    error::{AppError, AppResult},
    server::AppState,
    store::audit,
    workspace::{bad, db, missing, nonempty},
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
struct ApiCredential {
    api_key: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Save {
    pub tenant_id: Uuid,
    pub id: Option<Uuid>,
    pub expected_version: Option<u64>,
    pub name: String,
    pub api_key: String,
}
fn client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|_| AppError::internal("provider_client_failed"))
}
pub async fn save(s: &AppState, r: Save) -> AppResult<Value> {
    if !nonempty(&r.name, 160)
        || !nonempty(&r.api_key, 8192)
        || r.api_key.chars().any(char::is_whitespace)
    {
        return Err(bad());
    }
    let id = r.id.unwrap_or_else(Uuid::new_v4);
    let expected = r.expected_version.unwrap_or(0);
    let mut tx = s.pool.begin().await.map_err(db)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("provider:{id}"))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let current: Option<i64> =
        sqlx::query_scalar("SELECT version FROM provider_connections WHERE tenant_id=$1 AND id=$2")
            .bind(r.tenant_id)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
    if current.unwrap_or(0) as u64 != expected || (r.id.is_some() && current.is_none()) {
        return Err(AppError::new(
            axum::http::StatusCode::CONFLICT,
            "provider_version_conflict",
        ));
    }
    let version = s
        .vault
        .put(
            &format!("providerkeys/{id}"),
            &ApiCredential { api_key: r.api_key },
            expected,
        )
        .await?;
    sqlx::query("INSERT INTO provider_connections(id,tenant_id,name,version) VALUES($1,$2,$3,$4) ON CONFLICT(id) DO UPDATE SET name=EXCLUDED.name,version=EXCLUDED.version,enabled=true").bind(id).bind(r.tenant_id).bind(r.name).bind(version as i64).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    audit(
        &s.pool,
        Some(r.tenant_id),
        None,
        "operator",
        "provider.saved",
        "completed",
        json!({"connection_id":format!("api:{id}"),"version":version}),
    )
    .await?;
    Ok(json!({"id":format!("api:{id}"),"version":version,"kind":"openai_api_key"}))
}
pub async fn list(s: &AppState, tenant: Uuid) -> AppResult<Value> {
    let mut connections = s.oauth.connections().await?["connections"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for c in &mut connections {
        c["kind"] = json!("chatgpt_oauth");
        c["name"] = json!("ChatGPT OAuth");
    }
    let rows=sqlx::query("SELECT id,name,version,enabled FROM provider_connections WHERE tenant_id=$1 ORDER BY created_at").bind(tenant).fetch_all(&s.pool).await.map_err(db)?;
    connections.extend(rows.iter().map(|r|json!({"id":format!("api:{}",r.get::<Uuid,_>("id")),"name":r.get::<String,_>("name"),"kind":"openai_api_key","version":r.get::<i64,_>("version"),"enabled":r.get::<bool,_>("enabled")})));
    Ok(json!({"connections":connections}))
}
async fn credential(s: &AppState, tenant: Uuid, id: Uuid) -> AppResult<(ApiCredential, u64)> {
    let version: Option<i64> = sqlx::query_scalar(
        "SELECT version FROM provider_connections WHERE tenant_id=$1 AND id=$2 AND enabled",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&s.pool)
    .await
    .map_err(db)?;
    let expected = version.ok_or_else(missing)? as u64;
    let key = s
        .vault
        .get::<ApiCredential>(&format!("providerkeys/{id}"))
        .await?
        .ok_or_else(missing)?;
    if key.version != expected {
        return Err(AppError::forbidden("provider_version_unconfirmed"));
    }
    Ok((key.value, expected))
}
pub async fn models(s: &AppState, tenant: Uuid, id: &str) -> AppResult<Value> {
    if let Some(id) = id.strip_prefix("api:") {
        let id = Uuid::parse_str(id).map_err(|_| bad())?;
        let (key, _) = credential(s, tenant, id).await?;
        let response = client()?
            .get("https://api.openai.com/v1/models")
            .bearer_auth(key.api_key)
            .send()
            .await
            .map_err(|_| AppError::bad("provider_unavailable"))?;
        if !response.status().is_success() {
            return Err(AppError::bad("model_catalog_rejected"));
        }
        let v: Value = response
            .json()
            .await
            .map_err(|_| AppError::bad("model_catalog_invalid"))?;
        let models = v["data"]
            .as_array()
            .ok_or_else(|| AppError::bad("model_catalog_invalid"))?;
        Ok(
            json!({"models":models.iter().filter_map(|m|m["id"].as_str()).filter(|id|(id.starts_with("gpt-")||id.starts_with("o1")||id.starts_with("o3")||id.starts_with("o4"))&&!id.contains("audio")&&!id.contains("realtime")&&!id.contains("transcribe")&&!id.contains("search")&&!id.contains("image")).take(500).map(|id|json!({"slug":id,"display_name":id})).collect::<Vec<_>>()}),
        )
    } else {
        s.oauth.models(id).await
    }
}
pub async fn infer(
    s: &AppState,
    tenant: Uuid,
    id: &str,
    model: &str,
    instructions: &str,
    input: &[Value],
    tools: Value,
) -> AppResult<ModelReply> {
    if let Some(id) = id.strip_prefix("api:") {
        if serde_json::to_vec(input).map_err(|_| bad())?.len() > 262144 {
            return Err(AppError::bad("inference_input_budget_exceeded"));
        }
        let (key, generation) =
            credential(s, tenant, Uuid::parse_str(id).map_err(|_| bad())?).await?;
        let response=client()?.post("https://api.openai.com/v1/responses").bearer_auth(key.api_key).json(&json!({"model":model,"instructions":instructions,"input":input,"tools":tools,"tool_choice":"required","parallel_tool_calls":false,"store":false,"stream":true,"include":["reasoning.encrypted_content"]})).send().await.map_err(|_|AppError::bad("provider_unavailable"))?;
        if !response.status().is_success() {
            return Err(AppError::bad("inference_rejected"));
        }
        let mut stream = response.bytes_stream();
        let mut parser = SseProbe::default();
        while let Some(chunk) = stream.next().await {
            parser.feed(&chunk.map_err(|_| AppError::bad("inference_stream_interrupted"))?)?;
        }
        let (output, usage) = parser.finish_output()?;
        Ok(ModelReply {
            output,
            usage,
            generation,
        })
    } else {
        s.oauth.infer(id, model, instructions, input, tools).await
    }
}
