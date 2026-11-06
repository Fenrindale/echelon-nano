//! Task-local operational memory. These records never publish central knowledge.
use crate::{
    error::{AppError, AppResult},
    workspace::nonempty,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub kind: String,
    pub id: Uuid,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Step {
    pub id: String,
    pub objective: String,
    pub depends_on: Vec<String>,
    pub priority: i32,
    pub status: String,
    pub work_order_id: Option<Uuid>,
    pub result: Value,
}
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Goal {
    pub id: String,
    pub condition: String,
    pub evidence: Vec<Evidence>,
    pub reason: String,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Memory {
    pub revision: u64,
    pub goals: Vec<Goal>,
    pub steps: BTreeMap<String, Step>,
    pub notes: Vec<Value>,
    pub active_step: Option<String>,
    pub packs: Vec<Value>,
}
impl Memory {
    pub fn has_attempts(&self) -> bool {
        self.steps.values().any(|s| s.work_order_id.is_some())
    }
    pub fn begin_run(&mut self) {
        for goal in &mut self.goals {
            goal.evidence.clear();
            goal.reason.clear();
        }
        self.active_step = None;
        for step in self.steps.values_mut().filter(|s| s.status == "running") {
            step.status = "blocked".into();
        }
    }
    pub fn configure(&mut self, conditions: &[String]) -> AppResult<()> {
        if self.has_attempts()
            && self.goals.iter().map(|g| &g.condition).collect::<Vec<_>>()
                != conditions.iter().collect::<Vec<_>>()
        {
            return Err(AppError::bad("execution_goal_contract_frozen"));
        }
        if self.goals.is_empty() || !self.has_attempts() {
            self.goals = conditions
                .iter()
                .map(|condition| Goal {
                    id: crate::workspace::digest(condition.as_bytes())[..16].into(),
                    condition: condition.clone(),
                    ..Goal::default()
                })
                .collect();
            self.revision += 1;
        }
        Ok(())
    }
    pub fn update(&mut self, input: &Value, call_id: &str) -> AppResult<Value> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Update {
            #[serde(default)]
            steps: Vec<Step>,
            #[serde(default)]
            note: Option<String>,
        }
        let u: Update = serde_json::from_value(input.clone())
            .map_err(|_| AppError::bad("invalid_work_state"))?;
        if u.steps.len() > 32 || u.note.as_ref().is_some_and(|n| !nonempty(n, 4000)) {
            return Err(AppError::bad("invalid_work_state"));
        }
        let mut next = self.steps.clone();
        for mut step in u.steps {
            if !nonempty(&step.id, 64)
                || !step
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
                || !nonempty(&step.objective, 4000)
                || step.depends_on.len() > 32
                || step.work_order_id.is_some()
                || !step.result.is_null()
                || !matches!(
                    step.status.as_str(),
                    "" | "pending" | "blocked" | "superseded"
                )
            {
                return Err(AppError::bad("invalid_work_state"));
            }
            if next
                .get(&step.id)
                .is_some_and(|old| old.work_order_id.is_some())
            {
                return Err(AppError::bad("work_attempt_immutable"));
            }
            if step.status.is_empty() {
                step.status = "pending".into();
            }
            next.insert(step.id.clone(), step);
        }
        if next.values().filter(|s| s.status == "pending").count() > 256 || !valid_graph(&next) {
            return Err(AppError::bad("work_dependency_cycle_or_missing"));
        }
        self.steps = next;
        if let Some(note) = u.note
            && !self.notes.iter().any(|n| n["call_id"] == call_id)
        {
            self.notes.push(json!({"call_id":call_id,"text":note}));
            if self.notes.len() > 128 {
                self.notes.drain(..self.notes.len() - 128);
            }
        }
        self.revision += 1;
        Ok(self.view())
    }
    pub fn ready(&self) -> Vec<&Step> {
        let mut steps: Vec<_> = self
            .steps
            .values()
            .filter(|s| {
                s.status == "pending"
                    && s.depends_on
                        .iter()
                        .all(|d| self.steps.get(d).is_some_and(|p| p.status == "done"))
            })
            .collect();
        steps.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.id.cmp(&b.id)));
        steps
    }
    pub fn check_step(&self, id: &str) -> AppResult<()> {
        if !self.ready().iter().any(|s| s.id == id) {
            return Err(AppError::bad("work_step_not_ready"));
        }
        Ok(())
    }
    pub fn attach(&mut self, input: &Value, order: Uuid, call_id: &str) -> AppResult<()> {
        let id = input["step_id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "attempt-{}",
                    &crate::workspace::digest(call_id.as_bytes())[..16]
                )
            });
        if self
            .steps
            .get(&id)
            .is_some_and(|s| s.work_order_id == Some(order))
        {
            return Ok(());
        }
        if input["step_id"].is_string() {
            self.check_step(&id)?;
        }
        let s = self.steps.entry(id.clone()).or_insert_with(|| Step {
            id: id.clone(),
            objective: input["objective"].as_str().unwrap_or("").into(),
            ..Step::default()
        });
        s.status = "running".into();
        s.work_order_id = Some(order);
        self.active_step = Some(id);
        self.revision += 1;
        Ok(())
    }
    pub fn settle(&mut self, order: Uuid, result: &Value) {
        for step in self
            .steps
            .values_mut()
            .filter(|s| s.work_order_id == Some(order))
        {
            step.status = if result["outcome"] == "completed" {
                "done"
            } else {
                "blocked"
            }
            .into();
            step.result = json!({"outcome":result["outcome"],"summary":result["summary"],"artifacts":result["artifacts"]});
        }
        self.active_step = None;
        self.revision += 1;
    }
    pub fn knows_order(&self, order: &Value) -> bool {
        self.steps
            .values()
            .any(|s| json!(s.work_order_id) == *order && s.work_order_id.is_some())
    }
    pub fn prove(&mut self, goal_id: &str, evidence: Vec<Evidence>, reason: &str) -> AppResult<()> {
        if evidence.is_empty() || evidence.len() > 16 || !nonempty(reason, 2000) {
            return Err(AppError::bad("goal_evidence_required"));
        }
        let goal = self
            .goals
            .iter_mut()
            .find(|g| g.id == goal_id)
            .ok_or_else(|| AppError::bad("goal_not_found"))?;
        goal.evidence = evidence;
        goal.reason = reason.into();
        self.revision += 1;
        Ok(())
    }
    pub fn complete(&self) -> bool {
        !self.goals.is_empty() && self.goals.iter().all(|g| !g.evidence.is_empty())
    }
    pub fn view(&self) -> Value {
        let mut hot = BTreeSet::new();
        let mut frontier: Vec<_> = self
            .steps
            .values()
            .filter(|s| matches!(s.status.as_str(), "pending" | "running"))
            .map(|s| s.id.clone())
            .collect();
        while let Some(id) = frontier.pop() {
            if hot.insert(id.clone())
                && let Some(step) = self.steps.get(&id)
            {
                frontier.extend(step.depends_on.iter().cloned());
            }
        }
        let compact = |s: &Step| json!({"id":s.id,"objective":s.objective.chars().take(300).collect::<String>(),"depends_on":s.depends_on,"status":s.status,"work_order_id":s.work_order_id,"result":{"outcome":s.result["outcome"],"summary":s.result["summary"].as_str().unwrap_or("").chars().take(400).collect::<String>(),"artifacts":s.result["artifacts"]}});
        let active: Vec<_> = self
            .steps
            .values()
            .filter(|s| hot.contains(&s.id))
            .take(48)
            .map(compact)
            .collect();
        let settled: Vec<_> = self
            .steps
            .values()
            .filter(|s| !matches!(s.status.as_str(), "pending" | "running"))
            .rev()
            .take(12)
            .map(compact)
            .collect();
        json!({"revision":self.revision,"goals":self.goals,"active":active,"hot_step_ids":hot,"ready":self.ready().into_iter().take(16).map(compact).collect::<Vec<_>>(),"recent_attempts":settled,"older_attempts":self.steps.len().saturating_sub(12),"notes":self.notes.iter().rev().take(8).collect::<Vec<_>>(),"packs":self.packs,"placement":"task_local","note":"Operational records, not central knowledge or authorization. Active dependency IDs stay hot. Re-read scoped Work Orders for full receipts."})
    }
}
fn valid_graph(steps: &BTreeMap<String, Step>) -> bool {
    let mut done = BTreeSet::new();
    loop {
        let before = done.len();
        for s in steps.values() {
            if s.depends_on.iter().all(|p| done.contains(p)) {
                done.insert(s.id.clone());
            }
        }
        if done.len() == steps.len() {
            return true;
        }
        if before == done.len() {
            return false;
        }
    }
}

