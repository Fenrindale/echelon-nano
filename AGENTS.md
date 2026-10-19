# Echelon Nano development

Read README.md, docs/PLAN.ko.md and docs/PHASE0.md before changing the architecture. Keep the distinction between design, implemented adapters, local tests, and live account verification.

## Verification

- Run `docker build --target checks .` for Rust formatting, Clippy and tests. Rust is pinned in rust-toolchain.toml; keep Cargo.lock committed.
- Build the runtime with `docker compose build core`.
- On the configured Windows development machine, `scripts/Verify-Roles.ps1` creates owned synthetic records and checks actual dispatch and isolation.
- Run the isolated PostgreSQL test with `docker compose -f compose.verify.yaml up --build --abort-on-container-exit --exit-code-from tests`, then remove only that verification project with `docker compose -f compose.verify.yaml down --remove-orphans`. Never point the destructive fixture test at the working database.
- For knowledge changes, run `scripts/Verify-Knowledge.ps1 -RestartCore` against the local core. Preserve submitted payloads, negative evidence and prior revisions; use expected versions for updates.
- For Context Packs, run `scripts/Verify-ContextPacks.ps1 -RestartCore`. Handler access must remain bound to an explicitly delivered pack, its target Task and its immutable references; keep the central knowledge.read capability unavailable to Handler.
- `scripts/Verify-LiveOAuth.ps1` requires an explicitly connected account and makes small real inference and refresh requests. Report its result separately from unit tests.
- For role execution changes, run `scripts/Verify-LLMLoop.ps1` with the explicitly connected account. Preserve terminal-response gating, role/phase checks, pinned Work Order evidence, result collection, bounded calls and fail-stop restart behavior. Do not replay unknown provider or tool outcomes automatically.
- Run `scripts/Test-PublicContent.ps1` and `git diff --check` before publishing.
- For beta tools and console changes, run `scripts/Verify-Beta.ps1`. Changes to the full workflow also need `scripts/Verify-WorkScenarios.ps1` with the explicitly connected OAuth account. Keep the controlled performance sample and setup/retry costs distinguishable.
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
