//! Event-driven Handler intervention; ordinary reads are not findings.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const HEARTBEAT_SECONDS: u64 = 600;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Monitor {
    failures: u32,
    repeated: u32,
    last_read: String,
    reason: Option<String>,
    read_visits: BTreeMap<String, u32>,
}

impl Monitor {
    pub fn observe(&mut self, tool: &str, input: &Value, output: &Value) {
        let failed = output["ok"] == false
            || output["result"]["operation_ok"] == false
            || matches!(
                output["result"]["status"].as_str(),
                Some("unknown" | "failed")
            )
            || (tool == "shell.exec"
                && (output["result"]["timed_out"] == true
                    || output["result"]["exit_code"]
                        .as_i64()
                        .is_some_and(|c| c != 0)));
        self.failures = if failed { self.failures + 1 } else { 0 };
        if self.failures >= 2 {
            self.reason = Some("repeated_tool_failure".into());
        }
        if tool == "task.observe" && input["needs_handler"] == true {
            self.reason = Some("agent_requested_direction".into());
        }
        if output["result"]["status"] == "unknown" {
            self.reason = Some("unknown_tool_outcome".into());
        }
        if tool == "shell.exec" && output["ok"] == false {
            // A local execution adapter failure should not lead to hundreds of
            // manual JSON reads before Handler learns that computation broke.
            self.reason = Some("local_computation_failed".into());
        }
        if is_recovery_read(tool) {
            let mut semantic = output.clone();
            if let Some(result) = semantic["result"].as_object_mut() {
                for key in ["request_id", "replayed", "full_output"] {
                    result.remove(key);
                }
            }
            let fingerprint = crate::workspace::digest(
                json!({"tool":tool,"input":input,"output":semantic})
                    .to_string()
                    .as_bytes(),
            );
            self.repeated = if fingerprint == self.last_read {
                self.repeated + 1
            } else {
                1
            };
            if self.read_visits.len() < 512 || self.read_visits.contains_key(&fingerprint) {
                let count = self.read_visits.entry(fingerprint.clone()).or_default();
                *count += 1;
                if *count >= 3 {
                    self.reason = Some("revisited_unchanged_read".into());
                }
            }
            self.last_read = fingerprint;
            if self.repeated >= 3 {
                self.reason = Some("repeated_unchanged_read".into());
            }
        } else {
            self.repeated = 0;
            self.last_read.clear();
        }
    }
    pub fn reason(&self, now: u64, supervised_at: u64) -> Option<&str> {
        self.reason.as_deref().or_else(|| {
            (supervised_at > 0 && now.saturating_sub(supervised_at) >= HEARTBEAT_SECONDS)
                .then_some("heartbeat")
        })
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn triggered(&mut self, reason: String) {
        self.reason = Some(reason);
    }
}

pub fn is_recovery_read(tool: &str) -> bool {
    matches!(tool, "artifact.read" | "run.output" | "work_order.read")
}

/// Reads and narrative notes can be useful but do not by themselves demonstrate
/// progress on a business completion condition.
pub fn substantive(tool: &str, output: &Value) -> bool {
    output["ok"] == true
        && !is_recovery_read(tool)
        && !matches!(tool, "task.observe" | "run.work")
        && output["result"]["operation_ok"] != false
        && !matches!(
            output["result"]["status"].as_str(),
            Some("failed" | "unknown" | "running" | "cancelling")
        )
        && output["result"]["timed_out"] != true
        && output["result"]["exit_code"]
            .as_i64()
            .is_none_or(|code| code == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_runs_without_count_interruptions_but_failures_and_stalls_wake_handler() {
        let mut m = Monitor::default();
        for offset in 0..100 {
            m.observe(
                "artifact.read",
                &json!({"offset":offset}),
                &json!({"ok":true,"result":{"value":offset}}),
            );
            assert!(m.reason(500, 1).is_none());
        }
        assert_eq!(m.reason(601, 1), Some("heartbeat"));
        let mut m: Monitor = serde_json::from_value(json!(m)).unwrap();
        let failure = json!({"ok":true,"result":{"operation_ok":false}});
        m.observe("mcp.call", &json!({}), &failure);
        assert!(m.reason(2, 1).is_none());
        m.observe("mcp.call", &json!({}), &failure);
        assert_eq!(m.reason(2, 1), Some("repeated_tool_failure"));
        m.reset();
        for _ in 0..3 {
            m.observe("run.output", &json!({"id":"same"}), &json!({"ok":true}));
        }
        assert_eq!(m.reason(2, 1), Some("repeated_unchanged_read"));
        m.reset();
        m.observe(
            "task.observe",
            &json!({"needs_handler":true}),
            &json!({"ok":true}),
        );
        assert_eq!(m.reason(2, 1), Some("agent_requested_direction"));
    }
    #[test]
    fn interleaved_rereads_and_failed_local_execution_wake_handler() {
        let mut m = Monitor::default();
        for visit in 0..3 {
            for id in ["one", "two"] {
                m.observe(
                    "artifact.read",
                    &json!({"id":id,"offset":0}),
                    &json!({"ok":true,"result":{"value":id,"request_id":visit,"replayed":visit>0}}),
                );
                m.observe(
                    "task.observe",
                    &json!({"observation":"cursor saved"}),
                    &json!({"ok":true}),
                );
            }
        }
        assert_eq!(m.reason(2, 1), Some("revisited_unchanged_read"));
        let mut restored: Monitor = serde_json::from_value(json!(m)).unwrap();
        restored.reset();
        restored.observe(
            "shell.exec",
            &json!({}),
            &json!({"ok":false,"error":{"code":"worker_storage_exhausted"}}),
        );
        assert_eq!(restored.reason(2, 1), Some("local_computation_failed"));
        assert!(!substantive("artifact.read", &json!({"ok":true})));
        assert!(!substantive("task.observe", &json!({"ok":true})));
        assert!(substantive(
            "shell.exec",
            &json!({"ok":true,"result":{"exit_code":0}})
        ));
    }
}
