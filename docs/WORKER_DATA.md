# Worker data and original-goal progress

## Source and output handling

The authenticated internal Worker RPC has no fixed request/file byte quota. Its former 128-KiB input/output-file checks, 512-KiB request/output-total checks and child file-size limit are removed. Full stdout/stderr are retained rather than cut at 64 KiB. Generated Agent UTF-8 artifacts are stored without the former 1-MiB artifact quota. User upload limits remain the documented separate attachment policy.

The dedicated workspace tmpfs, container/child memory, CPU, timeout and process limits still apply. Storage exhaustion is reported as `worker_storage_exhausted`; removing a transfer quota does not imply unlimited machine resources. The Worker retains the child-only syscall policy, no network access, unprivileged per-Work-Order identities, empty inherited environment and no broker/DB/Vault credentials.

Before execution, Broker loads only explicitly granted Task/Work-Order artifacts, verifies their SHA-256 hashes and materializes them at `inputs/ARTIFACT_UUID`. `echelon-inputs.json` records the artifact ID, original name, exact path, byte count and hash. Unique, nonconflicting legacy names also remain available. Multiple `mcp-output.json` files have independent UUID paths and no ambiguous filename alias. Unselected siblings, foreign Tasks and credential attachments are not granted by this manifest. Non-UTF-8 input remains an explicit unsupported input type.

Agent can parse nested MCP JSON locally with Python, deduplicate/join/sort rows, preserve contradictory versions, and declare a compact summary plus a complete ledger as outputs. Original sources and complete outputs remain immutable stored evidence. Model views use existing protected-output references, partial queries and context summaries; large data should not be transcribed through prompts for mechanical arithmetic.

Worker validation exposes fixed, specific error codes for invalid script, timeout, input/output path, duplicate input path, output count and output type. These are local execution errors, not business authentication or missing connectors. An unknown previously dispatched job still requires reconciliation; it is never silently reexecuted.

## Goal and inference supervision

Handler receives original unverified completion conditions and must periodically connect its next method to a specific original goal. Recovery reads/cursor notes do not constitute verified business progress. The progress review cannot grant tools, create completion evidence or rewrite earlier Work Orders. Legitimate evidence analysis need not perform unnecessary external writes. The ten-minute heartbeat and event-based supervision remain; alternating unchanged reads are now detected too.

Provider stream failures use safe cause codes for context, transient server failure, rate limiting, quota exhaustion, authentication and rejected requests. Unknown generic generation failures get at most three retries. Only uncommitted model generation is retried, with unknown usage and zero executed effects recorded. Quota/authentication/permanent request failures are not transient retries; business tool outcomes are never replayed by this policy. Every main-loop model failure records a safe `model.failed` event, including a terminal nonretryable failure. Raw provider prose is not exposed.

## Reproducible checks

- `docker build --target checks .`: formatting, Clippy, unit and protocol tests.
- `docker compose -f compose.verify.yaml up --build --abort-on-container-exit --exit-code-from tests`, then `down --remove-orphans`: isolated database checks.
- `scripts/Verify-WorkerData.ps1`, only in the isolated port-18432 copy: two large same-name source pages handed between Work Orders, 2,000 returned rows/1,500 unique IDs/500 duplicates/one conflicting status, verified source hashes, a complete output over 1 MiB, stdout beyond 64 KiB, immutable replay, unselected sibling denial and exact validation errors.
- `scripts/Verify-Beta.ps1`: actual shell network/UID/capability/credential isolation and tool behavior.
- `scripts/Verify-LLMLoop.ps1`, `scripts/Verify-WorkScenarios.ps1`, `scripts/Verify-Handoff.ps1`, `scripts/Verify-Persistence.ps1` and `scripts/Verify-Recovery.ps1`: separate real-account workflow, reference delivery, retry and restart checks. Their account/network/restart restrictions remain unchanged.

Synthetic checks do not prove completion of a live ESET incident Task. That Task must retain its original goal, prior analysis and receipts, reconcile uncertain outcomes, explicitly resume, and independently verify actual remaining business conditions.
