//! Prompt-only operator entry. Handler choices stay inside the installed adapters.
use crate::{
    error::{AppError, AppResult},
    execution::{self, StartRun},
    server::AppState,
    workspace::{self, TaskContract, bad, db, missing},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub tenant_id: Uuid,
    pub request_id: Uuid,
    pub goal: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub model: String,
    pub max_calls: u32,
    pub max_total_tokens: u64,
    #[serde(default)]
    pub retain_knowledge: bool,
    #[serde(default)]
    pub dream_after: bool,
    #[serde(default = "mode")]
    pub mode: String,
    #[serde(default)]
    pub protocol_id: Option<Uuid>,
    #[serde(default)]
    pub protocol_version: Option<i32>,
    #[serde(default)]
    pub parameters: Value,
}
fn mode() -> String {
    "work".into()
}
impl Settings {
    pub fn apply(&self, base: &StartRun) -> StartRun {
        let mut c = base.clone();
        c.model = self.model.clone();
        c.max_calls = self.max_calls;
        c.max_total_tokens = self.max_total_tokens;
        c.retain_knowledge = self.retain_knowledge;
        c.dream_after = self.dream_after;
        c.mode = self.mode.clone();
        c.protocol_id = self.protocol_id;
        c.protocol_version = self.protocol_version;
        c.parameters = self.parameters.clone();
        c
    }
    pub fn validate(&self, catalog: &Value) -> AppResult<()> {
        if !(16..=48).contains(&self.max_calls)
            || !(10000..=300000).contains(&self.max_total_tokens)
            || !matches!(self.mode.as_str(), "work" | "dream")
            || !catalog["models"]
                .as_array()
                .is_some_and(|models| models.iter().any(|m| m["slug"] == self.model))
            || self.protocol_version.is_some_and(|v| v < 1)
            || self.mode == "dream" && self.protocol_id.is_some()
        {
            return Err(bad());
        }
        Ok(())
    }
}
pub async fn defaults(s: &AppState, tenant: Uuid) -> AppResult<(String, String)> {
    let pref: Option<Value> =
        sqlx::query_scalar("SELECT preferences FROM tenant_settings WHERE tenant_id=$1")
            .bind(tenant)
            .fetch_optional(&s.pool)
            .await
            .map_err(db)?;
    let pref = pref.unwrap_or(json!({}));
    let choices = crate::provider::list(s, tenant).await?;
    let choices = choices["connections"].as_array().ok_or_else(bad)?;
    let usable = |v: &&Value| v["enabled"] != false && v["status"] != "revoked";
    let connection = choices
        .iter()
        .filter(usable)
        .find(|v| v["id"] == pref["connection_id"] && v["id"].is_string())
        .or_else(|| {
            choices
                .iter()
                .filter(usable)
                .find(|v| v["kind"] == "chatgpt_oauth")
        })
        .ok_or_else(|| AppError::bad("model_connection_required"))?;
    let id = connection["id"].as_str().ok_or_else(bad)?.to_string();
    let catalog = crate::provider::models(s, tenant, &id).await?;
    let models = catalog["models"].as_array().ok_or_else(bad)?;
    let model = models
        .iter()
        .find(|m| m["slug"] == pref["model"] && m["slug"].is_string())
        .or_else(|| models.first())
        .and_then(|m| m["slug"].as_str())
        .ok_or_else(|| AppError::bad("model_connection_required"))?
        .to_string();
    Ok((id, model))
}
pub async fn launch(s: &AppState, r: Launch) -> AppResult<(Value, bool)> {
    if !workspace::nonempty(&r.goal, 4000) {
        return Err(bad());
    }
    let mut tx = s.pool.begin().await.map_err(db)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("goal-launch:{}:{}", r.tenant_id, r.request_id))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let old = sqlx::query("SELECT id,goal FROM tasks WHERE tenant_id=$1 AND launch_key=$2")
        .bind(r.tenant_id)
        .bind(r.request_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;
    let task = if let Some(old) = old {
        if old.get::<String, _>("goal") != r.goal {
            return Err(AppError::bad("goal_launch_conflict"));
        }
        old.get("id")
    } else {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tenants WHERE id=$1)")
            .bind(r.tenant_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
        if !exists {
            return Err(missing());
        }
        let task = Uuid::new_v4();
        sqlx::query("INSERT INTO tasks(id,tenant_id,goal,contract,launch_key) VALUES($1,$2,$3,'{\"kind\":\"work\"}',$4)").bind(task).bind(r.tenant_id).bind(&r.goal).bind(r.request_id).execute(&mut *tx).await.map_err(db)?;
        task
    };
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM execution_runs WHERE tenant_id=$1 AND task_id=$2 AND request_key=$3",
    )
    .bind(r.tenant_id)
    .bind(task)
    .bind(r.request_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?;
    tx.commit().await.map_err(db)?;
    if let Some(run) = existing {
        return Ok((json!({"task_id":task,"run_id":run,"replayed":true}), false));
    }
    let (connection_id, model) = defaults(s, r.tenant_id).await?;
    let c = StartRun {
        request_id: r.request_id,
        tenant_id: r.tenant_id,
        task_id: task,
        connection_id,
        model,
        target: format!("task:{task}"),
        max_calls: 48,
        max_total_tokens: 300000,
        mode: mode(),
        protocol_id: None,
        protocol_version: None,
        parameters: Value::Null,
        retain_knowledge: false,
        dream_after: false,
        prepared_pack_id: None,
        eager_context: false,
        auto_configure: true,
    };
    let (run, spawn) = execution::start(s, c).await?;
    Ok((
        json!({"task_id":task,"run_id":run["id"],"replayed":run["replayed"]}),
        spawn,
    ))
}
pub fn validate_contract(
    c: &TaskContract,
    goal: &str,
    registered_urls: &[String],
) -> AppResult<()> {
    c.validate()?;
    if !matches!(c.kind.as_str(), "work" | "dream")
        || !matches!(c.model_access.as_str(), "" | "external_allowed")
        || c.output_format.trim().is_empty()
        || c.completion_conditions.is_empty()
        || c.verification.trim().is_empty()
        || c.max_work_orders == 0
    {
        return Err(bad());
    }
    for u in &c.urls {
        if (u.allow_private || u.allow_http)
            && !goal.contains(&u.url)
            && !registered_urls.contains(&u.url)
        {
            return Err(AppError::forbidden("automatic_url_scope_denied"));
        }
    }
    Ok(())
}
pub async fn catalog(s: &AppState, c: &StartRun, models: &Value) -> AppResult<Value> {
    let mut protocols = vec![];
    let mut source_ids = vec![];
    let list = crate::protocol::list(&s.pool, c.tenant_id, "").await?;
    for item in list["protocols"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["status"] == "active" && p["external_model_allowed"] == true)
        .take(20)
    {
        let id = serde_json::from_value(item["id"].clone()).map_err(|_| bad())?;
        let value = crate::protocol::load(&s.pool, c.tenant_id, id, None).await?;
        if value["document"]["external_model_allowed"] == false {
            continue;
        }
        protocols.push(json!({"id":item["id"],"version":item["version"],"codename":item["codename"],"description":item["description"].as_str().unwrap_or("").chars().take(500).collect::<String>(),"parameters":value["document"]["parameters"]}));
        for r in value["document"]["references"]
            .as_array()
            .into_iter()
            .flatten()
        {
            source_ids.push(r["item_id"].clone());
        }
    }
    let secrets = crate::broker::secret_list(&s.pool, c.tenant_id).await?;
    let secrets: Vec<Value> = secrets["secrets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["enabled"] == true)
        .take(16)
        .cloned()
        .collect();
    let artifacts=sqlx::query("SELECT id,name,media_type,sha256,work_order_id,octet_length(body) AS bytes FROM artifacts WHERE tenant_id=$1 AND task_id=$2 AND work_order_id IS NULL ORDER BY created_at LIMIT 16").bind(c.tenant_id).bind(c.task_id).fetch_all(&s.pool).await.map_err(db)?;
    let value = json!({"models":models["models"],"protocols":protocols,"secrets":secrets,"input_artifacts":artifacts.iter().map(workspace::artifact_header).collect::<Vec<_>>(),"source_ids":source_ids});
    if serde_json::to_vec(&value).map_err(|_| bad())?.len() > 65536 {
        return Err(AppError::bad("setup_catalog_budget_exceeded"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handler_cannot_select_uninstalled_tools_or_unregistered_private_urls() {
        let mut c = TaskContract {
            kind: "work".into(),
            output_format: "report.md".into(),
            completion_conditions: vec!["Verify source".into()],
            verification: "Inspect actual artifact".into(),
            max_work_orders: 2,
            ..Default::default()
        };
        c.allowed_tools = vec!["vault.raw_read".into()];
        assert!(validate_contract(&c, "goal", &[]).is_err());
        c.allowed_tools = vec!["external.fetch".into()];
        c.urls = vec![workspace::UrlGrant {
            url: "http://fixture:8081/snapshot".into(),
            allow_private: true,
            allow_http: true,
        }];
        assert!(validate_contract(&c, "goal", &[]).is_err());
        assert!(validate_contract(&c, "goal", &[c.urls[0].url.clone()]).is_ok());
        c.model_access = "blocked".into();
        assert!(validate_contract(&c, "goal", &[]).is_err());
    }
}
