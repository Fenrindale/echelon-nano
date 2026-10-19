use crate::{
    error::{AppError, AppResult},
    knowledge::{self, Document},
    policy::{Claims, Role},
    workspace::{bad, db, digest, missing, nonempty},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};
use uuid::Uuid;
static PACK_CACHE: OnceLock<Mutex<BTreeMap<String, Value>>> = OnceLock::new();

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub item_id: Uuid,
    pub version: i32,
    #[serde(default = "yes")]
    pub required: bool,
}
fn yes() -> bool {
    true
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub required: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Protocol {
    #[serde(default = "yes", skip_serializing_if = "yes_value")]
    pub external_model_allowed: bool,
    pub codename: String,
    pub description: String,
    #[serde(default)]
    pub applicability: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    pub references: Vec<Member>,
    pub steps: Vec<String>,
    pub completion_conditions: Vec<String>,
    #[serde(default)]
    pub stop_conditions: Vec<String>,
    #[serde(default)]
    pub retry_conditions: Vec<String>,
    pub owner: String,
    pub status: String,
    #[serde(default)]
    pub source_tasks: Vec<Uuid>,
}
fn yes_value(v: &bool) -> bool {
    *v
}
pub fn execution_contract(p: &Value) -> Value {
    if p.is_null() {
        return Value::Null;
    }
    let d = &p["document"];
    json!({"id":p["id"],"version":p["version"],"codename":d["codename"],"references":d["references"],"steps":d["steps"],"completion_conditions":d["completion_conditions"],"stop_conditions":d["stop_conditions"],"retry_conditions":d["retry_conditions"]})
}
pub fn validate_parameters(document: &Value, parameters: &Value) -> AppResult<()> {
    let declared = document["parameters"].as_array().ok_or_else(bad)?;
    if !(parameters.is_object() || parameters.is_null() && declared.is_empty()) {
        return Err(bad());
    }
    if serde_json::to_vec(parameters).map_err(|_| bad())?.len() > 8000 {
        return Err(bad());
    }
    if let Some(values) = parameters.as_object()
        && values
            .keys()
            .any(|key| !declared.iter().any(|p| p["name"] == key.as_str()))
    {
        return Err(AppError::bad("unknown_protocol_parameter"));
    }
    for p in declared {
        if p["required"] == true
            && parameters
                .get(p["name"].as_str().ok_or_else(bad)?)
                .is_none_or(|v| v.is_null() || v.as_str().is_some_and(|s| s.trim().is_empty()))
        {
            return Err(AppError::bad("protocol_parameter_required"));
        }
    }
    Ok(())
}
impl Protocol {
    pub fn validate(&self) -> AppResult<()> {
        if !nonempty(&self.codename, 80)
            || !nonempty(&self.description, 2000)
            || !nonempty(&self.owner, 160)
            || self.applicability.len() > 2000
            || self.references.is_empty()
            || self.references.len() > 64
            || self.references.iter().filter(|r| r.required).count() > 16
            || self.steps.is_empty()
            || self.steps.len() > 24
            || self.completion_conditions.is_empty()
            || self.completion_conditions.len() > 16
            || self.stop_conditions.len() > 16
            || self.retry_conditions.len() > 16
            || self.parameters.len() > 16
            || self.aliases.len() > 16
            || self.source_tasks.len() > 32
            || !matches!(
                self.status.as_str(),
                "candidate" | "testing" | "active" | "suspended" | "retired"
            )
            || self
                .steps
                .iter()
                .chain(&self.completion_conditions)
                .chain(&self.stop_conditions)
                .chain(&self.retry_conditions)
                .any(|s| !nonempty(s, 1000))
            || self
                .parameters
                .iter()
                .any(|p| !nonempty(&p.name, 80) || !nonempty(&p.description, 500))
            || self.aliases.iter().any(|s| !nonempty(s, 80))
        {
            return Err(bad());
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut parameters = std::collections::BTreeSet::new();
        for p in &self.parameters {
            if !parameters.insert(&p.name) || p.name.chars().any(char::is_control) {
                return Err(bad());
            }
        }
        for r in &self.references {
            if r.version < 1 || !seen.insert((r.item_id, r.version)) {
                return Err(bad());
            }
        }
        Ok(())
    }
}
pub async fn list(pool: &PgPool, tenant: Uuid, query: &str) -> AppResult<Value> {
    if query.len() > 200 {
        return Err(bad());
    }
    let rows=sqlx::query("SELECT p.id,p.version,p.codename,p.description,p.status,COALESCE((r.document->>'external_model_allowed')::bool,true) AND NOT EXISTS(SELECT 1 FROM protocol_members m JOIN knowledge_items k ON k.tenant_id=m.tenant_id AND k.id=m.item_id WHERE m.tenant_id=p.tenant_id AND m.protocol_id=p.id AND m.protocol_version=p.version AND NOT k.external_model_allowed) AS external_model_allowed FROM protocols p JOIN protocol_revisions r ON r.tenant_id=p.tenant_id AND r.protocol_id=p.id AND r.version=p.version WHERE p.tenant_id=$1 AND (strpos(lower(p.codename),lower($2))>0 OR strpos(lower(p.description),lower($2))>0) ORDER BY p.codename LIMIT 100").bind(tenant).bind(query).fetch_all(pool).await.map_err(db)?;
    Ok(
        json!({"protocols":rows.iter().map(|r|json!({"id":r.get::<Uuid,_>("id"),"version":r.get::<i32,_>("version"),"codename":r.get::<String,_>("codename"),"description":r.get::<String,_>("description"),"external_model_allowed":r.get::<bool,_>("external_model_allowed"),"status":r.get::<String,_>("status")})).collect::<Vec<_>>()}),
    )
}
pub async fn load(pool: &PgPool, tenant: Uuid, id: Uuid, version: Option<i32>) -> AppResult<Value> {
    let row=sqlx::query("SELECT p.version AS current_version,p.status AS current_status,r.version,r.document FROM protocols p JOIN protocol_revisions r ON r.tenant_id=p.tenant_id AND r.protocol_id=p.id AND r.version=COALESCE($3,p.version) WHERE p.tenant_id=$1 AND p.id=$2").bind(tenant).bind(id).bind(version).fetch_optional(pool).await.map_err(db)?.ok_or_else(missing)?;
    let mut doc: Value = row.get("document");
    let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM protocol_members m JOIN knowledge_items k ON k.tenant_id=m.tenant_id AND k.id=m.item_id WHERE m.tenant_id=$1 AND m.protocol_id=$2 AND m.protocol_version=$3 AND NOT k.external_model_allowed)").bind(tenant).bind(id).bind(row.get::<i32,_>("version")).fetch_one(pool).await.map_err(db)?;
    if blocked {
        doc["external_model_allowed"] = json!(false);
    }
    let version = row.get::<i32, _>("version");
    let epoch: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(version),0)::bigint FROM knowledge_items WHERE tenant_id=$1",
    )
    .bind(tenant)
    .fetch_one(pool)
    .await
    .map_err(db)?;
    let expiry_epoch:Option<i64>=sqlx::query_scalar("SELECT max((document->>'valid_until')::bigint) FROM knowledge_revisions WHERE tenant_id=$1 AND (document->>'valid_until')::bigint<=extract(epoch from now())").bind(tenant).fetch_one(pool).await.map_err(db)?;
    let cache_key=digest(&serde_json::to_vec(&json!({"tenant":tenant,"protocol":id,"version":version,"document":doc,"knowledge_epoch":epoch,"expiry_epoch":expiry_epoch,"current_version":row.get::<i32,_>("current_version"),"status":row.get::<String,_>("current_status")})).map_err(|_|bad())?);
    let cache = PACK_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some(mut result) = cache
        .lock()
        .map_err(|_| AppError::internal("protocol_cache_failed"))?
        .get(&cache_key)
        .cloned()
    {
        result["cache_hit"] = json!(true);
        return Ok(result);
    }
    let warnings=sqlx::query("SELECT m.item_id,m.item_version,k.version AS current_version,k.truth_state,k.outcome,CASE WHEN s.document->>'freshness'='superseded' THEN 'superseded' WHEN (s.document->>'valid_until')::bigint<=extract(epoch from now()) THEN 'expired' ELSE s.document->>'freshness' END AS freshness FROM protocol_members m JOIN knowledge_items k ON k.tenant_id=m.tenant_id AND k.id=m.item_id JOIN knowledge_revisions s ON s.tenant_id=k.tenant_id AND s.item_id=k.id AND s.version=k.version JOIN knowledge_revisions pinned ON pinned.tenant_id=m.tenant_id AND pinned.item_id=m.item_id AND pinned.version=m.item_version WHERE m.tenant_id=$1 AND m.protocol_id=$2 AND m.protocol_version=$3 AND ((s.document-'tags')<>(pinned.document-'tags') OR s.document->>'freshness'<>'current' OR s.document->>'truth_state' IN ('refuted','conflicted') OR s.document->>'outcome'='failed' OR (s.document->>'valid_until')::bigint<=extract(epoch from now()) OR (pinned.document->>'valid_until')::bigint<=extract(epoch from now())) ORDER BY m.item_id").bind(tenant).bind(id).bind(row.get::<i32,_>("version")).fetch_all(pool).await.map_err(db)?;
    let result = json!({"id":id,"version":version,"document":doc,"current_version":row.get::<i32,_>("current_version"),"current_status":row.get::<String,_>("current_status"),"review_required":!warnings.is_empty(),"cache_key":cache_key,"cache_hit":false,"warnings":warnings.iter().map(|r|json!({"item_id":r.get::<Uuid,_>("item_id"),"pinned_version":r.get::<i32,_>("item_version"),"current_version":r.get::<i32,_>("current_version"),"truth_state":r.get::<String,_>("truth_state"),"outcome":r.get::<String,_>("outcome"),"freshness":r.get::<String,_>("freshness")})).collect::<Vec<_>>()});
    let mut cached = cache
        .lock()
        .map_err(|_| AppError::internal("protocol_cache_failed"))?;
    if cached.len() >= 128 {
        cached.clear();
    }
    cached.insert(cache_key, result.clone());
    Ok(result)
}
pub async fn write(
    tx: &mut Transaction<'_, Postgres>,
    a: &Claims,
    id: Option<Uuid>,
    expected: Option<i32>,
    mut doc: Protocol,
    reason: &str,
) -> AppResult<Value> {
    if a.role != Role::Master {
        return Err(AppError::forbidden("role_denied"));
    }
    doc.validate()?;
    knowledge::lock_tenant(tx, a.tenant_id).await?;
    let protocol_id = id.unwrap_or_else(Uuid::new_v4);
    let current: Option<i32> =
        sqlx::query_scalar("SELECT version FROM protocols WHERE tenant_id=$1 AND id=$2 FOR UPDATE")
            .bind(a.tenant_id)
            .bind(protocol_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db)?;
    if current != expected || id.is_some() != current.is_some() {
        return Err(AppError::new(
            axum::http::StatusCode::CONFLICT,
            "protocol_version_conflict",
        ));
    }
    let version = current.unwrap_or(0) + 1;
    for task in &doc.source_tasks {
        let present: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tasks WHERE tenant_id=$1 AND id=$2)")
                .bind(a.tenant_id)
                .bind(task)
                .fetch_one(&mut **tx)
                .await
                .map_err(db)?;
        if !present {
            return Err(missing());
        }
    }
    // Protocol membership is versioned separately; add the discoverable tag without replacing evidence.
    let tag = format!("프로토콜:{}", doc.codename);
    for member in &mut doc.references {
        let row=sqlx::query("SELECT k.version,r.document,r.source_task_id,r.submission_id FROM knowledge_items k JOIN knowledge_revisions r ON r.tenant_id=k.tenant_id AND r.item_id=k.id AND r.version=$3 WHERE k.tenant_id=$1 AND k.id=$2").bind(a.tenant_id).bind(member.item_id).bind(member.version).fetch_optional(&mut **tx).await.map_err(db)?.ok_or_else(missing)?;
        let mut information: Document =
            serde_json::from_value(row.get("document")).map_err(|_| bad())?;
        if !information.external_model_allowed {
            doc.external_model_allowed = false;
        }
        if row.get::<i32, _>("version") == member.version && !information.tags.contains(&tag) {
            information.tags.push(tag.clone());
            information.normalize_tags()?;
            member.version += 1;
            knowledge::append_revision(
                tx,
                a,
                member.item_id,
                member.version,
                &information,
                row.get("source_task_id"),
                row.get("submission_id"),
                "Protocol membership tag",
            )
            .await?;
        }
    }
    sqlx::query("INSERT INTO protocols(id,tenant_id,version,codename,description,status) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(id) DO UPDATE SET version=EXCLUDED.version,codename=EXCLUDED.codename,description=EXCLUDED.description,status=EXCLUDED.status").bind(protocol_id).bind(a.tenant_id).bind(version).bind(&doc.codename).bind(&doc.description).bind(&doc.status).execute(&mut **tx).await.map_err(db)?;
    sqlx::query("INSERT INTO protocol_revisions(tenant_id,protocol_id,version,document,actor_task_id,reason) VALUES($1,$2,$3,$4,$5,$6)").bind(a.tenant_id).bind(protocol_id).bind(version).bind(json!(doc)).bind(a.task_id).bind(reason).execute(&mut **tx).await.map_err(db)?;
    for member in &doc.references {
        sqlx::query("INSERT INTO protocol_members(tenant_id,protocol_id,protocol_version,item_id,item_version,required) VALUES($1,$2,$3,$4,$5,$6)").bind(a.tenant_id).bind(protocol_id).bind(version).bind(member.item_id).bind(member.version).bind(member.required).execute(&mut **tx).await.map_err(db)?;
    }
    Ok(json!({"protocol_id":protocol_id,"version":version,"status":doc.status}))
}
pub async fn read(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    if !matches!(a.role, Role::Analyst | Role::Master) {
        return Err(AppError::forbidden("role_denied"));
    }
    #[derive(Deserialize)]
    #[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
    enum Read {
        List {
            #[serde(default)]
            query: String,
        },
        Get {
            protocol_id: Uuid,
            version: Option<i32>,
        },
    }
    match serde_json::from_value(input.clone()).map_err(|_| bad())? {
        Read::List { query } => list(pool, a.tenant_id, &query).await,
        Read::Get {
            protocol_id,
            version,
        } => load(pool, a.tenant_id, protocol_id, version).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_parameters_and_pinned_procedure_are_bounded() {
        let doc = json!({"parameters":[{"name":"week","required":true}],"steps":["Use the pinned scope"],"completion_conditions":["Verified output"]});
        assert!(validate_parameters(&doc, &json!({"week":"41"})).is_ok());
        assert!(validate_parameters(&doc, &json!({"week":""})).is_err());
        assert!(
            validate_parameters(&doc, &json!({"week":"41","scope_override":"outside"})).is_err()
        );
        let fixed = execution_contract(&json!({"id":Uuid::nil(),"version":3,"document":doc}));
        assert_eq!(fixed["version"], 3);
        assert_eq!(fixed["steps"][0], "Use the pinned scope");
    }
}
