# Work Order handoff and supervision

Handler selects prior Task artifacts with agent.delegate input_artifacts and protected Agent results with input_outputs. Exact owned artifact/output UUIDs mentioned in the objective or expected output are also resolved automatically. Incident IDs and unknown UUIDs do not become grants. Explicit nonexistent, foreign-Task or Handler-private output selections are rejected.

The server pins artifact/output IDs, hashes and source Work Orders in the immutable contract.handoff_artifacts and contract.input_outputs manifests. This adds input access only; it does not expand business tools, credentials, URLs or central knowledge access. Agent can read granted artifacts and protected results through artifact.read and run.output, including their scoped output index. Unselected sibling results remain inaccessible. Replaying the delegation request returns the original Work Order and manifest.

The current Work Order must be collected with work_order.read before replanning. A missing collection returns execution_result_required. Only an exhausted positive Work Order limit returns work_order_budget_exceeded. Automatic Tasks retain their unlimited lifetime setting.

## Event-driven supervision

Ordinary partial reads no longer interrupt Agent every eight tool results or two minutes. Handler is invoked at a tool boundary for:

- An important finding or decision explicitly reported with task.observe needs_handler:true.
- Two consecutive failed tool results, including MCP operation_ok:false.
- Three consecutive identical read requests with unchanged results.
- An unknown tool outcome needing reconciliation.
- A ten-minute heartbeat since the preceding supervision or delegation.

Events in an independent read batch coalesce before the next model call. Completion still goes directly to Handler collection and original-goal verification. Pause, terminal state, manual call budgets and server-owned Work Order grants retain their existing checks. The heartbeat does not interrupt/replay an in-flight external operation.

Supervision uses a separate per-Work-Order Handler conversation with the original goal, working-state goals/steps, recent operational notes, previous result references and trigger metadata. The complete notes and planning conversation remain in the server checkpoint; this projection does not rewrite them. A server restart still needs explicit resume.

This design references Artex revision 869a4aefc2e6aeee9575e841d3331d6ac93367e7:

- [Engine change notifications, debounce and ten-minute minimum heartbeat](https://github.com/jiwoochris/artex-global/blob/869a4aefc2e6aeee9575e841d3331d6ac93367e7/server/engine.go#L816-L947).
- [Planner receives concrete trigger events](https://github.com/jiwoochris/artex-global/blob/869a4aefc2e6aeee9575e841d3331d6ac93367e7/agent/planner.go#L351-L354).
- [Worker findings notify the planner](https://github.com/jiwoochris/artex-global/blob/869a4aefc2e6aeee9575e841d3331d6ac93367e7/agent/worker.go#L342-L372).

Artex coalesces changes with an 800ms debounce in a concurrent planner. Nano coalesces a completed native read batch in its serial driver. No Go source is copied.

## NUL-bearing evidence

Original artifacts retain exact bytes and hashes. A selected JSON/text value containing NUL cannot be stored directly in PostgreSQL JSONB. Such a view returns encoding:json_escaped and a json string: decoding that string once recovers the exact selected data, including NUL and literal escape sequences. Original storage is never normalized, truncated or replaced.

## Verification

Docker checks and the isolated PostgreSQL suite cover read grants, sibling/Task isolation, immutable manifests, idempotency, NUL view persistence, precise error classification, ordinary read progress, event triggers, heartbeat and checkpoint serialization.

Run scripts/Verify-Handoff.ps1 only in the isolated port-18432 copy with the explicitly connected OAuth account. It runs two real Agent stages, twelve distinct partial reads, a NUL projection, explicit event supervision and actual previous-Agent output access, then checks exact final bytes and the original source. Broader role, LLM, workflow, attachment, continuity and restart verifiers remain separate gates. Live ESET results are operational evidence, not synthetic test results.
