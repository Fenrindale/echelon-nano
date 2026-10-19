# Echelon Nano development

Read README.md, docs/PLAN.ko.md and docs/PHASE0.md before changing the architecture. Keep the distinction between design, implemented adapters, local tests, and live account verification.

## Verification

- Run `docker build --target checks .` for Rust formatting, Clippy and tests. Rust is pinned in rust-toolchain.toml; keep Cargo.lock committed.
- Build the runtime with `docker compose build core`.
- On the configured Windows development machine, `scripts/Verify-Roles.ps1` creates owned synthetic records and checks actual dispatch and isolation.
- Run the isolated PostgreSQL test with `docker compose -f compose.verify.yaml up --build --abort-on-container-exit --exit-code-from tests`, then remove only that verification project with `docker compose -f compose.verify.yaml down --remove-orphans`. Never point the destructive fixture test at the working database.
- For knowledge changes, run `scripts/Verify-Knowledge.ps1 -RestartCore` against the local core. Preserve submitted payloads, negative evidence and prior revisions; use expected versions for updates.
- `scripts/Verify-LiveOAuth.ps1` requires an explicitly connected account and makes small real inference and refresh requests. Report its result separately from unit tests.
- Run `scripts/Test-PublicContent.ps1` and `git diff --check` before publishing.

## Boundaries

- Keep credentials, account evidence and recovery material under ignored `.runtime`; never include them in source, command arguments, logs or public issues. Use Vault for OAuth tokens.
- Use the official Sign in with ChatGPT protocol. Do not import another application's authentication files or add silent API-key fallback.
- Enforce the role policy and tenant/task/Work Order scope on the server. A caller-supplied role or prompt is not an authorization source.
- Analyst curates submitted knowledge; Handler plans and delegates; Agent acts within its Work Order; Master audits and optimizes. All roles are denied raw Vault access and audit rewriting.
- Return 501 for allowed capabilities whose adapters are not implemented. Do not report them as working automation.
- Keep Docker resources within the `echelon-nano` Compose project. Preserve unrelated containers and volumes.
