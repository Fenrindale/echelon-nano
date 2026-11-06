# Context continuity

Model input has no application byte cutoff or Task-wide token budget. Usage is observed. Call/time rounds retain explicit manual call limits and automatic continuation. Context pressure is separate from no-progress supervision and cannot itself declare a Task unsuccessful.

The catalog supplies `context_window` when available; otherwise the runtime labels a conservative 128,000-token soft estimate. A Unicode-aware estimate and observed input usage guide compression at 75% of that window. This triggers a summary, not a storage or lifetime-usage limit. Provider context rejection follows the same recovery path without executing the rejected model response.

## Original output and partial reads

Full protected results remain immutable in `execution_actions.outcome`. Large model views add `full_output:{id,sha256,call_id}` and `abbreviated:true`. `execution_output_refs` records their birth role and Work Order. Existing artifact/MCP/observation/native-effect records remain business evidence. Views never overwrite them or prove successful effects.

`run.output {id,pointer,format:'json'|'text',offset,limit}` reads an original within the current run, tenant, Task and role scope. Agent can read only its Work Order outputs; Handler may also inspect Agent outputs in this Task. Analyst/Master cannot obtain another role's data. Source-policy checks still apply before model transmission.

JSON Pointer enters JSON encoded inside strings, including MCP `content[].text`. A narrow `/result/content/0/text/incidents` query avoids copying a large response through shell or chat. Actual keys must come from the source. Pages return `total_items` or `total_characters`, `next_offset`, and the original hash/reference. JSON offsets refer to array/object entries, not bytes.

`run.output {op:'list',tool?:'mcp.call',offset:0,limit:20}` recovers older reference IDs after a long session. This index enforces the same role/Work Order scope and returns metadata only. Omit `op` for a read by ID.

`artifact.read {artifact_id,format:'json',pointer,offset,limit}` offers the same selection over authorized UTF-8 JSON. Default text reads are paginated without base64; operator downloads retain exact bytes. Text pages use up to 16,000 characters and JSON pages up to 100 entries. Oversized legacy requests normalize to a page rather than repeatedly failing. Work Order receipt pages normalize to 1–16 entries. View sizes do not end a Task. Storage/transport protections and credential masking remain in effect.

## Summary and continuation

Complete sessions are archived with their hash before compression. A restricted model call summarizes only observable decisions/results and has no business tools. Calls, usage, failures and unknown usage are recorded. Failed generation leaves a labelled structured-receipt fallback. Summaries are immutable, cached by run/session/source hash and checkpointed separately for each role/Agent Work Order.

Automatic runs use model summaries. Explicitly call-limited manual runs use the structured-receipt continuation without spending an extra summary call outside their requested limit.

Continuation combines that note, complete recent call/output pairs and server-owned state. Original goal, completion conditions, active steps, progress notes, pinned evidence, grants, MCP preparations and effects remain intact. A summary cannot verify goals, publish knowledge, expand permissions or authorize replay. References survive compaction and restart. Restart still requires explicit reconciliation/resume; unknown external effects are never automatically retried. Knowledge adoption remains optional.

## Verification and references

Docker checks and isolated PostgreSQL tests cover preservation and role scope. `scripts/Verify-Continuity.ps1` runs only in the isolated port-18432 project and covers large encoded JSON, exact hashes, partial queries, actual model continuation and checkpointed goals/progress. Existing role, LLM, launch, work-scenario and restart-recovery verifiers cover broader regression; live account evidence remains separate.

References: Artex's [model-window compaction](https://github.com/jiwoochris/artex-global/blob/869a4aefc2e6aeee9575e841d3331d6ac93367e7/agent/provider.go#L102-L135) and [Worker goal/output handling](https://github.com/jiwoochris/artex-global/blob/869a4aefc2e6aeee9575e841d3331d6ac93367e7/agent/worker.go#L384-L469). No Artex source is copied.
