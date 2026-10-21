//! Operator-only, read-only projections. No model history, provider envelopes or
//! credential bytes are exposed; business nodes and execution lanes stay distinct.
use crate::{
    error::AppResult,
    server::{AppState, operator},
    workspace::{bad, db, missing},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::{collections::BTreeSet, sync::Arc};
use uuid::Uuid;

const NODES: &str = include_str!("observability_nodes.sql");
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Page {
    pub tenant_id: Uuid,
    pub offset: i64,
    pub limit: Option<i64>,
    pub kind: String,
    pub q: String,
    pub focus: Option<String>,
}
fn bounds(offset: i64, limit: Option<i64>) -> AppResult<i64> {
    let limit = limit.unwrap_or(40);
    if !(0..=1_000_000).contains(&offset) || !(1..=100).contains(&limit) {
        return Err(bad());
    }
    Ok(limit)
}
async fn exists(pool: &PgPool, tenant: Uuid, task: Uuid) -> AppResult<()> {
    let yes: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tasks WHERE tenant_id=$1 AND id=$2)")
            .bind(tenant)
            .bind(task)
            .fetch_one(pool)
            .await
            .map_err(db)?;
    if yes { Ok(()) } else { Err(missing()) }
}
/// Bounded display previews, never the storage representation. Original receipts
/// and pinned documents remain intact and can be inspected through existing APIs.
pub fn preview(value: &Value) -> Value {
    preview_at(value, 0)
}
fn preview_at(value: &Value, depth: u8) -> Value {
    if depth > 14 {
        return json!("[표시 깊이 생략]");
    }
    match value {
        Value::Object(o) => {
            let mut result = serde_json::Map::new();
            for (k, v) in o.iter().take(80) {
                let lower = k.to_ascii_lowercase();
                let hidden = [
                    "access_token",
                    "refresh_token",
                    "id_token",
                    "authorization",
                    "cookie",
                    "set-cookie",
                    "password",
                    "passwd",
                    "client_secret",
                    "private_key",
                    "api_key",
                    "api_token",
                    "body_base64",
                    "base64",
                    "reasoning",
                    "thinking",
                    "encrypted_content",
                    "history",
                    "instructions",
                ]
                .contains(&lower.as_str());
                result.insert(
                    k.clone(),
                    if hidden {
                        json!("[REDACTED]")
                    } else {
                        preview_at(v, depth + 1)
                    },
                );
            }
            if o.len() > 80 {
                result.insert("_display_omitted_fields".into(), json!(o.len() - 80));
            }
            Value::Object(result)
        }
        Value::Array(a) => {
            let mut a2: Vec<Value> = a
                .iter()
                .take(40)
                .map(|v| preview_at(v, depth + 1))
                .collect();
            if a.len() > 40 {
                a2.push(json!({"_display_omitted_items":a.len()-40}));
            }
            json!(a2)
        }
        Value::String(s) => {
            let safe = crate::ingestion::redact(s)
                .trim_end_matches('\n')
                .to_owned();
            let mut p: String = safe.chars().take(8000).collect();
            if safe.chars().count() > 8000 {
                p.push_str("… [표시 생략 · 원본 기록 보존]");
            }
            json!(p)
        }
        _ => value.clone(),
    }
}
pub async fn path(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(task): Path<Uuid>,
    Query(q): Query<Page>,
) -> AppResult<Json<Value>> {
    operator(&s, &h).await?;
    path_data(&s.pool, task, &q).await.map(Json)
}
pub async fn path_data(pool: &PgPool, task: Uuid, q: &Page) -> AppResult<Value> {
    let limit = bounds(q.offset, q.limit)?;
    if q.q.len() > 300 || q.kind.len() > 40 || q.focus.as_ref().is_some_and(|v| v.len() > 150) {
        return Err(bad());
    }
    exists(pool, q.tenant_id, task).await?;
    let mut tx = pool.begin().await.map_err(db)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let filter = " WHERE ($3='' OR kind=$3) AND ($4='' OR strpos(lower(title||' '||summary),lower($4))>0) AND ($5::text IS NULL OR id=$5)";
    let total: i64 = sqlx::query_scalar(&format!("{NODES} SELECT count(*) FROM nodes{filter}"))
        .bind(q.tenant_id)
        .bind(task)
        .bind(&q.kind)
        .bind(&q.q)
        .bind(&q.focus)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    let rows = sqlx::query(&format!("{NODES} SELECT to_jsonb(n) AS value FROM nodes n{filter} ORDER BY created_at DESC,id DESC LIMIT $6 OFFSET $7"))
        .bind(q.tenant_id).bind(task).bind(&q.kind).bind(&q.q).bind(&q.focus).bind(limit).bind(q.offset).fetch_all(&mut *tx).await.map_err(db)?;
    let items: Vec<Value> = rows.iter().map(|r| r.get("value")).collect();
    let selected: Vec<String> = items
        .iter()
        .filter_map(|v| v["id"].as_str().map(str::to_owned))
        .collect();
    // One-hop neighbours only; no recursive expansion of the company knowledge graph.
    let adjacent = sqlx::query(&format!("{NODES} SELECT to_jsonb(n) AS value FROM nodes n WHERE n.id=ANY($3) OR n.id IN (SELECT p->>'id' FROM nodes x CROSS JOIN LATERAL jsonb_array_elements(x.parents) p WHERE x.id=ANY($3)) OR EXISTS(SELECT 1 FROM jsonb_array_elements(n.parents) p WHERE p->>'id'=ANY($3)) ORDER BY (n.id=ANY($3)) DESC,n.created_at DESC,n.id DESC LIMIT 201"))
        .bind(q.tenant_id).bind(task).bind(&selected).fetch_all(&mut *tx).await.map_err(db)?;
    let truncated = adjacent.len() > 200;
    let nodes: Vec<Value> = adjacent.iter().take(200).map(|r| r.get("value")).collect();
    let ids: BTreeSet<&str> = nodes.iter().filter_map(|n| n["id"].as_str()).collect();
    let mut edges = Vec::new();
    for n in &nodes {
        if let Some(ps) = n["parents"].as_array() {
            for p in ps {
                if p["id"].as_str().is_some_and(|id| ids.contains(id)) {
                    edges.push(json!({"source":p["id"],"target":n["id"],"relation":p["relation"]}));
                }
            }
        }
    }
    tx.commit().await.map_err(db)?;
    Ok(
        json!({"items":items.iter().map(preview).collect::<Vec<_>>(),"nodes":nodes.iter().map(preview).collect::<Vec<_>>(),"edges":edges,"total":total,"offset":q.offset,"limit":limit,"has_more":q.offset+limit<total,"neighbours_truncated":truncated,"projection":"business_records_current_state","read_only":true}),
    )
}

