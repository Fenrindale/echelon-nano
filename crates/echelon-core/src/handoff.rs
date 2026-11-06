//! Handler-selected, immutable Task-local inputs for the next Work Order.
use crate::{
    error::{AppError, AppResult},
    policy::{Claims, Role},
    workspace,
};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::collections::BTreeSet;
use uuid::Uuid;

pub async fn pin(
    pool: &PgPool,
    a: &Claims,
    artifacts: &[Uuid],
    outputs: &[Uuid],
    text: &str,
) -> AppResult<(Vec<Value>, Vec<Value>)> {
    if a.role != Role::Handler {
        return Err(AppError::forbidden("role_denied"));
    }
    if artifacts.len() > 16 || outputs.len() > 16 {
        return Err(AppError::bad("handoff_input_limit"));
    }
    let mentions: BTreeSet<Uuid> = text
        .split(|c: char| !c.is_ascii_hexdigit() && c != '-')
        .filter_map(|s| Uuid::parse_str(s).ok())
        .collect();
    let mut files = Vec::new();
    let mut refs = Vec::new();
    let file_ids: BTreeSet<_> = artifacts
        .iter()
        .copied()
        .chain(mentions.iter().copied())
        .collect();
    for id in file_ids {
        let row=sqlx::query("SELECT id,name,media_type,sha256,work_order_id,octet_length(body) AS bytes FROM artifacts WHERE tenant_id=$1 AND task_id=$2 AND id=$3")
            .bind(a.tenant_id).bind(a.task_id).bind(id).fetch_optional(pool).await.map_err(|_|AppError::internal("handoff_database_failed"))?;
        if let Some(row) = row {
            let mut file = workspace::artifact_header(&row);
            file["source_policies"] = source_policies(pool, a, row.get("work_order_id")).await?;
            files.push(file);
        } else if artifacts.contains(&id) {
            return Err(AppError::forbidden("handoff_artifact_scope_denied"));
        }
    }
    let output_ids: BTreeSet<_> = outputs.iter().copied().chain(mentions).collect();
    for id in output_ids {
        let row=sqlx::query("SELECT x.id,x.run_id,x.call_id,x.sha256,x.work_order_id,a.request->>'tool' AS tool FROM execution_output_refs x JOIN execution_runs r ON r.id=x.run_id JOIN execution_actions a ON a.run_id=x.run_id AND a.call_id=x.call_id WHERE r.tenant_id=$1 AND r.task_id=$2 AND x.id=$3 AND x.role='agent' AND x.work_order_id IS NOT NULL AND a.outcome IS NOT NULL")
            .bind(a.tenant_id).bind(a.task_id).bind(id).fetch_optional(pool).await.map_err(|_|AppError::internal("handoff_database_failed"))?;
        if let Some(row) = row {
            let sources = source_policies(pool, a, Some(row.get("work_order_id"))).await?;
            refs.push(json!({"id":row.get::<Uuid,_>("id"),"run_id":row.get::<Uuid,_>("run_id"),"call_id":row.get::<String,_>("call_id"),"sha256":row.get::<String,_>("sha256"),"work_order_id":row.get::<Uuid,_>("work_order_id"),"tool":row.get::<Option<String>,_>("tool"),"source_policies":sources}));
        } else if outputs.contains(&id) {
            return Err(AppError::forbidden("handoff_output_scope_denied"));
        }
    }
    if files.len() > 16 || refs.len() > 16 {
        return Err(AppError::bad("handoff_input_limit"));
    }
    Ok((files, refs))
}

async fn source_policies(pool: &PgPool, a: &Claims, order: Option<Uuid>) -> AppResult<Value> {
    let ids:Vec<Uuid>=sqlx::query_scalar("SELECT DISTINCT (v#>>'{}')::uuid FROM work_orders w CROSS JOIN LATERAL jsonb_path_query(w.contract,'$.**.item_id') v WHERE w.id=$1 AND w.tenant_id=$2 AND w.task_id=$3 AND jsonb_typeof(v)='string' AND (v#>>'{}') ~ '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'")
        .bind(order).bind(a.tenant_id).bind(a.task_id).fetch_all(pool).await.map_err(|_|AppError::internal("handoff_database_failed"))?;
    let denied:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_items WHERE tenant_id=$1 AND id=ANY($2) AND NOT external_model_allowed)").bind(a.tenant_id).bind(&ids).fetch_one(pool).await.map_err(|_|AppError::internal("handoff_database_failed"))?;
    if denied {
        return Err(AppError::forbidden("external_model_policy_denied"));
    }
    Ok(json!(
        ids.iter()
            .map(|id| json!({"item_id":id}))
            .collect::<Vec<_>>()
    ))
}

pub fn output_granted(contract: &Value, id: Uuid, hash: &str) -> bool {
    contract["input_outputs"].as_array().is_some_and(|refs| {
        refs.iter()
            .any(|r| r["id"] == json!(id) && r["sha256"] == hash)
    })
}
