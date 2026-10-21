# Echelon Nano development

Read README.md, docs/PLAN.ko.md and docs/PHASE0.md before changing the architecture. Keep the distinction between design, implemented adapters, local tests, and live account verification.

## Verification

- Run `docker build --target checks .` for Rust formatting, Clippy and tests. Rust is pinned in rust-toolchain.toml; keep Cargo.lock committed.
- Build the runtime with `docker compose build core`.
- On the configured Windows development machine, `scripts/Verify-Roles.ps1` creates owned synthetic records and checks actual dispatch and isolation.
- Run the isolated PostgreSQL test with `docker compose -f compose.verify.yaml up --build --abort-on-container-exit --exit-code-from tests`, then remove only that verification project with `docker compose -f compose.verify.yaml down --remove-orphans`. Never point the destructive fixture test at the working database.
- For knowledge changes, run `scripts/Verify-Knowledge.ps1 -RestartCore` against the local core. Preserve submitted payloads, negative evidence and prior revisions; use expected versions for updates.
- For conversation corpus changes, run `scripts/Verify-BodyIngestion.ps1` only in the isolated port-18432 verification copy. Use actual body text and real role inference, retain rejected decisions and source coverage, and verify that the operator transports sources while Handler submits and Analyst publishes. Never register synthetic fixtures in a live user corpus Task.
- For Context Packs, run `scripts/Verify-ContextPacks.ps1 -RestartCore`. Handler access must remain bound to an explicitly delivered pack, its target Task and its immutable references; keep the central knowledge.read capability unavailable to Handler.
- `scripts/Verify-LiveOAuth.ps1` requires an explicitly connected account and makes small real inference and refresh requests. Report its result separately from unit tests.
- For role execution changes, run `scripts/Verify-LLMLoop.ps1` with the explicitly connected account. Preserve terminal-response gating, role/phase checks, pinned Work Order evidence and result collection. Automatic launches use bounded model rounds, while explicit manual runs retain their call limits. Retry only transient model generation whose output was never committed or executed; record each attempt and unknown usage. Never replay unknown business tool outcomes automatically. Server restarts still require explicit reconciliation/resume.
- Run `scripts/Verify-Persistence.ps1` only in the isolated port-18432 copy for controlled provider-network interruption, pause during retry, goal supervision and round continuation. Restore its exact provider network in a finally block; never disconnect the live project's network.
- Run `scripts/Test-PublicContent.ps1` and `git diff --check` before publishing.
- For beta tools and console changes, run `scripts/Verify-Beta.ps1`. Changes to the full workflow also need `scripts/Verify-WorkScenarios.ps1` with the explicitly connected OAuth account. Keep the controlled performance sample and setup/retry costs distinguishable.
- For prompt-only launches, run `scripts/Verify-GoalLaunch.ps1` and `scripts/Verify-Recovery.ps1 -PromptOnly`. Check real Handler settings, the Analyst/Agent loop, immutable artifacts, idempotent launch replay and settings preserved across restart. Handler configuration must stay inside installed capabilities and registered Task/tenant references.
- For attachments, run `scripts/Verify-Attachments.ps1` and `scripts/Verify-GoalLaunch.ps1 -WithAttachments`. Keep Task/file creation atomic, retain exact bytes and request manifests, bound previews, and preserve Work Order input grants. Binary storage does not prove installed document extraction support.
- Verify checkpoint changes with `scripts/Verify-Recovery.ps1` when no other Echelon run is active; it intentionally kills only this project's core, preserves Worker state and checks explicit resume and SSE replay.
- Worker children must inherit the child-only seccomp policy. The RPC controller's capabilities and token must never reach the unprivileged process. Do not add general network access, mounts or credentials to jobs.
- Add new migrations after applied versions. Keep Work Order contracts, observations, completed effects and terminal execution checkpoints immutable.

## Boundaries

- Keep credentials, account evidence and recovery material under ignored `.runtime`; never include them in source, command arguments, logs or public issues. Use Vault for OAuth tokens.
- Use the official Sign in with ChatGPT protocol. Do not import another application's authentication files or add silent API-key fallback.
- Enforce the role policy and tenant/task/Work Order scope on the server. A caller-supplied role or prompt is not an authorization source.
- Analyst curates submitted knowledge; Handler plans and delegates; Agent acts within its Work Order; Master audits and optimizes. All roles are denied raw Vault access and audit rewriting.
- Return 501 for allowed capabilities whose adapters are not implemented. Do not report them as working automation.
- Keep Docker resources within the `echelon-nano` Compose project. Preserve unrelated containers and volumes.
