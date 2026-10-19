# Echelon Nano

Rust and Docker foundations for a shared-knowledge work agent system.

Analyst curates registered knowledge, Handler plans and delegates work, Agent executes a scoped Work Order, and Master audits and optimizes knowledge. Role capabilities are enforced by the server and signed, short-lived scope tokens.

## Current scope

Phase 0 implements the control server, Vault-backed OAuth credentials, Sign in with ChatGPT, streamed connection probes, serialized refresh rotation, and isolated role verification containers. Phase 1 adds PostgreSQL central knowledge with Data/Info/Intel, versioned tags and reference edges, immutable failure history, and Handler submission → Analyst adoption or rejection.

Phase 2 adds Handler context requests, Analyst candidate selection, immutable Context Packs with pinned evidence references, and task-scoped Handler resolution. Packs carry metadata and cautions; full knowledge is loaded through individual references.

Phase 3 adds bounded OAuth-backed Handler/Analyst/Agent execution loops, scoped Work Order evidence contracts, immutable results and Handler collection. See [execution contracts, API and limits](docs/EXECUTION.md). External fetch and shell adapters, Master optimization, protocols, and the full interface are subsequent phases. An allowed capability with no adapter returns HTTP 501 and performs no action.

The initial model connection uses the official Sign in with ChatGPT route. It does not reuse another application's Codex credentials, call private ChatGPT inference endpoints, or silently fall back to a billed API key.

## Requirements

- Docker Desktop with Linux containers and Compose.
- Windows and PowerShell 7 for the local setup scripts and DPAPI protection.
- An eligible ChatGPT account and explicit consent for ChatGPT plan usage.
- Available local port 18431.

A host Rust installation is optional. Docker pins Rust 1.90.0; Cargo.lock is committed.

## Start locally

From the repository root, run:

```powershell
./scripts/Initialize-Local.ps1
./scripts/Verify-Roles.ps1
./scripts/Start-Login.ps1
```

The last command opens the official account selector and consent flow. After the callback reports success:

```powershell
./scripts/Verify-LiveOAuth.ps1
```

The live verifier uses the connected account's model catalog, completes a fixed synthetic inference, sends four concurrent refresh requests for the same generation, then completes another inference. It must observe response.completed for success. A listed model is not sufficient proof of access.

The control page is available at http://127.0.0.1:18431. Operational APIs require a local operator credential. Do not publish this localhost setup directly to the internet.

## Credentials and storage

OAuth access, refresh and ID tokens are stored only in Vault. Public metadata and audit records contain no credential values. The role containers receive only their own short-lived session token and cannot connect directly to Vault, PostgreSQL or the internet.

The local Vault instance uses persistent file storage on an internal Docker network. This single-machine setup uses one unseal share. Recovery material and the local operator credential are encrypted for the current Windows user with DPAPI. Scoped bootstrap credentials and the database password are restricted runtime files outside the Git and Docker build contexts.

After restarting a sealed Vault, rerun Initialize-Local.ps1 -SkipBuild to restore it and rotate the scoped application token. Keep an independent protected backup of the recovery material; DPAPI recovery is tied to the Windows user and machine.

All .runtime content is private and ignored by Git and Docker. Keep evidence, credentials and database volumes out of public issues and commits.

## Verification

```powershell
docker build --target checks .
docker build --target runtime -t echelon-nano:local .
```

These checks run rustfmt, Clippy with warnings denied, unit tests, and ephemeral RSA/JWKS validation tests. Live OAuth and container isolation require the local setup and are recorded separately from unit tests.

To test the knowledge workflow against an isolated, disposable PostgreSQL database:

```powershell
docker compose -f compose.verify.yaml up --build --abort-on-container-exit --exit-code-from tests
docker compose -f compose.verify.yaml down --remove-orphans
```

GitHub CI runs this database test separately; the database test is intentionally ignored by plain cargo test. After rebuilding and starting the local core, run `./scripts/Verify-Knowledge.ps1 -RestartCore` for the actual authenticated HTTP workflow, isolation and restart checks. It retains tagged synthetic records in a separate verification tenant and saves private evidence under .runtime/evidence.

Run `./scripts/Verify-ContextPacks.ps1 -RestartCore` to verify the Context Pack request, curation, reference resolution, evidence pinning and scope boundaries through actual authenticated HTTP calls. The isolated PostgreSQL verification also covers Context Packs and concurrent delivery retries.

Run `./scripts/Verify-LLMLoop.ps1` with an explicitly connected account to execute an owned synthetic task through all three actual model roles, verify evidence and failure history delivery, retrieve the Agent result, and exercise role/scope and call-budget limits. This makes real ChatGPT-plan inference requests; raw evidence remains private.

See [the design](docs/PLAN.ko.md), [Phase 0 validation](docs/PHASE0.md), [the knowledge API](docs/KNOWLEDGE.md), and [Context Packs and Phase 2 validation](docs/CONTEXT_PACKS.md). This is a new Rust implementation inspired by workflow concepts from Artex; it does not include Artex source code.

## Official connection references

- [Registration and sign-in](https://developers.openai.com/siwc/token-sharing-open-source/sign-in)
- [Models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)
- [Accounts and sessions](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions)
- [Preview limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)

MIT License.
