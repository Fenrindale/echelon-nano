use crate::{
    error::{AppError, AppResult},
    knowledge::{self, Document, Freshness, Relation, RelationKind, TruthState},
    policy::{Claims, Role},
    protocol::{self, Protocol},
    workspace::{bad, db, digest, missing, nonempty},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Base {
    pub item_id: Uuid,
    pub version: i32,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Replacement {
    pub target_id: Option<Uuid>,
    pub document: Document,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub bases: Vec<Base>,
    pub replacements: Vec<Replacement>,
}
pub async fn list(pool: &PgPool, tenant: Uuid) -> AppResult<Value> {
    let rows=sqlx::query("SELECT p.id,p.task_id,p.kind,p.reason,p.payload,p.status,p.decision,p.created_at::text AS created_at,COALESCE(t.contract->>'model_access','')<>'blocked' AND COALESCE((p.payload->'document'->>'external_model_allowed')::bool,true) AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(COALESCE(p.payload->'replacements','[]')) r WHERE r->'document'->>'external_model_allowed'='false') AND NOT EXISTS(SELECT 1 FROM knowledge_items k WHERE k.tenant_id=p.tenant_id AND NOT k.external_model_allowed AND (p.payload->'bases' @> jsonb_build_array(jsonb_build_object('item_id',k.id)) OR p.payload->'document'->'references' @> jsonb_build_array(jsonb_build_object('item_id',k.id)) OR p.payload->>'item_id'=k.id::text)) AS external_model_allowed FROM maintenance_proposals p JOIN tasks t ON t.tenant_id=p.tenant_id AND t.id=p.task_id WHERE p.tenant_id=$1 ORDER BY p.created_at DESC LIMIT 100").bind(tenant).fetch_all(pool).await.map_err(db)?;
    Ok(
        json!({"proposals":rows.iter().map(|r|json!({"id":r.get::<Uuid,_>("id"),"task_id":r.get::<Uuid,_>("task_id"),"kind":r.get::<String,_>("kind"),"reason":r.get::<String,_>("reason"),"payload":r.get::<Value,_>("payload"),"external_model_allowed":r.get::<bool,_>("external_model_allowed"),"status":r.get::<String,_>("status"),"decision":r.get::<Option<Value>,_>("decision"),"created_at":r.get::<String,_>("created_at")})).collect::<Vec<_>>()}),
    )
}
pub async fn propose(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    if a.role != Role::Master {
        return Err(AppError::forbidden("role_denied"));
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Proposal {
        request_id: Uuid,
        kind: String,
        reason: String,
        payload: Value,
    }
    let p: Proposal = serde_json::from_value(input.clone()).map_err(|_| bad())?;
    if !nonempty(&p.reason, 2000)
        || !matches!(
            p.kind.as_str(),
            "revise" | "merge" | "split" | "protocol" | "verification"
        )
        || serde_json::to_vec(&p.payload).map_err(|_| bad())?.len() > 48000
    {
        return Err(bad());
    }
    if matches!(p.kind.as_str(), "revise" | "merge" | "split") {
        let mut c: Change = serde_json::from_value(p.payload.clone()).map_err(|_| bad())?;
        if c.bases.is_empty()
            || c.bases.len() > 16
            || c.replacements.is_empty()
            || c.replacements.len() > 8
        {
            return Err(bad());
        }
        if (p.kind == "revise"
            && (c.bases.len() != 1
                || c.replacements.len() != 1
                || c.replacements[0].target_id != Some(c.bases[0].item_id)))
            || (p.kind == "merge"
                && (c.bases.len() < 2
                    || c.replacements.len() != 1
                    || c.replacements[0].target_id.is_some()))
            || (p.kind == "split"
                && (c.bases.len() != 1
                    || c.replacements.len() < 2
                    || c.replacements.iter().any(|r| r.target_id.is_some())))
        {
            return Err(bad());
        }
        for r in &mut c.replacements {
            r.document.validate()?;
        }
        for b in &c.bases {
            let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_revisions WHERE tenant_id=$1 AND item_id=$2 AND version=$3)").bind(a.tenant_id).bind(b.item_id).bind(b.version).fetch_one(pool).await.map_err(db)?;
            if !exists {
                return Err(missing());
            }
        }
    }
    if p.kind == "protocol" {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Payload {
            protocol_id: Option<Uuid>,
            expected_version: Option<i32>,
            document: Protocol,
        }
        let payload: Payload = serde_json::from_value(p.payload.clone()).map_err(|_| bad())?;
        payload.document.validate()?;
        if payload.protocol_id.is_some() != payload.expected_version.is_some() {
            return Err(bad());
        }
        for member in payload.document.references {
            let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_revisions WHERE tenant_id=$1 AND item_id=$2 AND version=$3)").bind(a.tenant_id).bind(member.item_id).bind(member.version).fetch_one(pool).await.map_err(db)?;
            if !exists {
                return Err(missing());
            }
        }
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO maintenance_proposals(id,tenant_id,task_id,request_key,kind,reason,payload) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(tenant_id,task_id,request_key) DO NOTHING").bind(id).bind(a.tenant_id).bind(a.task_id).bind(p.request_id).bind(&p.kind).bind(&p.reason).bind(&p.payload).execute(pool).await.map_err(db)?;
    let row=sqlx::query("SELECT id,kind,reason,payload,status FROM maintenance_proposals WHERE tenant_id=$1 AND task_id=$2 AND request_key=$3").bind(a.tenant_id).bind(a.task_id).bind(p.request_id).fetch_one(pool).await.map_err(db)?;
    if row.get::<Value, _>("payload") != p.payload
        || row.get::<String, _>("kind") != p.kind
        || row.get::<String, _>("reason") != p.reason
    {
        return Err(AppError::new(
            axum::http::StatusCode::CONFLICT,
            "maintenance_idempotency_conflict",
        ));
    }
    Ok(json!({"proposal_id":row.get::<Uuid,_>("id"),"status":row.get::<String,_>("status")}))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub tenant_id: Uuid,
    pub proposal_id: Uuid,
    pub accept: bool,
    pub reason: String,
    #[serde(default)]
    pub activate_protocol: bool,
}
pub async fn decide(pool: &PgPool, r: Decision) -> AppResult<Value> {
    if !nonempty(&r.reason, 2000) {
        return Err(bad());
    }
    let mut tx = pool.begin().await.map_err(db)?;
    let row =
        sqlx::query("SELECT * FROM maintenance_proposals WHERE tenant_id=$1 AND id=$2 FOR UPDATE")
            .bind(r.tenant_id)
            .bind(r.proposal_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?
            .ok_or_else(missing)?;
    if row.get::<String, _>("status") != "pending" {
        if (row.get::<String, _>("status") == "applied") != r.accept {
            return Err(AppError::new(
                axum::http::StatusCode::CONFLICT,
                "maintenance_decision_conflict",
            ));
        }
        return Ok(
            json!({"status":row.get::<String,_>("status"),"decision":row.get::<Value,_>("decision"),"replayed":true}),
        );
    }
    let task = row.get("task_id");
    let a = crate::workspace::actor(r.tenant_id, task, Role::Master);
    let kind: String = row.get("kind");
    let payload: Value = row.get("payload");
    let mut changes = vec![];
    if r.accept {
        knowledge::lock_tenant(&mut tx, r.tenant_id).await?;
        match kind.as_str() {
            "protocol" => {
                let mut doc: Protocol =
                    serde_json::from_value(payload["document"].clone()).map_err(|_| bad())?;
                if r.activate_protocol {
                    doc.status = "active".into();
                }
                let id = payload
                    .get("protocol_id")
                    .filter(|v| !v.is_null())
                    .map(|v| serde_json::from_value(v.clone()))
                    .transpose()
                    .map_err(|_| bad())?;
                let version = payload
                    .get("expected_version")
                    .filter(|v| !v.is_null())
                    .map(|v| serde_json::from_value(v.clone()))
                    .transpose()
                    .map_err(|_| bad())?;
                changes.push(protocol::write(&mut tx, &a, id, version, doc, &r.reason).await?);
            }
            "verification" => {
                let item: Option<Uuid> = payload
                    .get("item_id")
                    .filter(|v| !v.is_null())
                    .map(|v| serde_json::from_value(v.clone()))
                    .transpose()
                    .map_err(|_| bad())?;
                let id = Uuid::new_v4();
                sqlx::query("INSERT INTO review_requests(id,tenant_id,source_proposal,item_id,reason) VALUES($1,$2,$3,$4,$5)").bind(id).bind(r.tenant_id).bind(r.proposal_id).bind(item).bind(&r.reason).execute(&mut *tx).await.map_err(db)?;
                changes.push(json!({"review_request_id":id}));
            }
            _ => {
                let mut c: Change = serde_json::from_value(payload).map_err(|_| bad())?;
                let mut originals = std::collections::BTreeMap::new();
                for base in &c.bases {
                    let row=sqlx::query("SELECT k.version,r.document,r.source_task_id,r.submission_id FROM knowledge_items k JOIN knowledge_revisions r ON r.tenant_id=k.tenant_id AND r.item_id=k.id AND r.version=k.version WHERE k.tenant_id=$1 AND k.id=$2 FOR UPDATE OF k").bind(r.tenant_id).bind(base.item_id).fetch_optional(&mut *tx).await.map_err(db)?.ok_or_else(missing)?;
                    if row.get::<i32, _>("version") != base.version {
                        return Err(AppError::new(
                            axum::http::StatusCode::CONFLICT,
                            "knowledge_version_conflict",
                        ));
                    }
                    originals.insert(base.item_id, row);
                }
                let mut targets = std::collections::BTreeSet::new();
                for replacement in &mut c.replacements {
                    let id = replacement.target_id.unwrap_or_else(Uuid::new_v4);
                    if !targets.insert(id) {
                        return Err(bad());
                    }
                    let version = if let Some(old) = replacement.target_id {
                        let row = originals.get(&old).ok_or_else(bad)?;
                        row.get::<i32, _>("version") + 1
                    } else {
                        1
                    };
                    for base in &c.bases {
                        if originals
                            .get(&base.item_id)
                            .ok_or_else(bad)?
                            .get::<Value, _>("document")["external_model_allowed"]
                            == false
                        {
                            replacement.document.external_model_allowed = false;
                        }
                        if base.item_id != id
                            && !replacement.document.relations.iter().any(|e| {
                                e.target_id == base.item_id && e.target_version == base.version
                            })
                        {
                            replacement.document.relations.push(Relation {
                                kind: RelationKind::DerivedFrom,
                                target_id: base.item_id,
                                target_version: base.version,
                            });
                        }
                    }
                    if replacement.target_id.is_none() {
                        replacement.document.truth_state = TruthState::Unverified;
                    }
                    replacement.document.validate()?;
                    knowledge::check_relations(
                        &mut tx,
                        r.tenant_id,
                        Some(id),
                        &replacement.document,
                    )
                    .await?;
                    if replacement.target_id.is_none() {
                        sqlx::query("INSERT INTO knowledge_items(id,tenant_id,category,title,version,truth_state,outcome) VALUES($1,$2,$3,$4,1,$5,$6)").bind(id).bind(r.tenant_id).bind(json!(replacement.document.category).as_str()).bind(&replacement.document.title).bind(json!(replacement.document.truth_state).as_str()).bind(json!(replacement.document.outcome).as_str()).execute(&mut *tx).await.map_err(db)?;
                    }
                    knowledge::append_revision(
                        &mut tx,
                        &a,
                        id,
                        version,
                        &replacement.document,
                        Some(task),
                        None,
                        &r.reason,
                    )
                    .await?;
                    changes.push(json!({"item_id":id,"version":version,"previous_version":if version>1{Some(version-1)}else{None}}));
                }
                if matches!(kind.as_str(), "merge" | "split") {
                    for base in &c.bases {
                        if targets.contains(&base.item_id) {
                            continue;
                        }
                        let row = originals.get(&base.item_id).ok_or_else(bad)?;
                        let mut doc: Document =
                            serde_json::from_value(row.get("document")).map_err(|_| bad())?;
                        doc.freshness = Freshness::Superseded;
                        knowledge::append_revision(
                            &mut tx,
                            &a,
                            base.item_id,
                            base.version + 1,
                            &doc,
                            row.get("source_task_id"),
                            row.get("submission_id"),
                            "Superseded by reviewed maintenance; original preserved",
                        )
                        .await?;
                        changes.push(json!({"item_id":base.item_id,"version":base.version+1,"previous_version":base.version}));
                    }
                }
            }
        }
    }
    let status = if r.accept { "applied" } else { "rejected" };
    let decision = json!({"reason":r.reason,"changes":changes});
    sqlx::query(
        "UPDATE maintenance_proposals SET status=$3,decision=$4 WHERE tenant_id=$1 AND id=$2",
    )
    .bind(r.tenant_id)
    .bind(r.proposal_id)
    .bind(status)
    .bind(&decision)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    sqlx::query("INSERT INTO audit_records(tenant_id,task_id,actor_role,action,outcome,details) VALUES($1,$2,'master','maintenance.decided',$3,$4)").bind(r.tenant_id).bind(task).bind(status).bind(json!({"proposal_id":r.proposal_id,"kind":kind,"decision":decision})).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(json!({"status":status,"decision":decision}))
}
pub async fn optimize(pool: &PgPool, a: &Claims, input: &Value) -> AppResult<Value> {
    if a.role != Role::Master {
        return Err(AppError::forbidden("role_denied"));
    }
    match input["op"].as_str() {
        Some("propose") => {
            let mut p = input.clone();
            p.as_object_mut().ok_or_else(bad)?.remove("op");
            propose(pool, a, &p).await
        }
        Some("proposals") => list(pool, a.tenant_id).await,
        Some("inspect") => inspect(pool, a.tenant_id).await,
        Some("deduplicate") => deduplicate(pool, a).await,
        _ => Err(bad()),
    }
}
pub async fn inspect(pool: &PgPool, tenant: Uuid) -> AppResult<Value> {
    let repetitions=sqlx::query("WITH finished AS (SELECT DISTINCT ON (r.task_id) r.task_id,t.goal,t.contract,r.state FROM execution_runs r JOIN tasks t ON t.id=r.task_id AND t.tenant_id=r.tenant_id WHERE r.tenant_id=$1 AND COALESCE(t.contract->>'model_access','')<>'blocked' AND r.status='completed' AND r.state->'result'->>'outcome'='completed' ORDER BY r.task_id,r.created_at DESC), grouped AS (SELECT regexp_replace(lower(goal),'[0-9]+','*','g') AS purpose,contract->>'output_format' AS format,contract->'allowed_tools' AS tools,contract->>'verification' AS verification,count(*) AS completed_tasks,jsonb_agg(task_id) AS task_ids FROM finished GROUP BY purpose,format,tools,verification HAVING count(*)>=3) SELECT * FROM grouped ORDER BY completed_tasks DESC LIMIT 10").bind(tenant).fetch_all(pool).await.map_err(db)?;
    let denied=sqlx::query("SELECT actor_role,action,count(*) AS count FROM audit_records WHERE tenant_id=$1 AND outcome='denied' GROUP BY actor_role,action ORDER BY count(*) DESC LIMIT 10").bind(tenant).fetch_all(pool).await.map_err(db)?;
    let inventory=sqlx::query("SELECT k.id,k.category,k.title,k.version,k.truth_state,k.outcome,k.external_model_allowed FROM knowledge_items k JOIN knowledge_revisions r ON r.tenant_id=k.tenant_id AND r.item_id=k.id AND r.version=k.version WHERE k.tenant_id=$1 ORDER BY k.updated_at DESC LIMIT 30").bind(tenant).fetch_all(pool).await.map_err(db)?;
    Ok(
        json!({"repetitions":repetitions.iter().map(|r|json!({"purpose":r.get::<String,_>("purpose"),"output_format":r.get::<Option<String>,_>("format"),"completed_tasks":r.get::<i64,_>("completed_tasks"),"task_ids":r.get::<Value,_>("task_ids")})).collect::<Vec<_>>(),"denials":denied.iter().map(|r|json!({"role":r.get::<String,_>("actor_role"),"action":r.get::<String,_>("action"),"count":r.get::<i64,_>("count")})).collect::<Vec<_>>(),"knowledge":inventory.iter().map(|r|json!({"item_id":r.get::<Uuid,_>("id"),"category":r.get::<String,_>("category"),"external_model_allowed":r.get::<bool,_>("external_model_allowed"),"title":r.get::<String,_>("title"),"version":r.get::<i32,_>("version"),"truth_state":r.get::<String,_>("truth_state"),"outcome":r.get::<String,_>("outcome")})).collect::<Vec<_>>()}),
    )
}
async fn deduplicate(pool: &PgPool, a: &Claims) -> AppResult<Value> {
    let mut tx = pool.begin().await.map_err(db)?;
    knowledge::lock_tenant(&mut tx, a.tenant_id).await?;
    let rows=sqlx::query("SELECT k.id,r.document FROM knowledge_items k JOIN knowledge_revisions r ON r.tenant_id=k.tenant_id AND r.item_id=k.id AND r.version=k.version WHERE k.tenant_id=$1 AND NOT EXISTS(SELECT 1 FROM knowledge_aliases x WHERE x.tenant_id=k.tenant_id AND x.source_id=k.id) ORDER BY k.id LIMIT 500").bind(a.tenant_id).fetch_all(&mut *tx).await.map_err(db)?;
    let mut hashes = std::collections::BTreeMap::new();
    let mut linked = vec![];
    for row in rows {
        let id: Uuid = row.get("id");
        let mut doc: Value = row.get("document");
        doc.as_object_mut().ok_or_else(bad)?.remove("tags");
        let hash = digest(&serde_json::to_vec(&doc).map_err(|_| bad())?);
        if let Some(target) = hashes.get(&hash) {
            sqlx::query("INSERT INTO knowledge_aliases(tenant_id,source_id,target_id,reason) VALUES($1,$2,$3,'Exact document duplicate; both originals retained') ON CONFLICT DO NOTHING").bind(a.tenant_id).bind(id).bind(*target).execute(&mut *tx).await.map_err(db)?;
            linked.push(json!({"source_id":id,"target_id":target}));
        } else {
            hashes.insert(hash, id);
        }
    }
    sqlx::query("INSERT INTO audit_records(tenant_id,task_id,actor_role,action,outcome,details) VALUES($1,$2,'master','knowledge.deduplicated','completed',$3)").bind(a.tenant_id).bind(a.task_id).bind(json!({"linked":linked})).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(json!({"linked":linked}))
}