pub async fn validate_evidence(
    pool: &sqlx::PgPool,
    tenant: Uuid,
    task: Uuid,
    e: &Evidence,
    successful: bool,
) -> AppResult<()> {
    use sqlx::Row;
    let valid=match e.kind.as_str() {
        "artifact" if successful=>sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM artifacts WHERE tenant_id=$1 AND task_id=$2 AND id=$3 AND octet_length(body)>0)").bind(tenant).bind(task).bind(e.id).fetch_one(pool).await,
        "observation"=>sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM observations WHERE tenant_id=$1 AND task_id=$2 AND id=$3)").bind(tenant).bind(task).bind(e.id).fetch_one(pool).await,
        "work_order"=>sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM work_order_results WHERE tenant_id=$1 AND task_id=$2 AND work_order_id=$3 AND (result->>'outcome'='completed')=$4)").bind(tenant).bind(task).bind(e.id).bind(successful).fetch_one(pool).await,
        "receipt"=> {
            let rows=sqlx::query("SELECT status,result FROM tool_receipts WHERE tenant_id=$1 AND task_id=$2 AND request_key=$3 UNION ALL SELECT status,result FROM mcp_call_receipts WHERE tenant_id=$1 AND task_id=$2 AND request_key=$3 UNION ALL SELECT 'completed' AS status,result FROM mcp_preparation_results WHERE tenant_id=$1 AND task_id=$2 AND request_key=$3").bind(tenant).bind(task).bind(e.id).fetch_all(pool).await.map_err(|_|AppError::internal("execution_database_failed"))?;
            Ok(rows.iter().any(|r| {
                let status:String=r.get("status");let v:Option<Value>=r.get("result");
                let v=v.unwrap_or(Value::Null);
                let good=status=="completed" && (v["operation_ok"]==true || v["exit_code"]==0 && v["timed_out"]==false || v["status"].as_u64().is_some_and(|s|(200..300).contains(&s)) || v["source"]=="server.mcp_preparation" && v["status"]=="ready");
                let bad=status=="unknown" || status=="completed" && (v["operation_ok"]==false || v["exit_code"].as_i64().is_some_and(|s|s!=0) || v["timed_out"]==true || v["status"].as_u64().is_some_and(|s|s>=400) || matches!(v["status"].as_str(),Some("cancelled"|"failed")));
                if successful {good}else{bad}
            }))
        }
        _=>Ok(false),
    }.map_err(|_|AppError::internal("execution_database_failed"))?;
    if !valid {
        return Err(AppError::forbidden("goal_evidence_not_owned_or_successful"));
    }
    Ok(())
}

