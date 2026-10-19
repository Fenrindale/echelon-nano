use crate::{
    error::{AppError, AppResult},
    knowledge::Category,
    policy::{Claims, Role},
};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

fn max_references() -> i64 {
    64
}
fn max_manifest_bytes() -> usize {
    32768
}
fn limit() -> i64 {
    30
}
fn invalid() -> AppError {
    AppError::bad("invalid_context_input")
}
fn missing() -> AppError {
    AppError::new(StatusCode::NOT_FOUND, "context_not_found")
}
fn conflict(code: &'static str) -> AppError {
    AppError::new(StatusCode::CONFLICT, code)
}
fn database(_: sqlx::Error) -> AppError {
    AppError::internal("context_database_failed")
}
fn parse<T: DeserializeOwned>(value: &Value) -> AppResult<T> {
    serde_json::from_value(value.clone()).map_err(|_| invalid())
}
fn encode<T: Serialize>(value: &T) -> AppResult<Value> {
    serde_json::to_value(value).map_err(|_| AppError::internal("context_encoding_failed"))
}
fn bounded(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.len() <= max
}
fn require(a: &Claims, role: Role) -> AppResult<()> {
    if a.role != role {
        return Err(AppError::forbidden("role_denied"));
    }
    Ok(())
}
fn pagination(n: i64) -> AppResult<()> {
    if !(1..=100).contains(&n) {
        return Err(invalid());
    }
    Ok(())
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Criteria {
    need: String,
    #[serde(default)]
    query: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    categories: Vec<Category>,
    #[serde(default)]
    constraints: Vec<String>,
    #[serde(default = "max_references")]
    max_references: i64,
    #[serde(default = "max_manifest_bytes")]
    max_manifest_bytes: usize,
}
impl Criteria {
    fn normalize(&mut self) -> AppResult<()> {
        self.need = self.need.trim().into();
        self.query = self.query.trim().into();
        if !bounded(&self.need, 2000)
            || self.query.len() > 200
            || self.query.split_whitespace().count() > 8
            || self.tags.len() > 16
            || self.categories.len() > 3
            || self.constraints.len() > 8
            || self.constraints.iter().any(|s| !bounded(s, 500))
            || !(1..=128).contains(&self.max_references)
            || !(1024..=65536).contains(&self.max_manifest_bytes)
        {
            return Err(invalid());
        }
        for tag in &mut self.tags {
            *tag = tag.trim().into();
            if !bounded(tag, 128) || tag.chars().any(char::is_control) {
                return Err(invalid());
            }
        }
        self.tags.sort();
        self.tags.dedup();
        self.categories.sort_by_key(|c| match c {
            Category::Data => 0,
            Category::Info => 1,
            Category::Intel => 2,
        });
        self.categories.dedup();
        if self.tags.is_empty() && self.query.is_empty() {
            return Err(AppError::bad("context_search_scope_required"));
        }
        Ok(())
    }
    fn patterns(&self) -> Vec<String> {
        self.query
            .split_whitespace()
            .map(|q| {
                format!(
                    "%{}%",
                    q.replace('\\', "\\\\")
                        .replace('%', "\\%")
                        .replace('_', "\\_")
                )
            })
            .collect()
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestInput {
    request_id: Uuid,
    criteria: Criteria,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    item_id: Uuid,
    version: i32,
    reason: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Delivery {
    request_id: Uuid,
    selections: Vec<Selection>,
    #[serde(default)]
    gaps: Vec<String>,
}
impl Delivery {
    fn validate(&self) -> AppResult<()> {
        if self.selections.len() > 16
            || self.gaps.len() > 8
            || self.gaps.iter().any(|s| !bounded(s, 500))
            || (self.selections.is_empty() && self.gaps.is_empty())
        {
            return Err(invalid());
        }
        let mut seen = BTreeSet::new();
        for selected in &self.selections {
            if selected.version < 1
                || !bounded(&selected.reason, 500)
                || !seen.insert((selected.item_id, selected.version))
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum CurateInput {
    Queue {
        status: Option<String>,
        after_id: Option<Uuid>,
        #[serde(default = "limit")]
        limit: i64,
    },
    Request {
        request_id: Uuid,
    },
    Pack {
        pack_id: Uuid,
    },
    Candidates {
        request_id: Uuid,
        #[serde(default)]
        offset: i64,
        #[serde(default = "limit")]
        limit: i64,
    },
    Deliver {
        request_id: Uuid,
        selections: Vec<Selection>,
        #[serde(default)]
        gaps: Vec<String>,
    },
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum ReadInput {
    Requests {
        after_id: Option<Uuid>,
        #[serde(default = "limit")]
        limit: i64,
    },
    Request {
        request_id: Uuid,
    },
    Pack {
        pack_id: Uuid,
    },
    Resolve {
        pack_id: Uuid,
        ref_id: Uuid,
    },
}
async fn event(
    tx: &mut Transaction<'_, Postgres>,
    a: &Claims,
    action: &str,
    details: Value,
) -> AppResult<()> {
    let role = if a.role == Role::Handler {
        "handler"
    } else {
        "analyst"
    };
    sqlx::query("INSERT INTO audit_records(tenant_id,task_id,actor_role,action,outcome,details) VALUES($1,$2,$3,$4,'completed',$5)")
        .bind(a.tenant_id).bind(a.task_id).bind(role).bind(action).bind(details).execute(&mut **tx).await.map_err(database)?;
    Ok(())
}
fn request_json(row: &PgRow) -> Value {
    json!({"id":row.get::<Uuid,_>("id"),"task_id":row.get::<Uuid,_>("task_id"),"goal":row.get::<String,_>("goal_snapshot"),
        "criteria":row.get::<Value,_>("criteria"),"status":row.get::<String,_>("status"),"pack_id":row.get::<Option<Uuid>,_>("pack_id")})
}
pub async fn request(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    require(a, Role::Handler)?;
    let mut req: RequestInput = parse(input)?;
    req.criteria.normalize()?;
    let criteria = encode(&req.criteria)?;
    let mut tx = pool.begin().await.map_err(database)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!(
            "context:{}:{}:{}",
            a.tenant_id, a.task_id, req.request_id
        ))
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    if let Some(row) = sqlx::query(
        "SELECT * FROM context_requests WHERE tenant_id=$1 AND task_id=$2 AND request_key=$3",
    )
    .bind(a.tenant_id)
    .bind(a.task_id)
    .bind(req.request_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(database)?
    {
        if row.get::<Value, _>("criteria") != criteria {
            return Err(conflict("context_request_conflict"));
        }
        return Ok(json!({"request":request_json(&row),"replayed":true}));
    }
    let goal: String = sqlx::query_scalar("SELECT goal FROM tasks WHERE tenant_id=$1 AND id=$2")
        .bind(a.tenant_id)
        .bind(a.task_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(database)?
        .ok_or_else(missing)?;
    let id = Uuid::new_v4();
    let row=sqlx::query("INSERT INTO context_requests(id,tenant_id,task_id,request_key,requested_by,goal_snapshot,criteria) VALUES($1,$2,$3,$4,$5,$6,$7) RETURNING *")
        .bind(id).bind(a.tenant_id).bind(a.task_id).bind(req.request_id).bind(a.jti).bind(goal).bind(criteria).fetch_one(&mut *tx).await.map_err(database)?;
    event(&mut tx, a, "context.requested", json!({"request_id":id})).await?;
    tx.commit().await.map_err(database)?;
    Ok(json!({"request":request_json(&row),"replayed":false}))
}

// All predicates are bound parameters. Candidate discovery never fetches an external URI.
const MATCHES: &str = "SELECT s.item_id,s.version,s.document,(1 + (SELECT count(*)*3 FROM unnest($2::text[]) p(pattern) WHERE s.document->>'title' ILIKE p.pattern) + (SELECT count(*) FROM unnest($2::text[]) p(pattern) WHERE s.document->>'content' ILIKE p.pattern)) AS score FROM knowledge_revisions s JOIN knowledge_items k ON k.tenant_id=s.tenant_id AND k.id=s.item_id WHERE s.tenant_id=$1 AND (s.version=k.version OR s.document->>'truth_state'='refuted' OR s.document->>'outcome'='failed') AND (cardinality($2::text[])=0 OR EXISTS(SELECT 1 FROM unnest($2::text[]) p(pattern) WHERE concat_ws(' ',s.document->>'title',s.document->>'content',s.document->>'applicability',s.document->'assessment'->>'conditions',s.document->'assessment'->>'observed',s.document->'assessment'->>'avoid') ILIKE p.pattern)) AND (s.document->'tags') @> $3::jsonb AND (cardinality($4::text[])=0 OR s.document->>'category'=ANY($4::text[])) AND (NOT $5 OR s.document->>'truth_state'='refuted' OR s.document->>'outcome'='failed') ORDER BY score DESC,s.item_id,s.version DESC LIMIT $6 OFFSET $7";
async fn matches(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    criteria: &Criteria,
    negative_only: bool,
    n: i64,
    offset: i64,
) -> AppResult<Vec<PgRow>> {
    let categories = criteria
        .categories
        .iter()
        .map(|c| encode(c).and_then(|v| v.as_str().map(String::from).ok_or_else(invalid)))
        .collect::<AppResult<Vec<_>>>()?;
    sqlx::query(MATCHES)
        .bind(tenant)
        .bind(criteria.patterns())
        .bind(json!(criteria.tags))
        .bind(categories)
        .bind(negative_only)
        .bind(n)
        .bind(offset)
        .fetch_all(&mut **tx)
        .await
        .map_err(database)
}
fn preview(document: &Value) -> Value {
    let applicability = document["applicability"].as_str().unwrap_or_default();
    json!({"category":document["category"],"title":document["title"],"truth_state":document["truth_state"],
        "outcome":document["outcome"],"freshness":document["freshness"],"applicability_excerpt":applicability.chars().take(120).collect::<String>(),
        "applicability_truncated":applicability.chars().count()>120})
}
pub async fn curate(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    require(a, Role::Analyst)?;
    match parse::<CurateInput>(input)? {
        CurateInput::Queue {
            status,
            after_id,
            limit,
        } => {
            pagination(limit)?;
            if status
                .as_ref()
                .is_some_and(|s| !matches!(s.as_str(), "pending" | "delivered"))
            {
                return Err(invalid());
            }
            let rows=sqlx::query("SELECT id,task_id,status,pack_id,left(criteria->>'need',160) AS need_preview FROM context_requests WHERE tenant_id=$1 AND ($2::text IS NULL OR status=$2) AND ($3::uuid IS NULL OR id>$3) ORDER BY id LIMIT $4")
                .bind(a.tenant_id).bind(status).bind(after_id).bind(limit).fetch_all(pool).await.map_err(database)?;
            Ok(
                json!({"requests":rows.iter().map(|r|json!({"id":r.get::<Uuid,_>("id"),"task_id":r.get::<Uuid,_>("task_id"),"status":r.get::<String,_>("status"),"pack_id":r.get::<Option<Uuid>,_>("pack_id"),"need_preview":r.get::<String,_>("need_preview")})).collect::<Vec<_>>(),"next_after_id":rows.last().map(|r|r.get::<Uuid,_>("id"))}),
            )
        }
        CurateInput::Request { request_id } => {
            let row = sqlx::query("SELECT * FROM context_requests WHERE tenant_id=$1 AND id=$2")
                .bind(a.tenant_id)
                .bind(request_id)
                .fetch_optional(pool)
                .await
                .map_err(database)?
                .ok_or_else(missing)?;
            Ok(json!({"request":request_json(&row)}))
        }
        CurateInput::Pack { pack_id } => {
            let manifest: Value = sqlx::query_scalar(
                "SELECT manifest FROM context_packs WHERE tenant_id=$1 AND id=$2",
            )
            .bind(a.tenant_id)
            .bind(pack_id)
            .fetch_optional(pool)
            .await
            .map_err(database)?
            .ok_or_else(missing)?;
            Ok(json!({"pack":manifest}))
        }
        CurateInput::Candidates {
            request_id,
            offset,
            limit,
        } => {
            pagination(limit)?;
            if !(0..=10000).contains(&offset) {
                return Err(invalid());
            }
            let mut tx = pool.begin().await.map_err(database)?;
            let criteria: Value = sqlx::query_scalar(
                "SELECT criteria FROM context_requests WHERE tenant_id=$1 AND id=$2",
            )
            .bind(a.tenant_id)
            .bind(request_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(database)?
            .ok_or_else(missing)?;
            let criteria: Criteria = parse(&criteria)?;
            let rows = matches(&mut tx, a.tenant_id, &criteria, false, limit + 1, offset).await?;
            let more = rows.len() > limit as usize;
            let candidates=rows.iter().take(limit as usize).map(|r|{let document:Value=r.get("document");json!({"item_id":r.get::<Uuid,_>("item_id"),"version":r.get::<i32,_>("version"),"score":r.get::<i64,_>("score"),"preview":preview(&document)})}).collect::<Vec<_>>();
            Ok(
                json!({"candidates":candidates,"next_offset":if more {Some(offset+limit)} else {None}}),
            )
        }
        CurateInput::Deliver {
            request_id,
            selections,
            gaps,
        } => {
            deliver(
                pool,
                a,
                Delivery {
                    request_id,
                    selections,
                    gaps,
                },
            )
            .await
        }
    }
}

fn document_hash(document: &Value) -> AppResult<String> {
    let bytes =
        serde_json::to_vec(document).map_err(|_| AppError::internal("context_encoding_failed"))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}
fn negative(document: &Value) -> bool {
    document["truth_state"] == "refuted" || document["outcome"] == "failed"
}
fn caution(document: &Value) -> bool {
    negative(document)
        || document["truth_state"] == "conflicted"
        || document["freshness"] != "current"
}

const CLOSURE: &str = "WITH RECURSIVE closure(item_id,version) AS (SELECT * FROM unnest($2::uuid[],$3::int[]) UNION SELECT n.item_id,n.version FROM closure c CROSS JOIN LATERAL (SELECT e.target_id AS item_id,e.target_version AS version FROM knowledge_edges e WHERE e.tenant_id=$1 AND e.source_id=c.item_id AND e.source_version=c.version UNION SELECT r.item_id,r.version FROM knowledge_revisions r WHERE r.tenant_id=$1 AND r.item_id=c.item_id AND (r.document->>'truth_state'='refuted' OR r.document->>'outcome'='failed') UNION SELECT k.id,k.version FROM knowledge_items k WHERE k.tenant_id=$1 AND k.id=c.item_id UNION SELECT e.source_id,e.source_version FROM knowledge_edges e JOIN knowledge_items k ON k.tenant_id=e.tenant_id AND k.id=e.source_id AND k.version=e.source_version WHERE e.tenant_id=$1 AND e.target_id=c.item_id AND e.kind='contradicts') n) SELECT item_id,version FROM closure LIMIT $4";

async fn deliver(pool: &PgPool, a: &Claims, delivery: Delivery) -> AppResult<Value> {
    delivery.validate()?;
    let parameters = encode(&delivery)?;
    let mut tx = pool.begin().await.map_err(database)?;
    sqlx::query("SET LOCAL statement_timeout='5s'")
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    let row = sqlx::query("SELECT * FROM context_requests WHERE tenant_id=$1 AND id=$2 FOR UPDATE")
        .bind(a.tenant_id)
        .bind(delivery.request_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(database)?
        .ok_or_else(missing)?;
    if row.get::<String, _>("status") == "delivered" {
        if row.get::<Option<Value>, _>("delivery").as_ref() != Some(&parameters) {
            return Err(conflict("context_already_delivered"));
        }
        let manifest: Value =
            sqlx::query_scalar("SELECT manifest FROM context_packs WHERE tenant_id=$1 AND id=$2")
                .bind(a.tenant_id)
                .bind(row.get::<Option<Uuid>, _>("pack_id"))
                .fetch_one(&mut *tx)
                .await
                .map_err(database)?;
        return Ok(json!({"pack":manifest,"replayed":true}));
    }
    // The same lock guards knowledge publication and classification. This gives
    // the graph a consistent view while request-row locking serializes retries.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('knowledge:' || $1::text,0))")
        .bind(a.tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    let criteria: Criteria = parse(&row.get::<Value, _>("criteria"))?;
    let task: Uuid = row.get("task_id");
    let mut roots = BTreeMap::new();
    let mut seeds = BTreeSet::new();
    for selected in &delivery.selections {
        let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_revisions WHERE tenant_id=$1 AND item_id=$2 AND version=$3)")
            .bind(a.tenant_id).bind(selected.item_id).bind(selected.version).fetch_one(&mut *tx).await.map_err(database)?;
        if !exists {
            return Err(AppError::bad("context_selection_unavailable"));
        }
        roots.insert(
            (selected.item_id, selected.version),
            selected.reason.clone(),
        );
        seeds.insert((selected.item_id, selected.version));
    }
    let matching_negative = matches(
        &mut tx,
        a.tenant_id,
        &criteria,
        true,
        criteria.max_references + 1,
        0,
    )
    .await?;
    for r in matching_negative {
        seeds.insert((r.get("item_id"), r.get("version")));
    }
    if seeds.len() > criteria.max_references as usize {
        return Err(conflict("context_reference_budget_exceeded"));
    }
    let ids: Vec<Uuid> = seeds.iter().map(|r| r.0).collect();
    let versions: Vec<i32> = seeds.iter().map(|r| r.1).collect();
    let closure = sqlx::query(CLOSURE)
        .bind(a.tenant_id)
        .bind(ids)
        .bind(versions)
        .bind(criteria.max_references + 1)
        .fetch_all(&mut *tx)
        .await
        .map_err(database)?;
    if closure.len() > criteria.max_references as usize {
        return Err(conflict("context_reference_budget_exceeded"));
    }
    let pack_id = Uuid::new_v4();
    let mut knowledge = Vec::new();
    let mut cautions = Vec::new();
    let mut references = Vec::new();
    let mut members: BTreeSet<(Uuid, i32)> = BTreeSet::new();
    for node in &closure {
        members.insert((node.get("item_id"), node.get("version")));
    }
    let member_ids: Vec<Uuid> = members.iter().map(|v| v.0).collect();
    let member_versions: Vec<i32> = members.iter().map(|v| v.1).collect();
    let opposing=sqlx::query("SELECT source_id,source_version,target_id,target_version FROM knowledge_edges WHERE tenant_id=$1 AND kind='contradicts' AND (source_id,source_version) IN (SELECT * FROM unnest($2::uuid[],$3::int[]))")
        .bind(a.tenant_id).bind(member_ids).bind(member_versions).fetch_all(&mut *tx).await.map_err(database)?;
    let mut contradictions = BTreeSet::new();
    for edge in opposing {
        contradictions.insert((
            edge.get::<Uuid, _>("source_id"),
            edge.get::<i32, _>("source_version"),
        ));
        contradictions.insert((
            edge.get::<Uuid, _>("target_id"),
            edge.get::<i32, _>("target_version"),
        ));
    }
    for (item, version) in members {
        let revision=sqlx::query("SELECT r.document,k.version AS current_version FROM knowledge_revisions r JOIN knowledge_items k ON k.tenant_id=r.tenant_id AND k.id=r.item_id WHERE r.tenant_id=$1 AND r.item_id=$2 AND r.version=$3")
            .bind(a.tenant_id).bind(item).bind(version).fetch_one(&mut *tx).await.map_err(database)?;
        let document: Value = revision.get("document");
        let current: i32 = revision.get("current_version");
        let ref_id = Uuid::new_v4();
        let hash = document_hash(&document)?;
        let mut codes = vec![];
        if roots.contains_key(&(item, version)) {
            codes.push("selected");
        } else {
            codes.push("related_evidence");
        }
        if negative(&document) {
            codes.push("negative_history");
        }
        if document["truth_state"] == "refuted" {
            codes.push("refuted");
        }
        if document["outcome"] == "failed" {
            codes.push("failed");
        }
        if document["truth_state"] == "conflicted" {
            codes.push("conflicted");
        }
        if document["freshness"] != "current" {
            codes.push("freshness_review");
        }
        if version != current {
            codes.push("newer_revision_exists");
        }
        let contradiction = contradictions.contains(&(item, version));
        if contradiction {
            codes.push("contradiction_related");
        }
        let header = json!({"ref_id":ref_id,"item_id":item,"version":version,"preview":preview(&document),"reason_codes":codes,
            "selection_reason":roots.get(&(item,version)),"document_sha256":hash});
        if caution(&document) || version != current || contradiction {
            cautions.push(header);
        } else {
            knowledge.push(header);
        }
        references.push((ref_id, item, version, hash));
    }
    let manifest = json!({"id":pack_id,"request_id":delivery.request_id,"task_id":task,"goal":row.get::<String,_>("goal_snapshot"),
        "need":criteria.need,"constraints":criteria.constraints,"knowledge":knowledge,"cautions":cautions,"gaps":delivery.gaps,
        "reference_count":references.len(),"format":"echelon.context.v1"});
    if serde_json::to_vec(&manifest).map_err(|_| invalid())?.len() > criteria.max_manifest_bytes {
        return Err(conflict("context_manifest_budget_exceeded"));
    }
    sqlx::query("INSERT INTO context_packs(id,tenant_id,task_id,context_request_id,created_by,manifest) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(pack_id).bind(a.tenant_id).bind(task).bind(delivery.request_id).bind(a.jti).bind(&manifest).execute(&mut *tx).await.map_err(database)?;
    for (id, item, version, hash) in references {
        sqlx::query("INSERT INTO context_pack_refs(id,tenant_id,task_id,pack_id,item_id,version,document_sha256) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(id).bind(a.tenant_id).bind(task).bind(pack_id).bind(item).bind(version).bind(hash).execute(&mut *tx).await.map_err(database)?;
    }
    sqlx::query("UPDATE context_requests SET status='delivered',pack_id=$3,delivery=$4 WHERE tenant_id=$1 AND id=$2").bind(a.tenant_id).bind(delivery.request_id).bind(pack_id).bind(parameters).execute(&mut *tx).await.map_err(database)?;
    event(&mut tx,a,"context.delivered",json!({"request_id":delivery.request_id,"pack_id":pack_id,"target_task_id":task,"reference_count":manifest["reference_count"]})).await?;
    tx.commit().await.map_err(database)?;
    Ok(json!({"pack":manifest,"replayed":false}))
}

pub async fn read(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    require(a, Role::Handler)?;
    match parse::<ReadInput>(input)? {
        ReadInput::Requests { after_id, limit } => {
            pagination(limit)?;
            let rows=sqlx::query("SELECT id,status,pack_id,left(criteria->>'need',160) AS need_preview FROM context_requests WHERE tenant_id=$1 AND task_id=$2 AND ($3::uuid IS NULL OR id>$3) ORDER BY id LIMIT $4")
                .bind(a.tenant_id).bind(a.task_id).bind(after_id).bind(limit).fetch_all(pool).await.map_err(database)?;
            Ok(
                json!({"requests":rows.iter().map(|r|json!({"id":r.get::<Uuid,_>("id"),"status":r.get::<String,_>("status"),"pack_id":r.get::<Option<Uuid>,_>("pack_id"),"need_preview":r.get::<String,_>("need_preview")})).collect::<Vec<_>>(),"next_after_id":rows.last().map(|r|r.get::<Uuid,_>("id"))}),
            )
        }
        ReadInput::Request { request_id } => {
            let row = sqlx::query(
                "SELECT * FROM context_requests WHERE tenant_id=$1 AND task_id=$2 AND id=$3",
            )
            .bind(a.tenant_id)
            .bind(a.task_id)
            .bind(request_id)
            .fetch_optional(pool)
            .await
            .map_err(database)?
            .ok_or_else(missing)?;
            Ok(json!({"request":request_json(&row)}))
        }
        ReadInput::Pack { pack_id } => {
            let manifest: Value = sqlx::query_scalar(
                "SELECT manifest FROM context_packs WHERE tenant_id=$1 AND task_id=$2 AND id=$3",
            )
            .bind(a.tenant_id)
            .bind(a.task_id)
            .bind(pack_id)
            .fetch_optional(pool)
            .await
            .map_err(database)?
            .ok_or_else(missing)?;
            Ok(json!({"pack":manifest}))
        }
        ReadInput::Resolve { pack_id, ref_id } => resolve(pool, a, pack_id, ref_id).await,
    }
}
async fn resolve(pool: &PgPool, a: &Claims, pack: Uuid, reference: Uuid) -> AppResult<Value> {
    let row=sqlx::query("SELECT f.item_id,f.version,f.document_sha256,r.document,r.source_task_id,r.submission_id,r.created_at::text AS recorded_at,k.version AS current_version,k.truth_state,k.outcome,h.document->>'freshness' AS current_freshness FROM context_pack_refs f JOIN knowledge_revisions r ON r.tenant_id=f.tenant_id AND r.item_id=f.item_id AND r.version=f.version JOIN knowledge_items k ON k.tenant_id=f.tenant_id AND k.id=f.item_id JOIN knowledge_revisions h ON h.tenant_id=k.tenant_id AND h.item_id=k.id AND h.version=k.version WHERE f.tenant_id=$1 AND f.task_id=$2 AND f.pack_id=$3 AND f.id=$4")
        .bind(a.tenant_id).bind(a.task_id).bind(pack).bind(reference).fetch_optional(pool).await.map_err(database)?.ok_or_else(missing)?;
    let document: Value = row.get("document");
    let hash: String = row.get("document_sha256");
    if document_hash(&document)? != hash {
        return Err(AppError::internal("context_reference_integrity_failed"));
    }
    let item: Uuid = row.get("item_id");
    let version: i32 = row.get("version");
    let current: i32 = row.get("current_version");
    let links=sqlx::query("SELECT e.kind,f.id AS ref_id,e.target_id,e.target_version FROM knowledge_edges e JOIN context_pack_refs f ON f.tenant_id=e.tenant_id AND f.item_id=e.target_id AND f.version=e.target_version WHERE e.tenant_id=$1 AND e.source_id=$2 AND e.source_version=$3 AND f.task_id=$4 AND f.pack_id=$5 ORDER BY e.kind,e.target_id,e.target_version")
        .bind(a.tenant_id).bind(item).bind(version).bind(a.task_id).bind(pack).fetch_all(pool).await.map_err(database)?;
    let changed = current != version;
    let truth: String = row.get("truth_state");
    let outcome: String = row.get("outcome");
    let freshness: String = row.get("current_freshness");
    let notice = json!({"current_version":current,"version_changed":changed,"truth_state":truth,"outcome":outcome,"freshness":freshness,
        "refresh_context_recommended":changed || matches!(truth.as_str(),"refuted"|"conflicted") || outcome=="failed" || freshness!="current"});
    let mut tx = pool.begin().await.map_err(database)?;
    event(
        &mut tx,
        a,
        "context.resolved",
        json!({"pack_id":pack,"ref_id":reference,"item_id":item,"version":version}),
    )
    .await?;
    tx.commit().await.map_err(database)?;
    Ok(
        json!({"reference":{"ref_id":reference,"item_id":item,"version":version,"document_sha256":hash},"document":document,
        "provenance":{"source_task_id":row.get::<Option<Uuid>,_>("source_task_id"),"submission_id":row.get::<Option<Uuid>,_>("submission_id"),"recorded_at":row.get::<String,_>("recorded_at")},
        "live_notice":notice,"links":links.iter().map(|r|json!({"kind":r.get::<String,_>("kind"),"ref_id":r.get::<Uuid,_>("ref_id"),"item_id":r.get::<Uuid,_>("target_id"),"version":r.get::<i32,_>("target_version")})).collect::<Vec<_>>()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn context_criteria_require_a_bounded_search_and_canonical_retry_identity() {
        let mut c: Criteria =
            parse(&json!({"need":"Report context","tags":[" system:aws ","system:aws"]})).unwrap();
        c.normalize().unwrap();
        assert_eq!(c.tags, vec!["system:aws"]);
        let mut empty: Criteria =
            parse(&json!({"need":"Do not load the entire database"})).unwrap();
        assert_eq!(
            empty.normalize().unwrap_err().code,
            "context_search_scope_required"
        );
        c.query = "literal_%".into();
        assert_eq!(c.patterns(), vec!["%literal\\_\\%%"]);
        c.max_references = 129;
        assert!(c.normalize().is_err());
    }
    #[test]
    fn manifest_previews_exclude_bodies_and_hashes_pin_the_whole_document() {
        let mut d = json!({"title":"Reference","content":"body that must be resolved on demand","assessment":{"observed":"sensitive failure detail"},"freshness":"current"});
        let header = preview(&d);
        assert!(header.get("content").is_none());
        assert!(header.get("assessment").is_none());
        let hash = document_hash(&d).unwrap();
        d["content"] = json!("new version");
        assert_ne!(hash, document_hash(&d).unwrap());
        assert!(parse::<ReadInput>(&json!({"op":"resolve","pack_id":Uuid::new_v4(),"ref_id":Uuid::new_v4(),"version":999})).is_err());
    }
}