const LANES: &str = r#"WITH lanes AS (
 SELECT 'execution' AS source,r.id AS source_id,COALESCE(e.role,CASE WHEN r.config->>'mode'='dream' THEN 'master' ELSE 'handler' END) AS role,r.status,r.created_at,COALESCE(max(e.created_at),r.created_at) AS last_activity,
 count(e.seq) AS events,count(*) FILTER(WHERE e.kind='model.started') AS calls,
 COALESCE(sum((e.details->'usage'->>'total_tokens')::bigint) FILTER(WHERE e.kind='model.completed'),0)::bigint AS tokens,
 r.state->>'phase' AS phase,NULL::int AS units,r.error_code
 FROM execution_runs r LEFT JOIN execution_events e ON e.run_id=r.id WHERE r.tenant_id=$1 AND r.task_id=$2 GROUP BY r.id,e.role
 UNION ALL
 SELECT 'ingestion',j.id,COALESCE(e.actor_role,'handler'),j.status,j.created_at,COALESCE(max(e.created_at),j.created_at),count(e.seq),count(*) FILTER(WHERE e.kind='model.started'),
 COALESCE(sum((e.details->'usage'->>'total_tokens')::bigint) FILTER(WHERE e.kind='model.completed'),0)::bigint,j.inflight->>'phase',j.total_units,j.error_code
 FROM knowledge_ingestion_jobs j LEFT JOIN knowledge_ingestion_events e ON e.job_id=j.id WHERE j.tenant_id=$1 AND j.task_id=$2 GROUP BY j.id,e.actor_role
 UNION ALL
 SELECT 'retention',j.id,'analyst',j.status,j.created_at,COALESCE(max(a.created_at),j.created_at),count(a.seq),
 count(*) FILTER(WHERE a.outcome='started'),COALESCE(sum((a.details->'usage'->>'total_tokens')::bigint) FILTER(WHERE a.outcome='completed'),0)::bigint,
 'knowledge.review', (SELECT count(*)::int FROM knowledge_retention_units u WHERE u.job_id=j.id),j.error_code
 FROM knowledge_retention_jobs j LEFT JOIN audit_records a ON a.tenant_id=j.tenant_id AND a.task_id=j.task_id AND a.details->>'job_id'=j.id::text AND a.action='knowledge.review_model'
 WHERE j.tenant_id=$1 AND j.task_id=$2 GROUP BY j.id
)"#;
pub async fn sessions(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(task): Path<Uuid>,
    Query(q): Query<Page>,
) -> AppResult<Json<Value>> {
    operator(&s, &h).await?;
    sessions_data(&s.pool, task, &q).await.map(Json)
}
pub async fn sessions_data(pool: &PgPool, task: Uuid, q: &Page) -> AppResult<Value> {
    let limit = bounds(q.offset, q.limit)?;
    exists(pool, q.tenant_id, task).await?;
    let rows=sqlx::query(&format!("{LANES} SELECT to_jsonb(l) AS value,count(*) OVER() AS total FROM lanes l ORDER BY last_activity DESC,source,source_id,role LIMIT $3 OFFSET $4"))
        .bind(q.tenant_id).bind(task).bind(limit).bind(q.offset).fetch_all(pool).await.map_err(db)?;
    let total: i64 = if let Some(r) = rows.first() {
        r.get("total")
    } else {
        sqlx::query_scalar(&format!("{LANES} SELECT count(*) FROM lanes"))
            .bind(q.tenant_id)
            .bind(task)
            .fetch_one(pool)
            .await
            .map_err(db)?
    };
    Ok(
        json!({"sessions":rows.iter().map(|r|preview(&r.get::<Value,_>("value"))).collect::<Vec<_>>(),"total":total,"has_more":q.offset+limit<total,"read_only":true}),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionQuery {
    pub tenant_id: Uuid,
    pub source: String,
    pub source_id: Uuid,
    pub role: String,
    pub before: Option<i64>,
    pub limit: Option<i64>,
    pub ordinal: Option<i32>,
}
pub async fn session(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(task): Path<Uuid>,
    Query(q): Query<SessionQuery>,
) -> AppResult<Json<Value>> {
    operator(&s, &h).await?;
    session_data(&s.pool, task, &q).await.map(Json)
}
pub async fn session_data(pool: &PgPool, task: Uuid, q: &SessionQuery) -> AppResult<Value> {
    let limit = bounds(0, q.limit)?;
    if !["handler", "analyst", "agent", "master"].contains(&q.role.as_str())
        || q.before.is_some_and(|v| v < 1)
        || q.ordinal.is_some_and(|v| !(0..=1_000_000).contains(&v))
        || q.source == "retention" && q.role != "analyst"
        || q.source == "ingestion" && q.role == "master"
    {
        return Err(bad());
    }
    exists(pool, q.tenant_id, task).await?;
    let table = match q.source.as_str() {
        "execution" => "execution_runs",
        "ingestion" => "knowledge_ingestion_jobs",
        "retention" => "knowledge_retention_jobs",
        _ => return Err(bad()),
    };
    let status: Option<String> = sqlx::query_scalar(&format!(
        "SELECT status FROM {table} WHERE tenant_id=$1 AND task_id=$2 AND id=$3"
    ))
    .bind(q.tenant_id)
    .bind(task)
    .bind(q.source_id)
    .fetch_optional(pool)
    .await
    .map_err(db)?;
    let status = status.ok_or_else(missing)?;
    let sql = match q.source.as_str() {
        "execution" => {
            "SELECT e.seq,e.created_at::text AS at,e.kind,e.details,NULL::int AS ordinal,CASE WHEN e.kind IN ('tool.completed','tool.failed','tool.denied') THEN a.request ELSE NULL END AS request,CASE WHEN e.kind IN ('tool.completed','tool.failed','tool.denied') THEN a.outcome ELSE NULL END AS outcome FROM execution_events e LEFT JOIN execution_actions a ON a.run_id=e.run_id AND a.call_id=e.details->>'call_id' WHERE e.run_id=$1 AND e.role=$2 AND ($3::bigint IS NULL OR e.seq<$3) ORDER BY e.seq DESC LIMIT $4"
        }
        "ingestion" => {
            "SELECT seq,created_at::text AS at,kind,details,ordinal,NULL::jsonb AS request,NULL::jsonb AS outcome FROM knowledge_ingestion_events WHERE job_id=$1 AND actor_role=$2 AND ($3::bigint IS NULL OR seq<$3) AND ($5::int IS NULL OR ordinal=$5) ORDER BY seq DESC LIMIT $4"
        }
        _ => {
            "SELECT seq,created_at::text AS at,action||'.'||outcome AS kind,details,NULL::int AS ordinal,NULL::jsonb AS request,NULL::jsonb AS outcome FROM audit_records WHERE details->>'job_id'=($1::uuid)::text AND actor_role=$2 AND action='knowledge.review_model' AND ($3::bigint IS NULL OR seq<$3) AND tenant_id=$5 AND task_id=$6 ORDER BY seq DESC LIMIT $4"
        }
    };
    let mut query = sqlx::query(sql)
        .bind(q.source_id)
        .bind(&q.role)
        .bind(q.before)
        .bind(limit + 1);
    if q.source == "ingestion" {
        query = query.bind(q.ordinal);
    }
    if q.source == "retention" {
        query = query.bind(q.tenant_id).bind(task);
    }
    let rows = query.fetch_all(pool).await.map_err(db)?;
    let has_more = rows.len() > limit as usize;
    let events:Vec<Value>=rows.iter().take(limit as usize).map(|r|json!({"seq":r.get::<i64,_>("seq"),"created_at":r.get::<String,_>("at"),"kind":r.get::<String,_>("kind"),"ordinal":r.get::<Option<i32>,_>("ordinal"),"details":preview(&r.get::<Value,_>("details")),"request":preview(&r.get::<Option<Value>,_>("request").unwrap_or(Value::Null)),"outcome":preview(&r.get::<Option<Value>,_>("outcome").unwrap_or(Value::Null))})).collect();
    let mut unit = Value::Null;
    if let Some(n) = q.ordinal {
        if q.source == "ingestion" {
            let row=sqlx::query("SELECT source_id,locator,status,state FROM knowledge_ingestion_units WHERE job_id=$1 AND ordinal=$2").bind(q.source_id).bind(n).fetch_optional(pool).await.map_err(db)?.ok_or_else(missing)?;
            let state: Value = row.get("state");
            let output = match q.role.as_str() {
                "agent" => {
                    json!({"extraction":state["extraction"],"empty_reason":state["empty_reason"]})
                }
                "handler" => json!({"proposals":state["proposals"],"submitted":state["submitted"]}),
                _ => json!({"published":state["published"]}),
            };
            unit = preview(
                &json!({"ordinal":n,"source_id":row.get::<String,_>("source_id"),"locator":row.get::<String,_>("locator"),"status":row.get::<String,_>("status"),"output":output}),
            );
        } else if q.source == "retention" && q.role == "analyst" {
            unit=sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('ordinal',ordinal,'snapshot',snapshot,'response',response,'result',result) FROM knowledge_retention_units WHERE job_id=$1 AND ordinal=$2").bind(q.source_id).bind(n).fetch_optional(pool).await.map_err(db)?.ok_or_else(missing)?;
            unit = preview(&unit);
        } else {
            return Err(bad());
        }
    }
    Ok(
        json!({"source":q.source,"source_id":q.source_id,"role":q.role,"status":status,"before":q.before,"next_before":events.last().map(|e|e["seq"].clone()),"has_more":has_more,"events":events,"unit":unit,"read_only":true,"display":"persisted_public_actions_and_receipts"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_previews_remove_credentials_and_private_model_envelopes() {
        let v = preview(
            &json!({"access_token":"sensitive","body_base64":"sensitive","history":["private"],"reasoning":"private","nested":{"password":"sensitive"},"usage":{"total_tokens":123},"safe":"확인된 관찰"}),
        );
        assert!(!v.to_string().contains("sensitive"));
        assert_eq!(v["usage"]["total_tokens"], 123);
        assert_eq!(v["safe"], "확인된 관찰");
        assert!(
            preview(&json!("가".repeat(9000)))
                .as_str()
                .unwrap()
                .ends_with("원본 기록 보존]")
        );
    }
    #[test]
    fn reject_unbounded_history_queries() {
        assert!(bounds(-1, Some(30)).is_err());
        assert!(bounds(0, Some(101)).is_err());
    }
}
