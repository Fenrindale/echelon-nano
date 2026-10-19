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
    target: String,
    #[serde(default)]
    objective: String,
    #[serde(default)]
    expected_output: String,
    #[serde(default)]
    references: Vec<Evidence>,
}

pub async fn delegate(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    if a.role != Role::Handler {
        return Err(AppError::forbidden("role_denied"));
    }
    let d: Delegate = serde_json::from_value(input.clone()).map_err(|_| invalid())?;
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
        evidence.push(json!({"pack_id":r.pack_id,"resolved":resolved}));
    }
    let contract = json!({"objective":d.objective,"expected_output":d.expected_output,"references":d.references,"evidence":evidence});
    if serde_json::to_vec(&contract).map_err(|_| invalid())?.len() > 131072 {
        return Err(AppError::bad("work_evidence_budget_exceeded"));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO work_orders(id,tenant_id,task_id,target,contract) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(a.tenant_id)
    .bind(a.task_id)
    .bind(d.target)
    .bind(contract)
    .execute(pool)
    .await
    .map_err(db)?;
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
    Ok(
        json!({"work_order_id":r.work_order_id,"target":row.get::<String,_>("target"),"status":row.get::<String,_>("status"),"contract":row.get::<Value,_>("contract"),"result":row.get::<Option<Value>,_>("result")}),
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
}
pub async fn complete(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    if a.role != Role::Agent {
        return Err(AppError::forbidden("role_denied"));
    }
    let id = a
        .work_order_id
        .ok_or_else(|| AppError::forbidden("work_order_required"))?;
    let r: ResultInput = serde_json::from_value(input.clone()).map_err(|_| invalid())?;
    if !matches!(r.outcome.as_str(), "completed" | "blocked" | "failed")
        || !bounded(&r.summary, 8000)
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
