//! Event-driven Handler intervention; ordinary reads are not findings.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const HEARTBEAT_SECONDS: u64 = 600;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Monitor {
    failures: u32,
    repeated: u32,
    last_read: String,
    reason: Option<String>,
}

impl Monitor {
    pub fn observe(&mut self, tool: &str, input: &Value, output: &Value) {
        let failed = output["ok"] == false
            || output["result"]["operation_ok"] == false
            || output["result"]["status"] == "unknown";
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
        if matches!(tool, "artifact.read" | "run.output" | "work_order.read") {
            let fingerprint = crate::workspace::digest(
                json!({"tool":tool,"input":input,"output":output})
                    .to_string()
                    .as_bytes(),
            );
            self.repeated = if fingerprint == self.last_read {
                self.repeated + 1
            } else {
                1
            };
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
}