/// Compact API input; the full birth history is archived separately before replacement.
pub fn recent_receipts(history: &[Value]) -> Vec<Value> {
    history.iter().filter(|v| v["type"] == "function_call_output").rev().take(12).map(|v| {
        let raw = v["output"].as_str().unwrap_or("");
        let value: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
        json!({"call_id":v["call_id"],"ok":value["ok"],"error":value["error"],"full_output":value["full_output"],"result":crate::continuity::view(&value["result"],600)["value"],"note":"Original effects and full protected outputs are preserved. Follow full_output using run.output when present."})
    }).collect()
}
pub fn tool_summary(tool: &str, call_id: &str, value: &Value) -> Value {
    json!({"tool":tool,"call_id":call_id,"ok":value["ok"],"error":value["error"],"job_id":value["result"]["job_id"],"status":value["result"]["status"],"artifacts":value["result"]["artifacts"],"artifact":value["result"]["artifact"],"full_output":value["full_output"],"note":"Use a narrow original output query; do not reload an unchanged entire Work Order."})
}

pub fn schema(tool: &str) -> Value {
    let string = || json!({"type":"string"});
    let strings = || json!({"type":"array","items":{"type":"string"}});
    let object = || json!({"type":"object","additionalProperties":true});
    let mut properties = match tool {
        "mcp.search" => {
            json!({"query":string(),"transport":{"type":"string","enum":["npm","python"]},"limit":{"type":"integer","minimum":1,"maximum":12}})
        }
        "mcp.describe" => {
            json!({"package":string(),"transport":{"type":"string","enum":["npm","python"]},"version":string(),"offset":{"type":"integer","minimum":0,"maximum":200000}})
        }
        "mcp.prepare" => {
            json!({"name":string(),"reason":string(),"spec":{"type":"object","properties":{"transport":{"type":"string","enum":["npm","python","http"]},"package":string(),"package_version":string(),"executable":string(),"args":strings(),"url":string(),"allow_private":{"type":"boolean"},"allow_http":{"type":"boolean"},"environment":{"type":"array","items":{"type":"object","properties":{"name":string(),"bundle_id":string(),"variable":string()},"required":["name","bundle_id","variable"],"additionalProperties":false}}},"additionalProperties":false}})
        }
        "mcp.inspect" => json!({"server_id":string()}),
        "mcp.call" => json!({"server_id":string(),"tool":string(),"arguments":object()}),
        "context.request" | "run.context_refresh" => json!({"criteria":object()}),
        "context.read" => {
            json!({"op":{"type":"string","enum":["pack","resolve","resolve_many"]},"ref_id":string(),"ref_ids":strings()})
        }
        "artifact.read" => {
            json!({"artifact_id":string(),"format":{"type":"string","enum":["text","json"]},"pointer":{"type":"string","description":"JSON Pointer; may enter JSON encoded inside MCP text. Use a narrow field instead of reading the whole result."},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":16000,"description":"Text characters (max 16000); JSON array/object items (max 100). Larger legacy requests are paged, never discard originals."}})
        }
        "artifact.write" => {
            json!({"name":string(),"media_type":string(),"content":string(),"data_base64":string()})
        }
        "shell.exec" => {
            json!({"op":{"type":"string","enum":["exec","start","poll","cancel"]},"script":string(),"job_id":string(),"outputs":strings(),"timeout_seconds":{"type":"integer","minimum":1,"maximum":3600},"wait_seconds":{"type":"integer","minimum":0,"maximum":20}})
        }
        "task.plan" => json!({"steps":strings(),"risks":strings()}),
        "agent.delegate" => {
            json!({"objective":string(),"expected_output":string(),"step_id":string(),"references":{"type":"array","items":object()}})
        }
        "work_order.read" => {
            json!({"work_order_id":string(),"receipt_offset":{"type":"integer","minimum":0,"maximum":100000},"receipt_limit":{"type":"integer","minimum":1,"maximum":16}})
        }
        "run.output" => {
            json!({"id":string(),"pointer":string(),"format":{"type":"string","enum":["json","text"]},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":16000}})
        }
        "work_order.complete" => {
            json!({"outcome":{"type":"string","enum":["completed","partial","blocked","failed"]},"summary":string(),"evidence":{"type":"array","items":object()},"artifacts":strings(),"limitations":strings()})
        }
        "run.work" => json!({"steps":{"type":"array","items":object()},"note":string()}),
        "run.supervise" => {
            json!({"decision":{"type":"string","enum":["continue","steer","recall"]},"message":string()})
        }
        "run.verify" => {
            json!({"proofs":{"type":"array","items":{"type":"object","properties":{"goal_id":string(),"reason":string(),"evidence":{"type":"array","items":{"type":"object","properties":{"kind":{"type":"string","enum":["artifact","receipt","observation","work_order"]},"id":string()},"required":["kind","id"],"additionalProperties":false}}},"required":["goal_id","reason","evidence"],"additionalProperties":false}}})
        }
        "run.finish" => {
            json!({"outcome":{"type":"string","enum":["completed","partial","blocked","failed"]},"summary":string(),"assessment":object()})
        }
        _ => json!({}),
    };
    if tool == "run.output" {
        properties["op"] = json!({"type":"string","enum":["list"],"description":"Omit for a read by id; list recovers older scoped reference IDs."});
        properties["tool"] = json!({"type":"string","description":"Optional exact tool name for list, such as mcp.call."});
    }
    // Individual adapters enforce their complete Deserialize contracts and role
    // scopes. Discovery supplies pinned MCP schemas without copying them here.
    json!({"type":"object","properties":properties,"additionalProperties":true})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn goals_freeze_when_work_is_issued_not_when_a_plan_is_drafted() {
        let mut m = Memory::default();
        m.update(
            &json!({"steps":[{"id":"prepare","objective":"Prepare owned output"}]}),
            "draft",
        )
        .unwrap();
        m.configure(&["Original verified output".into()]).unwrap();
        m.configure(&["Finalized verified output".into()]).unwrap();
        assert_eq!(m.goals[0].condition, "Finalized verified output");
        m.attach(&json!({"step_id":"prepare"}), Uuid::new_v4(), "issued")
            .unwrap();
        assert!(m.configure(&["Weakened output".into()]).is_err());
    }
    #[test]
    fn frontier_and_failed_attempts_are_separate() {
        let mut m = Memory::default();
        m.update(&json!({"steps":[{"id":"collect","objective":"Collect owned records"},{"id":"report","objective":"Verify report","depends_on":["collect"]}]}),"plan").unwrap();
        assert_eq!(m.ready().len(), 1);
        let order = Uuid::new_v4();
        m.attach(&json!({"step_id":"collect"}), order, "delegate")
            .unwrap();
        m.settle(
            order,
            &json!({"outcome":"partial","summary":"One page failed"}),
        );
        assert!(m.ready().is_empty());
        assert!(
            m.update(
                &json!({"steps":[{"id":"collect","objective":"Pretend retry"}]}),
                "rewrite"
            )
            .is_err()
        );
        assert!(
            m.update(
                &json!({"steps":[{"id":"cycle","objective":"Invalid","depends_on":["cycle"]}]}),
                "cycle"
            )
            .is_err()
        );
    }
    #[test]
    fn task_state_survives_serialization_without_claiming_goal_completion() {
        let mut m = Memory::default();
        m.configure(&["Verified output".into()]).unwrap();
        m.update(&json!({"note":"Read page two next"}), "note")
            .unwrap();
        let mut m: Memory = serde_json::from_value(json!(m)).unwrap();
        assert!(!m.complete());
        let id = m.goals[0].id.clone();
        m.prove(
            &id,
            vec![Evidence {
                kind: "artifact".into(),
                id: Uuid::new_v4(),
            }],
            "Verified actual content",
        )
        .unwrap();
        assert!(m.complete());
        assert_eq!(m.view()["placement"], "task_local");
    }
}
