use crate::{
    error::{AppError, AppResult},
    policy::{Claims, Role},
};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use uuid::Uuid;

macro_rules! vocabulary {
    ($name:ident { $($variant:ident),+ }) => {
        #[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
    };
}
vocabulary!(Category { Data, Info, Intel });
vocabulary!(StorageMode {
    Reference,
    Excerpt,
    Artifact,
    Hybrid
});
vocabulary!(TruthState {
    Unverified,
    Supported,
    Refuted,
    Conflicted
});
vocabulary!(Outcome {
    NotApplicable,
    Success,
    Failed,
    Partial,
    Blocked
});
vocabulary!(Freshness {
    Current,
    ReviewNeeded,
    Expired,
    Superseded
});
vocabulary!(RelationKind {
    References,
    DerivedFrom,
    Supports,
    Contradicts,
    DependsOn,
    Supersedes
});

fn unverified() -> TruthState {
    TruthState::Unverified
}
fn not_applicable() -> Outcome {
    Outcome::NotApplicable
}
fn current() -> Freshness {
    Freshness::Current
}
fn limit() -> i64 {
    30
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub uri: String,
    #[serde(default)]
    pub locator: String,
    #[serde(default)]
    pub note: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    pub kind: RelationKind,
    pub target_id: Uuid,
    pub target_version: i32,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub conditions: String,
    #[serde(default)]
    pub attempted_action: String,
    pub observed: String,
    pub reason: String,
    pub avoid: String,
    pub retry_when: String,
}
/// Curation metadata: an explicit future use and the submitted evidence supporting it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Reuse {
    pub applicable_when: String,
    pub decision_or_action: String,
    pub evidence: String,
}
fn task_scope() -> String {
    "task".into()
}
fn is_task_scope(v: &String) -> bool {
    v == "task"
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Document {
    #[serde(default = "task_scope", skip_serializing_if = "is_task_scope")]
    pub retention_scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reuse: Option<Reuse>,
    #[serde(default = "allow_external", skip_serializing_if = "is_allowed")]
    pub external_model_allowed: bool,
    pub category: Category,
    pub title: String,
    pub storage_mode: StorageMode,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sources: Vec<Source>,
    #[serde(default)]
    pub relations: Vec<Relation>,
    #[serde(default = "unverified")]
    pub truth_state: TruthState,
    #[serde(default = "not_applicable")]
    pub outcome: Outcome,
    #[serde(default = "current")]
    pub freshness: Freshness,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<u64>,
    #[serde(default)]
    pub applicability: String,
    pub assessment: Option<Assessment>,
}
fn allow_external() -> bool {
    true
}
fn is_allowed(v: &bool) -> bool {
    *v
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max
}
fn invalid() -> AppError {
    AppError::bad("invalid_knowledge_input")
}
fn conflict(code: &'static str) -> AppError {
    AppError::new(StatusCode::CONFLICT, code)
}
fn missing() -> AppError {
    AppError::new(StatusCode::NOT_FOUND, "knowledge_not_found")
}
fn database(_: sqlx::Error) -> AppError {
    AppError::internal("knowledge_database_failed")
}
fn parse<T: DeserializeOwned>(input: &Value) -> AppResult<T> {
    serde_json::from_value(input.clone()).map_err(|_| invalid())
}
fn encoded<T: Serialize>(value: &T) -> AppResult<Value> {
    serde_json::to_value(value).map_err(|_| AppError::internal("knowledge_encoding_failed"))
}
fn label<T: Serialize>(value: &T) -> AppResult<String> {
    encoded(value)?
        .as_str()
        .map(String::from)
        .ok_or_else(invalid)
}
fn require(a: &Claims, roles: &[Role]) -> AppResult<()> {
    if !roles.contains(&a.role) {
        return Err(AppError::forbidden("role_denied"));
    }
    Ok(())
}

impl Document {
    pub fn validate_reuse(&self) -> AppResult<()> {
        let proof = self
            .reuse
            .as_ref()
            .ok_or_else(|| AppError::bad("central_reuse_required"))?;
        if [
            &proof.applicable_when,
            &proof.decision_or_action,
            &proof.evidence,
        ]
        .iter()
        .any(|v| !bounded(v, 1600) || v.chars().count() < 8)
            || !(self.content.contains(&proof.evidence)
                || self.sources.iter().any(|s| s.uri == proof.evidence))
        {
            return Err(AppError::bad("central_reuse_not_grounded"));
        }
        Ok(())
    }

    pub fn normalize_tags(&mut self) -> AppResult<()> {
        if self.tags.len() > 34 {
            return Err(invalid());
        }
        for tag in &mut self.tags {
            *tag = tag.trim().to_string();
            if !bounded(tag, 128) || tag.chars().any(char::is_control) {
                return Err(invalid());
            }
        }
        self.tags.retain(|t| t != "거짓" && t != "실패");
        self.tags.sort();
        self.tags.dedup();
        if self.tags.len() > 32 {
            return Err(invalid());
        }
        if self.truth_state == TruthState::Refuted {
            self.tags.push("거짓".into());
        }
        if self.outcome == Outcome::Failed {
            self.tags.push("실패".into());
        }
        self.tags.sort();
        self.tags.dedup();
        Ok(())
    }
    pub fn validate(&mut self) -> AppResult<()> {
        if !matches!(self.retention_scope.as_str(), "central" | "task") {
            return Err(invalid());
        }
        if self.retention_scope == "central" {
            self.validate_reuse()?;
        }
        self.title = self.title.trim().to_string();
        if !bounded(&self.title, 240)
            || self.content.len() > 16000
            || self.applicability.len() > 2000
            || self.sources.len() > 12
            || self.relations.len() > 32
            || self.valid_until.is_some_and(|v| v == 0 || v > 253402300799)
        {
            return Err(invalid());
        }
        self.normalize_tags()?;
        for source in &self.sources {
            if source.uri.len() > 2048 || source.locator.len() > 500 || source.note.len() > 1000 {
                return Err(invalid());
            }
            let uri = url::Url::parse(&source.uri).map_err(|_| invalid())?;
            if !matches!(uri.scheme(), "https" | "http" | "artifact")
                || uri.host_str().is_none()
                || !uri.username().is_empty()
                || uri.password().is_some()
            {
                return Err(invalid());
            }
            if uri.query_pairs().any(|(key, _)| {
                matches!(
                    key.to_ascii_lowercase().as_str(),
                    "access_token"
                        | "refresh_token"
                        | "id_token"
                        | "password"
                        | "api_key"
                        | "client_secret"
                        | "signature"
                )
            }) {
                return Err(invalid());
            }
        }
        for edge in &self.relations {
            if edge.target_version < 1 {
                return Err(invalid());
            }
        }
        for (index, edge) in self.relations.iter().enumerate() {
            if self.relations[..index].contains(edge) {
                return Err(invalid());
            }
        }
        let content = !self.content.trim().is_empty();
        let valid_storage = match self.storage_mode {
            StorageMode::Reference => !content && !self.sources.is_empty(),
            StorageMode::Excerpt => {
                content && (!self.sources.is_empty() || !self.relations.is_empty())
            }
            StorageMode::Hybrid => content && !self.sources.is_empty(),
            StorageMode::Artifact => {
                !content
                    && self
                        .sources
                        .iter()
                        .any(|s| s.uri.starts_with("artifact://"))
            }
        };
        if !valid_storage {
            return Err(AppError::bad("knowledge_storage_invalid"));
        }
        if self.truth_state == TruthState::Refuted || self.outcome == Outcome::Failed {
            let assessment = self
                .assessment
                .as_ref()
                .ok_or_else(|| AppError::bad("negative_evidence_required"))?;
            for text in [
                &assessment.conditions,
                &assessment.observed,
                &assessment.reason,
                &assessment.avoid,
                &assessment.retry_when,
            ] {
                if !bounded(text, 2000) {
                    return Err(AppError::bad("negative_evidence_required"));
                }
            }
            if self.outcome == Outcome::Failed && !bounded(&assessment.attempted_action, 2000) {
                return Err(AppError::bad("negative_evidence_required"));
            }
        }
        if self.assessment.as_ref().is_some_and(|v| {
            [
                &v.conditions,
                &v.attempted_action,
                &v.observed,
                &v.reason,
                &v.avoid,
                &v.retry_when,
            ]
            .iter()
            .any(|s| s.len() > 2000)
        }) {
            return Err(invalid());
        }
        Ok(())
    }
}
pub fn effective_freshness(document: &Value) -> &str {
    let explicit = document["freshness"].as_str().unwrap_or("current");
    if explicit != "superseded"
        && document["valid_until"]
            .as_u64()
            .is_some_and(|v| v <= crate::unix_time())
    {
        "expired"
    } else {
        explicit
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum SubmitInput {
    Submit {
        request_id: Uuid,
        target_id: Option<Uuid>,
        base_version: Option<i32>,
        document: Box<Document>,
        rationale: String,
    },
    Status {
        submission_id: Uuid,
    },
}
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Decision {
    Accept,
    Local,
    Reject,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PublishInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reuse: Option<Reuse>,
    submission_id: Uuid,
    decision: Decision,
    reason: String,
    category: Option<Category>,
    tags: Option<Vec<String>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifyInput {
    request_id: Option<Uuid>,
    retention_scope: Option<String>,
    reuse: Option<Reuse>,
    item_id: Uuid,
    expected_version: i32,
    category: Option<Category>,
    tags: Option<Vec<String>>,
    reason: String,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum ReadInput {
    List {
        retention_scope: Option<String>,
        source_task_id: Option<Uuid>,
        category: Option<Category>,
        #[serde(default)]
        tags: Vec<String>,
        query: Option<String>,
        #[serde(default)]
        negative_history: bool,
        after_id: Option<Uuid>,
        #[serde(default = "limit")]
        limit: i64,
    },
    Get {
        item_id: Uuid,
        version: Option<i32>,
    },
    History {
        item_id: Uuid,
        before_version: Option<i32>,
        #[serde(default = "limit")]
        limit: i64,
    },
    Submissions {
        status: Option<String>,
        source_task_id: Option<Uuid>,
        after_id: Option<Uuid>,
        #[serde(default = "limit")]
        limit: i64,
    },
    Submission {
        submission_id: Uuid,
    },
}

pub(crate) async fn lock_tenant(tx: &mut Transaction<'_, Postgres>, tenant: Uuid) -> AppResult<()> {
    // Serialize graph writes within one tenant, including concurrent dependency edits.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('knowledge:' || $1::text,0))")
        .bind(tenant.to_string())
        .execute(&mut **tx)
        .await
        .map_err(database)?;
    Ok(())
}
async fn event(
    tx: &mut Transaction<'_, Postgres>,
    a: &Claims,
    action: &str,
    details: Value,
) -> AppResult<()> {
    sqlx::query("INSERT INTO audit_records(tenant_id,task_id,actor_role,action,outcome,details) VALUES($1,$2,$3,$4,'completed',$5)")
        .bind(a.tenant_id).bind(a.task_id).bind(label(&a.role)?).bind(action).bind(details)
        .execute(&mut **tx).await.map_err(database)?;
    Ok(())
}
pub(crate) async fn check_relations(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    item: Option<Uuid>,
    document: &Document,
) -> AppResult<()> {
    for edge in &document.relations {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_revisions WHERE tenant_id=$1 AND item_id=$2 AND version=$3)")
            .bind(tenant).bind(edge.target_id).bind(edge.target_version).fetch_one(&mut **tx).await.map_err(database)?;
        if !exists {
            return Err(AppError::bad("relation_target_unavailable"));
        }
        if edge.kind == RelationKind::DependsOn
            && let Some(item) = item
        {
            let cycle:bool=sqlx::query_scalar("WITH RECURSIVE reachable(id) AS (SELECT $2::uuid UNION SELECT e.target_id FROM knowledge_edges e JOIN knowledge_items k ON k.tenant_id=e.tenant_id AND k.id=e.source_id AND k.version=e.source_version JOIN reachable r ON r.id=e.source_id WHERE e.tenant_id=$1 AND e.kind='depends_on') SELECT EXISTS(SELECT 1 FROM reachable WHERE id=$3)")
                .bind(tenant).bind(edge.target_id).bind(item).fetch_one(&mut **tx).await.map_err(database)?;
            if cycle {
                return Err(AppError::bad("dependency_cycle"));
            }
        }
    }
    Ok(())
}

pub async fn submit(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    require(a, &[Role::Handler])?;
    let req: SubmitInput = parse(input)?;
    match req {
        SubmitInput::Status { submission_id } => {
            let row=sqlx::query("SELECT * FROM knowledge_submissions WHERE id=$1 AND tenant_id=$2 AND source_task_id=$3")
                .bind(submission_id).bind(a.tenant_id).bind(a.task_id).fetch_optional(pool).await.map_err(database)?.ok_or_else(missing)?;
            Ok(json!({"submission":submission_json(&row)}))
        }
        SubmitInput::Submit {
            request_id,
            target_id,
            base_version,
            mut document,
            rationale,
        } => {
            if !bounded(&rationale, 2000)
                || !matches!(
                    (target_id, base_version),
                    (None, None) | (Some(_), Some(1..))
                )
            {
                return Err(invalid());
            }
            document.validate()?;
            let contract: Value =
                sqlx::query_scalar("SELECT contract FROM tasks WHERE tenant_id=$1 AND id=$2")
                    .bind(a.tenant_id)
                    .bind(a.task_id)
                    .fetch_one(pool)
                    .await
                    .map_err(database)?;
            if contract["model_access"] == "blocked" {
                document.external_model_allowed = false;
            }
            for source in &document.sources {
                let uri = url::Url::parse(&source.uri).map_err(|_| invalid())?;
                if uri.scheme() == "artifact" {
                    let artifact: Uuid = uri
                        .host_str()
                        .and_then(|h| Uuid::parse_str(h).ok())
                        .ok_or_else(invalid)?;
                    let row=sqlx::query("SELECT f.task_id,t.contract FROM artifacts f JOIN tasks t ON t.tenant_id=f.tenant_id AND t.id=f.task_id WHERE f.tenant_id=$1 AND f.id=$2").bind(a.tenant_id).bind(artifact).fetch_optional(pool).await.map_err(database)?.ok_or_else(missing)?;
                    if row.get::<Uuid, _>("task_id") != a.task_id {
                        let permitted:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM context_pack_refs p JOIN knowledge_revisions r ON r.tenant_id=p.tenant_id AND r.item_id=p.item_id AND r.version=p.version WHERE p.tenant_id=$1 AND p.task_id=$2 AND r.document->'sources' @> $3::jsonb)").bind(a.tenant_id).bind(a.task_id).bind(json!([{"uri":source.uri}])).fetch_one(pool).await.map_err(database)?;
                        if !permitted {
                            return Err(AppError::forbidden("artifact_source_scope_denied"));
                        }
                    }
                    if row.get::<Value, _>("contract")["model_access"] == "blocked" {
                        document.external_model_allowed = false;
                    }
                }
            }
            for relation in &document.relations {
                let allowed:Option<bool>=sqlx::query_scalar("SELECT k.external_model_allowed AND COALESCE((r.document->>'external_model_allowed')::bool,true) FROM knowledge_revisions r JOIN knowledge_items k ON k.tenant_id=r.tenant_id AND k.id=r.item_id WHERE r.tenant_id=$1 AND r.item_id=$2 AND r.version=$3").bind(a.tenant_id).bind(relation.target_id).bind(relation.target_version).fetch_optional(pool).await.map_err(database)?;
                if allowed == Some(false) {
                    document.external_model_allowed = false;
                }
            }
            if let Some(target) = target_id {
                let allowed:Option<bool>=sqlx::query_scalar("SELECT COALESCE((r.document->>'external_model_allowed')::bool,true) FROM knowledge_items k JOIN knowledge_revisions r ON r.tenant_id=k.tenant_id AND r.item_id=k.id AND r.version=k.version WHERE k.tenant_id=$1 AND k.id=$2").bind(a.tenant_id).bind(target).fetch_optional(pool).await.map_err(database)?;
                if allowed == Some(false) {
                    document.external_model_allowed = false;
                }
            }
            let value = encoded(&document)?;
            let mut tx = pool.begin().await.map_err(database)?;
            // Retrying the same Task request must not create another queue entry.
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
                .bind(format!(
                    "submission:{}:{}:{}",
                    a.tenant_id, a.task_id, request_id
                ))
                .execute(&mut *tx)
                .await
                .map_err(database)?;
            if let Some(row)=sqlx::query("SELECT * FROM knowledge_submissions WHERE tenant_id=$1 AND source_task_id=$2 AND request_id=$3")
                .bind(a.tenant_id).bind(a.task_id).bind(request_id).fetch_optional(&mut *tx).await.map_err(database)? {
                if row.get::<Value,_>("document")!=value || row.get::<Option<Uuid>,_>("target_id")!=target_id || row.get::<Option<i32>,_>("base_version")!=base_version || row.get::<String,_>("rationale")!=rationale {
                    return Err(conflict("submission_request_conflict"));
                }
                return Ok(json!({"submission":submission_json(&row),"replayed":true}));
            }
            if let Some(id) = target_id {
                let version: Option<i32> = sqlx::query_scalar(
                    "SELECT version FROM knowledge_items WHERE tenant_id=$1 AND id=$2",
                )
                .bind(a.tenant_id)
                .bind(id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(database)?;
                if version.is_none() {
                    return Err(missing());
                }
                if version != base_version {
                    return Err(conflict("knowledge_version_conflict"));
                }
            }
            check_relations(&mut tx, a.tenant_id, target_id, &document).await?;
            let id = Uuid::new_v4();
            let row=sqlx::query("INSERT INTO knowledge_submissions(id,tenant_id,source_task_id,request_id,submitted_by,target_id,base_version,document,rationale) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING *")
                .bind(id).bind(a.tenant_id).bind(a.task_id).bind(request_id).bind(a.jti).bind(target_id).bind(base_version).bind(value).bind(rationale)
                .fetch_one(&mut *tx).await.map_err(database)?;
            event(
                &mut tx,
                a,
                "knowledge.submitted",
                json!({"submission_id":id}),
            )
            .await?;
            tx.commit().await.map_err(database)?;
            Ok(json!({"submission":submission_json(&row),"replayed":false}))
        }
    }
}

pub async fn publish(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    require(a, &[Role::Analyst])?;
    let req: PublishInput = parse(input)?;
    if !bounded(&req.reason, 2000)
        || (req.decision == Decision::Reject
            && (req.category.is_some() || req.tags.is_some() || req.reuse.is_some()))
    {
        return Err(invalid());
    }
    let mut tx = pool.begin().await.map_err(database)?;
    let row =
        sqlx::query("SELECT * FROM knowledge_submissions WHERE tenant_id=$1 AND id=$2 FOR UPDATE")
            .bind(a.tenant_id)
            .bind(req.submission_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(database)?
            .ok_or_else(missing)?;
    let decision = encoded(&req)?;
    if row.get::<String, _>("status") != "pending" {
        if row.get::<Option<Value>, _>("decision").as_ref() != Some(&decision) {
            return Err(conflict("submission_already_decided"));
        }
        return Ok(json!({"submission":submission_json(&row),"replayed":true}));
    }
    let mut result_item = None;
    let mut result_version = None;
    let status = match req.decision {
        Decision::Reject => "rejected",
        Decision::Accept | Decision::Local => {
            lock_tenant(&mut tx, a.tenant_id).await?;
            let target: Option<Uuid> = row.get("target_id");
            let id = target.unwrap_or_else(Uuid::new_v4);
            let version = if let Some(target) = target {
                let current: i32 = sqlx::query_scalar(
                    "SELECT version FROM knowledge_items WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
                )
                .bind(a.tenant_id)
                .bind(target)
                .fetch_one(&mut *tx)
                .await
                .map_err(database)?;
                if Some(current) != row.get::<Option<i32>, _>("base_version") {
                    return Err(conflict("knowledge_version_conflict"));
                }
                current.checked_add(1).ok_or_else(invalid)?
            } else {
                1
            };
            let mut document: Document = parse(&row.get::<Value, _>("document"))?;
            let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tasks WHERE tenant_id=$1 AND id=$2 AND contract->>'model_access'='blocked')").bind(a.tenant_id).bind(row.get::<Uuid,_>("source_task_id")).fetch_one(&mut *tx).await.map_err(database)?;
            if blocked {
                document.external_model_allowed = false;
            }
            if let Some(category) = req.category {
                document.category = category;
            }
            if let Some(tags) = req.tags.as_ref() {
                document.tags = tags.clone();
            }
            document.retention_scope = if req.decision == Decision::Accept {
                "central"
            } else {
                "task"
            }
            .into();
            if let Some(reuse) = &req.reuse {
                document.reuse = Some(reuse.clone());
            }
            if document.retention_scope == "task" {
                document.reuse = None;
            }
            document.validate()?;
            check_relations(&mut tx, a.tenant_id, Some(id), &document).await?;
            if target.is_none() {
                sqlx::query("INSERT INTO knowledge_items(id,tenant_id,category,title,version,truth_state,outcome) VALUES($1,$2,$3,$4,$5,$6,$7)")
                    .bind(id).bind(a.tenant_id).bind(label(&document.category)?).bind(&document.title).bind(version)
                    .bind(label(&document.truth_state)?).bind(label(&document.outcome)?).execute(&mut *tx).await.map_err(database)?;
            }
            append_revision(
                &mut tx,
                a,
                id,
                version,
                &document,
                row.get("source_task_id"),
                Some(req.submission_id),
                &req.reason,
            )
            .await?;
            result_item = Some(id);
            result_version = Some(version);
            if req.decision == Decision::Accept {
                "accepted"
            } else {
                "local"
            }
        }
    };
    let updated=sqlx::query("UPDATE knowledge_submissions SET status=$3,decision=$4,result_item_id=$5,result_version=$6,decided_by=$7,decided_at=now() WHERE tenant_id=$1 AND id=$2 RETURNING *")
        .bind(a.tenant_id).bind(req.submission_id).bind(status).bind(decision).bind(result_item).bind(result_version).bind(a.jti)
        .fetch_one(&mut *tx).await.map_err(database)?;
    event(&mut tx,a,"knowledge.decided",json!({"submission_id":req.submission_id,"decision":status,"item_id":result_item,"version":result_version})).await?;
    tx.commit().await.map_err(database)?;
    Ok(json!({"submission":submission_json(&updated),"replayed":false}))
}

pub async fn classify(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    require(a, &[Role::Analyst])?;
    let req: ClassifyInput = parse(input)?;
    if req.expected_version < 1
        || !bounded(&req.reason, 2000)
        || (req.category.is_none()
            && req.tags.is_none()
            && req.retention_scope.is_none()
            && req.reuse.is_none())
    {
        return Err(invalid());
    }
    let mut tx = pool.begin().await.map_err(database)?;
    lock_tenant(&mut tx, a.tenant_id).await?;
    if let Some(key) = req.request_id
        && let Some(row) = sqlx::query("SELECT payload,result FROM knowledge_curation_effects WHERE tenant_id=$1 AND request_key=$2").bind(a.tenant_id).bind(key).fetch_optional(&mut *tx).await.map_err(database)? {
            if row.get::<Value,_>("payload") != *input { return Err(conflict("curation_request_conflict")); }
            return Ok(row.get("result"));
    }
    let row=sqlx::query("SELECT k.version,r.document,r.source_task_id,r.submission_id FROM knowledge_items k JOIN knowledge_revisions r ON r.tenant_id=k.tenant_id AND r.item_id=k.id AND r.version=k.version WHERE k.tenant_id=$1 AND k.id=$2 FOR UPDATE OF k")
        .bind(a.tenant_id).bind(req.item_id).fetch_optional(&mut *tx).await.map_err(database)?.ok_or_else(missing)?;
    let current: i32 = row.get("version");
    if current != req.expected_version {
        return Err(conflict("knowledge_version_conflict"));
    }
    let mut document: Document = parse(&row.get::<Value, _>("document"))?;
    if let Some(category) = req.category {
        document.category = category;
    }
    if let Some(tags) = req.tags {
        document.tags = tags;
    }
    if let Some(scope) = req.retention_scope {
        document.retention_scope = scope;
    }
    if let Some(reuse) = req.reuse {
        document.reuse = Some(reuse);
    }
    if !matches!(document.retention_scope.as_str(), "central" | "task") {
        return Err(invalid());
    }
    if document.retention_scope == "central" {
        document.validate_reuse()?;
    } else {
        document.reuse = None;
    }
    // Metadata-only management cannot alter content, source, verdict or outcome.
    document.normalize_tags()?;
    let version = current.checked_add(1).ok_or_else(invalid)?;
    append_revision(
        &mut tx,
        a,
        req.item_id,
        version,
        &document,
        row.get("source_task_id"),
        row.get("submission_id"),
        &req.reason,
    )
    .await?;
    event(
        &mut tx,
        a,
        "knowledge.classified",
        json!({"item_id":req.item_id,"version":version}),
    )
    .await?;
    let result = json!({"item_id":req.item_id,"version":version});
    if let Some(key) = req.request_id {
        sqlx::query("INSERT INTO knowledge_curation_effects(tenant_id,request_key,payload,result) VALUES($1,$2,$3,$4)").bind(a.tenant_id).bind(key).bind(input).bind(&result).execute(&mut *tx).await.map_err(database)?;
    }
    tx.commit().await.map_err(database)?;
    Ok(json!({"item_id":req.item_id,"version":version}))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn append_revision(
    tx: &mut Transaction<'_, Postgres>,
    a: &Claims,
    id: Uuid,
    version: i32,
    doc: &Document,
    source_task: Option<Uuid>,
    submission: Option<Uuid>,
    reason: &str,
) -> AppResult<()> {
    sqlx::query("INSERT INTO knowledge_revisions(tenant_id,item_id,version,document,source_task_id,submission_id,actor_task_id,actor_role,actor_session,reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
        .bind(a.tenant_id).bind(id).bind(version).bind(encoded(doc)?).bind(source_task).bind(submission).bind(a.task_id).bind(label(&a.role)?).bind(a.jti).bind(reason)
        .execute(&mut **tx).await.map_err(database)?;
    for tag in &doc.tags {
        sqlx::query(
            "INSERT INTO knowledge_tags(tenant_id,item_id,version,tag) VALUES($1,$2,$3,$4)",
        )
        .bind(a.tenant_id)
        .bind(id)
        .bind(version)
        .bind(tag)
        .execute(&mut **tx)
        .await
        .map_err(database)?;
    }
    for edge in &doc.relations {
        sqlx::query("INSERT INTO knowledge_edges(tenant_id,source_id,source_version,kind,target_id,target_version) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(a.tenant_id).bind(id).bind(version).bind(label(&edge.kind)?).bind(edge.target_id).bind(edge.target_version).execute(&mut **tx).await.map_err(database)?;
    }
    sqlx::query("UPDATE knowledge_items SET version=$3,category=$4,title=$5,truth_state=$6,outcome=$7,retention_scope=$8,updated_at=now() WHERE tenant_id=$1 AND id=$2")
        .bind(a.tenant_id).bind(id).bind(version).bind(label(&doc.category)?).bind(&doc.title).bind(label(&doc.truth_state)?).bind(label(&doc.outcome)?).bind(&doc.retention_scope)
        .execute(&mut **tx).await.map_err(database)?;
    let related: Vec<Uuid> = doc.relations.iter().map(|r| r.target_id).collect();
    let restricted:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_items WHERE tenant_id=$1 AND id=ANY($2) AND NOT external_model_allowed)").bind(a.tenant_id).bind(related).fetch_one(&mut **tx).await.map_err(database)?;
    if !doc.external_model_allowed || restricted {
        sqlx::query("WITH RECURSIVE restricted(id) AS (SELECT $2::uuid UNION SELECT e.source_id FROM knowledge_edges e JOIN restricted r ON r.id=e.target_id WHERE e.tenant_id=$1) UPDATE knowledge_items SET external_model_allowed=false WHERE tenant_id=$1 AND id IN (SELECT id FROM restricted)").bind(a.tenant_id).bind(id).execute(&mut **tx).await.map_err(database)?;
    }
    Ok(())
}

fn submission_json(row: &PgRow) -> Value {
    json!({"id":row.get::<Uuid,_>("id"),"source_task_id":row.get::<Uuid,_>("source_task_id"),
        "target_id":row.get::<Option<Uuid>,_>("target_id"),"base_version":row.get::<Option<i32>,_>("base_version"),
        "document":row.get::<Value,_>("document"),"rationale":row.get::<String,_>("rationale"),"status":row.get::<String,_>("status"),
        "decision":row.get::<Option<Value>,_>("decision"),"item_id":row.get::<Option<Uuid>,_>("result_item_id"),"version":row.get::<Option<i32>,_>("result_version")})
}
fn valid_limit(n: i64) -> AppResult<()> {
    if !(1..=100).contains(&n) {
        return Err(invalid());
    }
    Ok(())
}

pub async fn read(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    require(a, &[Role::Analyst, Role::Master])?;
    let mut input = input.clone();
    if input.is_null() {
        input = json!({});
    }
    if let Some(map) = input.as_object_mut() {
        map.entry("op").or_insert(json!("list"));
    }
    match parse::<ReadInput>(&input)? {
        ReadInput::List {
            retention_scope,
            source_task_id,
            category,
            tags,
            query,
            negative_history,
            after_id,
            limit,
        } => {
            valid_limit(limit)?;
            if tags.len() > 32
                || tags.iter().any(|t| !bounded(t, 128))
                || query.as_ref().is_some_and(|q| q.len() > 200)
            {
                return Err(invalid());
            }
            let scope = retention_scope.unwrap_or_else(|| "central".into());
            if !matches!(scope.as_str(), "central" | "task" | "review_needed")
                || (scope == "task" && source_task_id.is_none())
            {
                return Err(invalid());
            }
            let query = query.map(|q| {
                format!(
                    "%{}%",
                    q.replace('\\', "\\\\")
                        .replace('%', "\\%")
                        .replace('_', "\\_")
                )
            });
            let rows=sqlx::query("SELECT k.id,k.version,k.retention_scope,r.source_task_id,k.external_model_allowed,r.document,EXISTS(SELECT 1 FROM knowledge_revisions h WHERE h.tenant_id=k.tenant_id AND h.item_id=k.id AND (h.document->>'truth_state'='refuted' OR h.document->>'outcome'='failed')) AS has_negative_history FROM knowledge_items k JOIN knowledge_revisions r ON r.tenant_id=k.tenant_id AND r.item_id=k.id AND r.version=k.version WHERE k.tenant_id=$1 AND ($2::text IS NULL OR k.category=$2) AND NOT EXISTS(SELECT 1 FROM unnest($3::text[]) wanted(tag) WHERE NOT EXISTS(SELECT 1 FROM knowledge_tags t WHERE t.tenant_id=k.tenant_id AND t.item_id=k.id AND t.version=k.version AND t.tag=wanted.tag)) AND ($4::text IS NULL OR k.title ILIKE $4 OR r.document->>'content' ILIKE $4) AND (NOT $5 OR EXISTS(SELECT 1 FROM knowledge_revisions h WHERE h.tenant_id=k.tenant_id AND h.item_id=k.id AND (h.document->>'truth_state'='refuted' OR h.document->>'outcome'='failed'))) AND ($6::uuid IS NULL OR k.id>$6) AND k.retention_scope=$8 AND ($9::uuid IS NULL OR r.source_task_id=$9) ORDER BY k.id LIMIT $7")
                .bind(a.tenant_id).bind(category.map(|c|label(&c)).transpose()?).bind(tags).bind(query).bind(negative_history).bind(after_id).bind(limit).bind(scope).bind(source_task_id)
                .fetch_all(pool).await.map_err(database)?;
            let items:Vec<Value>=rows.iter().map(|r| { let d:Value=r.get("document");json!({"id":r.get::<Uuid,_>("id"),"version":r.get::<i32,_>("version"),
                "retention_scope":r.get::<String,_>("retention_scope"),"source_task_id":r.get::<Option<Uuid>,_>("source_task_id"),"category":d["category"],"title":d["title"],"tags":d["tags"],"truth_state":d["truth_state"],"outcome":d["outcome"],"freshness":effective_freshness(&d),"external_model_allowed":r.get::<bool,_>("external_model_allowed"),"has_negative_history":r.get::<bool,_>("has_negative_history")}) }).collect();
            Ok(json!({"next_after_id":items.last().map(|v|v["id"].clone()),"items":items}))
        }
        ReadInput::Get { item_id, version } => {
            if version.is_some_and(|v| v < 1) {
                return Err(invalid());
            }
            let row=sqlx::query("SELECT r.*,k.retention_scope AS effective_scope,k.external_model_allowed AS effective_model_access,r.created_at::text AS recorded_at FROM knowledge_items k JOIN knowledge_revisions r ON r.tenant_id=k.tenant_id AND r.item_id=k.id AND r.version=COALESCE($3,k.version) WHERE k.tenant_id=$1 AND k.id=$2")
                .bind(a.tenant_id).bind(item_id).bind(version).fetch_optional(pool).await.map_err(database)?.ok_or_else(missing)?;
            let version: i32 = row.get("version");
            let mut warnings = dependency_warnings(pool, a.tenant_id, item_id, version).await?;
            let warnings_truncated = warnings.len() > 100;
            warnings.truncate(100);
            let negative=sqlx::query("SELECT version,document->>'truth_state' AS truth_state,document->>'outcome' AS outcome FROM knowledge_revisions WHERE tenant_id=$1 AND item_id=$2 AND (document->>'truth_state'='refuted' OR document->>'outcome'='failed') ORDER BY version DESC LIMIT 101")
                .bind(a.tenant_id).bind(item_id).fetch_all(pool).await.map_err(database)?;
            Ok(
                json!({"item":revision_json(&row),"retention_scope":row.get::<String,_>("effective_scope"),"external_model_allowed":row.get::<bool,_>("effective_model_access"),"effective_freshness":effective_freshness(&row.get::<Value,_>("document")),"dependency_warnings":warnings,"dependency_warnings_truncated":warnings_truncated,
                    "negative_history_truncated":negative.len()>100,"negative_history":negative.iter().take(100).map(|r|json!({"version":r.get::<i32,_>("version"),"truth_state":r.get::<String,_>("truth_state"),"outcome":r.get::<String,_>("outcome")})).collect::<Vec<_>>()}),
            )
        }
        ReadInput::History {
            item_id,
            before_version,
            limit,
        } => {
            valid_limit(limit)?;
            if before_version.is_some_and(|v| v < 1) {
                return Err(invalid());
            }
            let rows=sqlx::query("SELECT *,created_at::text AS recorded_at FROM knowledge_revisions WHERE tenant_id=$1 AND item_id=$2 AND ($3::int IS NULL OR version<$3) ORDER BY version DESC LIMIT $4")
                .bind(a.tenant_id).bind(item_id).bind(before_version).bind(limit).fetch_all(pool).await.map_err(database)?;
            if rows.is_empty() && before_version.is_none() {
                return Err(missing());
            }
            Ok(
                json!({"revisions":rows.iter().map(revision_json).collect::<Vec<_>>(),"next_before_version":rows.last().map(|r|r.get::<i32,_>("version"))}),
            )
        }
        ReadInput::Submissions {
            status,
            source_task_id,
            after_id,
            limit,
        } => {
            valid_limit(limit)?;
            if status.as_ref().is_some_and(|v| {
                !matches!(v.as_str(), "pending" | "accepted" | "local" | "rejected")
            }) {
                return Err(invalid());
            }
            let rows=sqlx::query("SELECT id,source_task_id,target_id,base_version,status,result_item_id,result_version,rationale,document->>'title' AS title,document->>'category' AS category,document->'tags' AS tags,(SELECT retention_scope FROM knowledge_items k WHERE k.tenant_id=knowledge_submissions.tenant_id AND k.id=result_item_id) AS retention_scope,created_at::text AS submitted_at,decided_at::text AS decided_at FROM knowledge_submissions WHERE tenant_id=$1 AND ($2::text IS NULL OR status=$2) AND ($3::uuid IS NULL OR source_task_id=$3) AND ($4::uuid IS NULL OR id>$4) ORDER BY id LIMIT $5")
                .bind(a.tenant_id).bind(status).bind(source_task_id).bind(after_id).bind(limit).fetch_all(pool).await.map_err(database)?;
            Ok(json!({"submissions":rows.iter().map(|r|json!({
                    "id":r.get::<Uuid,_>("id"),"source_task_id":r.get::<Uuid,_>("source_task_id"),
                    "target_id":r.get::<Option<Uuid>,_>("target_id"),"base_version":r.get::<Option<i32>,_>("base_version"),
                    "status":r.get::<String,_>("status"),"item_id":r.get::<Option<Uuid>,_>("result_item_id"),"version":r.get::<Option<i32>,_>("result_version"),
                    "title":r.get::<String,_>("title"),"category":r.get::<String,_>("category"),"tags":r.get::<Value,_>("tags"),
                    "retention_scope":r.get::<Option<String>,_>("retention_scope"),"rationale":r.get::<String,_>("rationale"),"submitted_at":r.get::<String,_>("submitted_at"),"decided_at":r.get::<Option<String>,_>("decided_at")
                })).collect::<Vec<_>>(),"next_after_id":rows.last().map(|r|r.get::<Uuid,_>("id"))}))
        }
        ReadInput::Submission { submission_id } => {
            let row =
                sqlx::query("SELECT * FROM knowledge_submissions WHERE tenant_id=$1 AND id=$2")
                    .bind(a.tenant_id)
                    .bind(submission_id)
                    .fetch_optional(pool)
                    .await
                    .map_err(database)?
                    .ok_or_else(missing)?;
            Ok(json!({"submission":submission_json(&row)}))
        }
    }
}
fn revision_json(row: &PgRow) -> Value {
    json!({"id":row.get::<Uuid,_>("item_id"),"version":row.get::<i32,_>("version"),"document":row.get::<Value,_>("document"),
        "source_task_id":row.get::<Option<Uuid>,_>("source_task_id"),"submission_id":row.get::<Option<Uuid>,_>("submission_id"),
        "actor_task_id":row.get::<Option<Uuid>,_>("actor_task_id"),"actor_role":row.get::<String,_>("actor_role"),
        "reason":row.get::<String,_>("reason"),"recorded_at":row.get::<String,_>("recorded_at")})
}
async fn dependency_warnings(
    pool: &PgPool,
    tenant: Uuid,
    item: Uuid,
    version: i32,
) -> AppResult<Vec<Value>> {
    let rows=sqlx::query("WITH RECURSIVE dependencies(item_id,version) AS (SELECT target_id,target_version FROM knowledge_edges WHERE tenant_id=$1 AND source_id=$2 AND source_version=$3 AND kind IN ('references','derived_from','depends_on') UNION SELECT e.target_id,e.target_version FROM knowledge_edges e JOIN dependencies d ON d.item_id=e.source_id AND d.version=e.source_version WHERE e.tenant_id=$1 AND e.kind IN ('references','derived_from','depends_on')) SELECT d.item_id,d.version AS pinned_version,k.version AS current_version,k.truth_state,k.outcome,CASE WHEN r.document->>'freshness'='superseded' THEN 'superseded' WHEN (r.document->>'valid_until')::bigint<=extract(epoch from now()) THEN 'expired' ELSE r.document->>'freshness' END AS freshness FROM dependencies d JOIN knowledge_items k ON k.tenant_id=$1 AND k.id=d.item_id JOIN knowledge_revisions r ON r.tenant_id=k.tenant_id AND r.item_id=k.id AND r.version=k.version WHERE k.version<>d.version OR k.truth_state='refuted' OR k.outcome='failed' OR r.document->>'freshness'<>'current' OR (r.document->>'valid_until')::bigint<=extract(epoch from now()) ORDER BY d.item_id,d.version LIMIT 101")
        .bind(tenant).bind(item).bind(version).fetch_all(pool).await.map_err(database)?;
    Ok(rows.iter().map(|r|json!({"item_id":r.get::<Uuid,_>("item_id"),"pinned_version":r.get::<i32,_>("pinned_version"),"current_version":r.get::<i32,_>("current_version"),"truth_state":r.get::<String,_>("truth_state"),"outcome":r.get::<String,_>("outcome"),"freshness":r.get::<String,_>("freshness")})).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reference() -> Document {
        parse(&json!({"category":"data","title":"Official reference","storage_mode":"reference","sources":[{"uri":"https://example.com/docs"}]})).unwrap()
    }
    #[test]
    fn reference_does_not_require_ingesting_the_document() {
        let mut d = reference();
        assert!(d.validate().is_ok());
        assert!(d.content.is_empty());
        d.content = "copied body".into();
        assert_eq!(d.validate().unwrap_err().code, "knowledge_storage_invalid");
    }
    #[test]
    fn central_placement_requires_a_grounded_future_use_but_local_records_do_not() {
        let mut d = reference();
        d.retention_scope = "central".into();
        assert_eq!(d.validate().unwrap_err().code, "central_reuse_required");
        d.reuse = Some(Reuse {
            applicable_when: "Configuring the documented service in another Task".into(),
            decision_or_action:
                "Consult this exact documentation section before choosing parameters".into(),
            evidence: "https://example.com/docs".into(),
        });
        d.validate().unwrap();
        d.reuse.as_mut().unwrap().evidence = "An invented general lesson".into();
        assert_eq!(d.validate().unwrap_err().code, "central_reuse_not_grounded");
        d.retention_scope = "task".into();
        d.reuse = None;
        d.validate().unwrap();
    }
    #[test]
    fn negative_records_require_context_and_keep_truth_separate_from_outcome() {
        let mut d = reference();
        d.outcome = Outcome::Failed;
        assert_eq!(d.validate().unwrap_err().code, "negative_evidence_required");
        d.assessment = Some(Assessment {
            conditions: "synthetic account".into(),
            attempted_action: "read".into(),
            observed: "denied".into(),
            reason: "missing grant".into(),
            avoid: "same grant".into(),
            retry_when: "grant restored".into(),
        });
        d.tags = vec![
            "실패".into(),
            "거짓".into(),
            " system:test ".into(),
            "system:test".into(),
        ];
        d.validate().unwrap();
        assert!(d.tags.contains(&"실패".into()));
        assert!(!d.tags.contains(&"거짓".into()));
        assert_eq!(d.truth_state, TruthState::Unverified);
        assert_eq!(d.tags.len(), 2);
        d.truth_state = TruthState::Refuted;
        d.validate().unwrap();
        assert!(d.tags.contains(&"거짓".into()));
    }
    #[test]
    fn invalid_categories_sources_and_unscoped_edges_are_rejected() {
        assert!(
            parse::<Document>(&json!({"category":"fact","title":"x","storage_mode":"reference"}))
                .is_err()
        );
        let mut d = reference();
        d.sources[0].uri = "https://user:password@example.com".into();
        assert!(d.validate().is_err());
        d.sources[0].uri = "https://example.com/?access_token=synthetic".into();
        assert!(d.validate().is_err());
        d.sources[0].uri = "file:///unscoped".into();
        assert!(d.validate().is_err());
        d = reference();
        d.relations.push(Relation {
            kind: RelationKind::References,
            target_id: Uuid::new_v4(),
            target_version: 0,
        });
        assert!(d.validate().is_err());
    }
    #[test]
    fn derived_negative_tags_are_idempotent_at_the_user_tag_limit() {
        let mut d = reference();
        d.tags = (0..32).map(|i| format!("topic:{i}")).collect();
        d.truth_state = TruthState::Refuted;
        d.outcome = Outcome::Failed;
        d.normalize_tags().unwrap();
        let tags = d.tags.clone();
        assert_eq!(tags.len(), 34);
        d.normalize_tags().unwrap();
        assert_eq!(tags, d.tags);
        d.tags.push("topic:overflow".into());
        assert!(d.normalize_tags().is_err());
    }
}
