use crate::{
    context,
    error::{AppError, AppResult},
    policy::{Claims, Role},
};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

fn db(_: sqlx::Error) -> AppError {
    AppError::internal("work_database_failed")
}
fn invalid() -> AppError {
    AppError::bad("invalid_work_input")
}
fn missing() -> AppError {
    AppError::new(StatusCode::NOT_FOUND, "work_order_not_found")
}
fn bounded(s: &str, n: usize) -> bool {
    !s.trim().is_empty() && s.len() <= n
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub pack_id: Uuid,
    pub ref_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Delegate {
    request_id: Option<Uuid>,
    target: String,
    #[serde(default)]
    objective: String,
    #[serde(default)]
    expected_output: String,
    #[serde(default)]
    references: Vec<Evidence>,
    #[serde(default)]
    protocol_parameters: Value,
}

pub async fn delegate(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    if a.role != Role::Handler {
        return Err(AppError::forbidden("role_denied"));
    }
    let d: Delegate = serde_json::from_value(input.clone()).map_err(|_| invalid())?;
    if let Some(key) = d.request_id
        && let Some(row)=sqlx::query("SELECT id,target,contract,status FROM work_orders WHERE tenant_id=$1 AND task_id=$2 AND request_key=$3").bind(a.tenant_id).bind(a.task_id).bind(key).fetch_optional(pool).await.map_err(db)? {
            if row.get::<Value,_>("contract")["request"]!=*input {return Err(AppError::new(StatusCode::CONFLICT,"work_idempotency_conflict"));}
            return Ok(json!({"work_order_id":row.get::<Uuid,_>("id"),"status":row.get::<String,_>("status"),"replayed":true}));
    }
    if !bounded(&d.target, 500)
        || d.references.len() > 16
        || d.objective.len() > 4000
        || d.expected_output.len() > 2000
    {
        return Err(invalid());
    }
    if (!d.objective.is_empty() || !d.expected_output.is_empty() || !d.references.is_empty())
        && (!bounded(&d.objective, 4000) || !bounded(&d.expected_output, 2000))
    {
        return Err(invalid());
    }
    let mut tx = pool.begin().await.map_err(db)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('work:' || $1,0))")
        .bind(a.task_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    if let Some(key)=d.request_id && let Some(row)=sqlx::query("SELECT id,contract,status FROM work_orders WHERE tenant_id=$1 AND task_id=$2 AND request_key=$3").bind(a.tenant_id).bind(a.task_id).bind(key).fetch_optional(&mut *tx).await.map_err(db)? {
        if row.get::<Value,_>("contract")["request"]!=*input {return Err(AppError::new(StatusCode::CONFLICT,"work_idempotency_conflict"));}
        return Ok(json!({"work_order_id":row.get::<Uuid,_>("id"),"status":row.get::<String,_>("status"),"replayed":true}));
    }
    let policy = crate::workspace::task_contract(pool, a).await?;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM work_orders WHERE tenant_id=$1 AND task_id=$2")
            .bind(a.tenant_id)
            .bind(a.task_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
    if count >= policy.max_work_orders.max(1) as i64 {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "work_order_budget_exceeded",
        ));
    }
    let mut evidence = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for r in &d.references {
        if !seen.insert((r.pack_id, r.ref_id)) {
            return Err(invalid());
        }
        let resolved = context::read(
            pool,
            a,
            &json!({"op":"resolve","pack_id":r.pack_id,"ref_id":r.ref_id}),
        )
        .await?;
        let allowed: bool = sqlx::query_scalar(
            "SELECT external_model_allowed FROM knowledge_items WHERE tenant_id=$1 AND id=$2",
        )
        .bind(a.tenant_id)
        .bind(
            serde_json::from_value::<Uuid>(resolved["reference"]["item_id"].clone())
                .map_err(|_| invalid())?,
        )
        .fetch_one(pool)
        .await
        .map_err(db)?;
        if !allowed {
            return Err(AppError::forbidden("external_model_policy_denied"));
        }
        if resolved["document"]["external_model_allowed"] == false {
            return Err(AppError::forbidden("external_model_policy_denied"));
        }
        evidence.push(json!({"pack_id":r.pack_id,"resolved":resolved}));
    }
    let packs = d.references.iter().map(|r| r.pack_id).collect::<Vec<_>>();
    let protocols=sqlx::query("SELECT DISTINCT (manifest->>'protocol_id')::uuid AS id,(manifest->>'protocol_version')::int AS version FROM context_packs WHERE tenant_id=$1 AND task_id=$2 AND id=ANY($3) AND manifest->>'protocol_id' IS NOT NULL").bind(a.tenant_id).bind(a.task_id).bind(packs).fetch_all(&mut *tx).await.map_err(db)?;
    if protocols.len() > 1 {
        return Err(AppError::bad("mixed_protocol_contract"));
    }
    let procedure = if let Some(row) = protocols.first() {
        let p = crate::protocol::load(pool, a.tenant_id, row.get("id"), Some(row.get("version")))
            .await?;
        if p["current_status"] != "active" || p["document"]["status"] != "active" {
            return Err(AppError::bad("protocol_not_active"));
        }
        if p["document"]["external_model_allowed"] == false {
            return Err(AppError::forbidden("external_model_policy_denied"));
        }
        for member in p["document"]["references"].as_array().ok_or_else(invalid)? {
            if member["required"] == true
                && !evidence.iter().any(|e| {
                    e["resolved"]["reference"]["item_id"] == member["item_id"]
                        && e["resolved"]["reference"]["version"] == member["version"]
                })
            {
                return Err(AppError::bad("protocol_required_evidence_missing"));
            }
        }
        crate::protocol::validate_parameters(&p["document"], &d.protocol_parameters)?;
        let mut p = crate::protocol::execution_contract(&p);
        p["parameters"] = d.protocol_parameters.clone();
        p
    } else {
        if !d.protocol_parameters.is_null() {
            return Err(invalid());
        }
        Value::Null
    };
    let contract = json!({"objective":d.objective,"expected_output":d.expected_output,"references":d.references,"evidence":evidence,"protocol":procedure,"input_artifacts":policy.input_artifacts,"tool_policy":policy,"request":input});
    if serde_json::to_vec(&contract).map_err(|_| invalid())?.len() > 131072 {
        return Err(AppError::bad("work_evidence_budget_exceeded"));
    }
    let id = Uuid::new_v4();
    let saved=sqlx::query(
        "INSERT INTO work_orders(id,tenant_id,task_id,target,contract,request_key) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(a.tenant_id)
    .bind(a.task_id)
    .bind(d.target)
    .bind(contract)
    .bind(d.request_id)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    if saved.rows_affected() == 0 {
        let row=sqlx::query("SELECT id,contract,status FROM work_orders WHERE tenant_id=$1 AND task_id=$2 AND request_key=$3").bind(a.tenant_id).bind(a.task_id).bind(d.request_id).fetch_one(&mut *tx).await.map_err(db)?;
        if row.get::<Value, _>("contract")["request"] != *input {
            return Err(AppError::new(
                StatusCode::CONFLICT,
                "work_idempotency_conflict",
            ));
        }
        return Ok(
            json!({"work_order_id":row.get::<Uuid,_>("id"),"status":row.get::<String,_>("status"),"replayed":true}),
        );
    }
    tx.commit().await.map_err(db)?;
    Ok(json!({"work_order_id":id,"status":"ready"}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    work_order_id: Uuid,
}
pub async fn read(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    if !matches!(a.role, Role::Handler | Role::Agent) {
        return Err(AppError::forbidden("role_denied"));
    }
    let r: Read = serde_json::from_value(input.clone()).map_err(|_| invalid())?;
    if a.role == Role::Agent && a.work_order_id != Some(r.work_order_id) {
        return Err(missing());
    }
    let row=sqlx::query("SELECT w.id,w.target,w.status,w.contract,r.result FROM work_orders w LEFT JOIN work_order_results r ON r.work_order_id=w.id WHERE w.id=$1 AND w.tenant_id=$2 AND w.task_id=$3")
        .bind(r.work_order_id).bind(a.tenant_id).bind(a.task_id).fetch_optional(pool).await.map_err(db)?.ok_or_else(missing)?;
    let artifacts=sqlx::query("SELECT id,name,media_type,sha256,work_order_id,octet_length(body) AS bytes FROM artifacts WHERE tenant_id=$1 AND task_id=$2 AND work_order_id=$3 ORDER BY created_at LIMIT 64").bind(a.tenant_id).bind(a.task_id).bind(r.work_order_id).fetch_all(pool).await.map_err(db)?;
    let observations=sqlx::query("SELECT id,content FROM observations WHERE tenant_id=$1 AND task_id=$2 AND work_order_id=$3 ORDER BY created_at LIMIT 32").bind(a.tenant_id).bind(a.task_id).bind(r.work_order_id).fetch_all(pool).await.map_err(db)?;
    let tool_evidence = crate::credentials::work_evidence(pool, a, r.work_order_id).await?;
    Ok(
        json!({"work_order_id":r.work_order_id,"target":row.get::<String,_>("target"),"status":row.get::<String,_>("status"),"contract":row.get::<Value,_>("contract"),"result":row.get::<Option<Value>,_>("result"),"artifacts":artifacts.iter().map(crate::workspace::artifact_header).collect::<Vec<_>>(),"observations":observations.iter().map(|o|json!({"id":o.get::<Uuid,_>("id"),"content":o.get::<Value,_>("content")})).collect::<Vec<_>>(),"tool_evidence":tool_evidence}),
    )
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ResultInput {
    outcome: String,
    summary: String,
    #[serde(default)]
    evidence: Vec<Evidence>,
    #[serde(default)]
    limitations: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    artifacts: Vec<Uuid>,
}
pub async fn complete(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    if a.role != Role::Agent {
        return Err(AppError::forbidden("role_denied"));
    }
    let id = a
        .work_order_id
        .ok_or_else(|| AppError::forbidden("work_order_required"))?;
    let r: ResultInput = serde_json::from_value(input.clone()).map_err(|_| invalid())?;
    if !matches!(
        r.outcome.as_str(),
        "completed" | "partial" | "blocked" | "failed"
    ) || !bounded(&r.summary, 8000)
        || r.evidence.len() > 16
        || r.limitations.len() > 8
        || r.limitations.iter().any(|s| !bounded(s, 1000))
    {
        return Err(invalid());
    }
    let mut tx = pool.begin().await.map_err(db)?;
    let row=sqlx::query("SELECT status,contract FROM work_orders WHERE id=$1 AND tenant_id=$2 AND task_id=$3 FOR UPDATE")
        .bind(id).bind(a.tenant_id).bind(a.task_id).fetch_optional(&mut *tx).await.map_err(db)?.ok_or_else(missing)?;
    if row.get::<String, _>("status") != "ready" {
        return Err(AppError::forbidden("work_order_inactive"));
    }
    let contract: Value = row.get("contract");
    if r.artifacts.len() > 16 {
        return Err(invalid());
    }
    if matches!(r.outcome.as_str(), "completed" | "partial")
        && !contract["tool_policy"]["output_format"]
            .as_str()
            .unwrap_or_default()
            .is_empty()
        && contract["tool_policy"]["allowed_tools"]
            .as_array()
            .is_some_and(|tools| {
                tools
                    .iter()
                    .any(|v| v == "artifact.write" || v == "shell.exec")
            })
        && r.artifacts.is_empty()
    {
        return Err(AppError::bad("work_artifact_required"));
    }
    for e in &r.evidence {
        let v = serde_json::to_value(e).map_err(|_| invalid())?;
        if !contract["references"]
            .as_array()
            .is_some_and(|refs| refs.contains(&v))
        {
            return Err(AppError::forbidden("work_evidence_denied"));
        }
    }
    if r.outcome == "completed"
        && contract["references"]
            .as_array()
            .is_some_and(|refs| !refs.is_empty())
        && r.evidence.is_empty()
    {
        return Err(AppError::bad("work_evidence_required"));
    }
    let result = serde_json::to_value(r).map_err(|_| invalid())?;
    for id in result["artifacts"].as_array().into_iter().flatten() {
        let id: Uuid = serde_json::from_value(id.clone()).map_err(|_| invalid())?;
        let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM artifacts WHERE tenant_id=$1 AND task_id=$2 AND work_order_id=$3 AND id=$4)").bind(a.tenant_id).bind(a.task_id).bind(a.work_order_id).bind(id).fetch_one(&mut *tx).await.map_err(db)?;
        if !exists {
            return Err(AppError::forbidden("work_artifact_denied"));
        }
    }
    sqlx::query("INSERT INTO work_order_results(work_order_id,tenant_id,task_id,result) VALUES($1,$2,$3,$4)")
        .bind(id).bind(a.tenant_id).bind(a.task_id).bind(&result).execute(&mut *tx).await.map_err(db)?;
    sqlx::query("UPDATE work_orders SET status='completed' WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(json!({"work_order_id":id,"status":"completed","result":result}))
}

pub async fn existing_completion(
    pool: &PgPool,
    a: &Claims,
    input: &Value,
) -> AppResult<Option<Value>> {
    if a.role != Role::Agent {
        return Err(AppError::forbidden("role_denied"));
    }
    let r: ResultInput = serde_json::from_value(input.clone()).map_err(|_| invalid())?;
    let record:Option<Value>=sqlx::query_scalar("SELECT result FROM work_order_results WHERE tenant_id=$1 AND task_id=$2 AND work_order_id=$3").bind(a.tenant_id).bind(a.task_id).bind(a.work_order_id).fetch_optional(pool).await.map_err(db)?;
    match record {
        Some(v) if v == json!(r) => Ok(Some(
            json!({"work_order_id":a.work_order_id,"status":"completed","result":v,"replayed":true}),
        )),
        Some(_) => Err(AppError::new(StatusCode::CONFLICT, "work_result_conflict")),
        None => Ok(None),
    }
}
