# Echelon Nano

Shared knowledge and bounded work agents, built with Rust and Docker. Local beta 0.2.

Analyst curates registered knowledge, Handler plans and delegates work, Agent executes a scoped Work Order, and Master audits and optimizes knowledge. Role capabilities are enforced by the server and signed, short-lived scope tokens.

## Current scope

The first beta includes all four roles, Data/Info/Intel knowledge, immutable revisions and failure history, pinned Context Packs, scoped Work Orders, artifact recovery, pause/cancel/explicit resume, and separate knowledge retention and Master audit stages.

Master reviews activity and repetition, proposes versioned merges/splits/revisions and protocols, and queues separate verification requests. Semantic changes go through operator review; exact duplicates can be linked without deleting either original. Active protocols load their pinned information sets directly and use a tenant- and policy-aware cache.

The Korean operations console includes Tasks with Brief/Path/Broadcast/Sessions/Knowledge/Outputs, Central, Protocols, Dreaming, Vault, and Settings. Path connects business records and pinned evidence, Broadcast lists the same records by creation time with current status, and Sessions shows role-specific execution receipts across ordinary runs, body ingestion and retention review. History is paginated and sensitive/private model data is excluded. See [Task observability](docs/OBSERVABILITY.md). The dark visual theme has one editable accent color with contrast correction. The local console opens directly without a password or pairing code. An automatic HttpOnly session supports CSRF protection; a user login system is deferred.

To start a Task, enter the business goal and optionally attach files. Handler selects an available model, a bounded call count per model round, completion and verification conditions, tools, protocol, input files and retention settings. After Analyst delivers pinned evidence, Handler finalizes the contract before delegating. Brief shows the resulting settings and attachments for inspection. Automatic Tasks refresh their model context when a call/time/context round limit is reached, preserving the goal, role, Work Order and confirmed receipts. Transient uncommitted model requests retry with observable backoff; Handler must assess goal completion and replan feasible remaining work. Token usage is collected without an application token cap. Account connection remains a one-time workspace setup; missing business inputs may be requested during execution.

Drag files into the attachment area or use the file selection button. New Tasks accept up to 16 attachments, each at most 1 MiB and at most 4 MiB combined. Files and the Task are saved together before execution starts; retrying the same request reuses their immutable bytes and IDs. Handler reads bounded UTF-8 previews and grants only selected files to Agent. Other binary formats can be stored and downloaded; PDF/Office extraction adapters remain outside this beta. Existing Tasks also accept file drops in Brief and Outputs, one file at a time; pause an active execution before adding files. Folder drops are rejected, and invalid additions preserve files already selected.

Large conversation archives can be processed as a durable [body-to-knowledge job](docs/CONVERSATION_INGESTION.md) within an existing Task. Agent reads actual dialogue, Handler authors substantive Data/Info/Intel submissions and Analyst decides adoption from pinned sources. Source inventories remain distinct from reusable knowledge; missing bodies, rejected candidates, failure history and versioned derivation links are preserved. The Task console shows body coverage, category adoption and pause/resume progress separately from earlier runs.

Agent tools include exact-URL read-only HTTP requests through a Vault-backed Secret Broker, immutable file artifacts, and a dedicated Linux Worker. Shell children run under an unprivileged UID, with resource limits, no network syscalls, no credentials or host mounts, and a temporary job directory. Initial document workflows use UTF-8 Markdown, CSV and source files; arbitrary Office/PDF processing adapters are outside this beta.

Credential attachments use a server-side broker. Handler reads variable names and non-secret service endpoints, configures Task-local connections, and delegates literal `.env` import and GitLab/Jenkins/Databricks identity reads to Agent. No connector registration or separate operator approval is required. Work Orders freeze the selected configuration; values stay out of model previews and the Worker. Handler retrieves actual storage receipts and identity observations when assessing the result. Missing adapters or business targets remain explicit limitations. See [credential handling and its limits](docs/CREDENTIALS.md).

See the [beta guide and API contracts](docs/BETA.md), [execution contracts](docs/EXECUTION.md), and [implementation evidence](docs/IMPLEMENTATION.md). Vector search, object storage, WCM, app-server, additional business write connectors and remote multi-user hosting remain the optional extensions identified in the design.

The initial model connection uses the official Sign in with ChatGPT route. It does not reuse another application's Codex credentials, call private ChatGPT inference endpoints, or silently fall back to a billed API key.

Shared workspace MCP installations now have a dedicated **MCP** menu. Handler selects installed tools, Agent executes only Work Order grants, and Vault injects referenced credentials. npm/Python stdio and Streamable HTTP transports are supported; see [MCP setup, boundaries and continuation](docs/MCP.md).

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
./scripts/Open-Console.ps1
```

Start-Login opens the official account selector and consent flow for model access. Open-Console opens the local workspace directly. After the OAuth callback reports success:

```powershell
./scripts/Verify-LiveOAuth.ps1
```

The live verifier uses the connected account's model catalog, completes a fixed synthetic inference, sends four concurrent refresh requests for the same generation, then completes another inference. It must observe response.completed for success. A listed model is not sufficient proof of access.

The console is available at http://127.0.0.1:18431. Anyone using this local console has operator access; there is no user authentication gate. Browser sessions are issued automatically and cookie mutations require a CSRF token. Direct automation uses the local operator credential, while model roles still require signed scope tokens. This installation targets one local operator with loopback-only port binding.

For an explicitly trusted LAN, add the exact host-IP port mappings in a local `compose.override.yaml` and set `ECHELON_UI_ORIGINS` on core to the corresponding comma-separated browser origins (for example `http://192.0.2.10:18431`). Restrict the host firewall to the intended local addresses and client subnets. The default remains loopback-only. The override is excluded from Git and the Docker build context. Browser Origin and Host must match an approved origin; CSRF and role scopes remain enforced. Anyone admitted to this login-free console has operator access. OAuth account connection still uses the host's loopback callback.

## Credentials and storage

OAuth access, refresh and ID tokens are stored only in Vault. Public metadata and audit records contain no credential values. The role containers receive only their own short-lived session token and cannot connect directly to Vault, PostgreSQL or the internet.

The local Vault instance uses persistent file storage on an internal Docker network. This single-machine setup uses one unseal share. Recovery material and the local operator credential are encrypted for the current Windows user with DPAPI. Scoped bootstrap credentials and the database password are restricted runtime files outside the Git and Docker build contexts.

After restarting a sealed Vault, rerun Initialize-Local.ps1 -SkipBuild to restore it and rotate the scoped application token. Keep an independent protected backup of the recovery material; DPAPI recovery is tied to the Windows user and machine.

All .runtime content is private and ignored by Git and Docker. Keep evidence, credentials and database volumes out of public issues and commits.

`./scripts/Backup-Local.ps1 -VerifyRestore` stops this project's writers, creates separate database/artifact and encrypted Vault-storage archives, and restores the database into an empty isolated verification container. It then restores the working stack. Keep independent protected recovery material: this command does not include unseal credentials or rehearse a Vault disaster restore.

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

`./scripts/Verify-GoalLaunch.ps1` checks the prompt-only entry with real OAuth inference: Handler settings and contract finalization, Analyst/Agent work, pinned report evidence, artifact hashes and duplicate-request replay. `-Resume` rechecks its saved owned run without starting another inference. `./scripts/Verify-Recovery.ps1 -PromptOnly` also checks that Handler settings and the finalized contract survive core interruption.

`./scripts/Verify-Attachments.ps1` checks large HTTP uploads, atomic rejection, binary downloads, Unicode previews, replay and Task isolation. `./scripts/Verify-GoalLaunch.ps1 -WithAttachments` runs an actual OAuth report workflow using attached CSV and text files.

Run `./scripts/Verify-Beta.ps1` for the actual broker, Vault rotation, Worker limits, artifact replay and console session boundaries. `./scripts/Verify-WorkScenarios.ps1` makes real OAuth calls for report generation, constrained source changes, request classification, Master protocol proposals and protocol reuse. It measures a controlled Handler/Agent baseline, Analyst selection and protocol reuse, with setup/audit costs shown separately. `./scripts/Verify-Recovery.ps1` deliberately interrupts only this project's core while a synthetic Worker job is running, then checks explicit resume, a single job execution, and SSE cursor replay. Run recovery when no unrelated Echelon Task is active.

Compose's parallel build can reject non-ASCII Windows workspace paths on some Docker Desktop versions. Initialize-Local uses sequential `docker build` commands for the same runtime and Worker targets. Direct commands are also available:

```powershell
docker build --target runtime -t echelon-nano:local .
docker build --target worker -t echelon-nano-worker:local .
docker build --target mcp-host -t echelon-nano-mcp:local .
./scripts/Initialize-Local.ps1 -SkipBuild
```

See [the design](docs/PLAN.ko.md), [Phase 0 validation](docs/PHASE0.md), [the knowledge API](docs/KNOWLEDGE.md), and [Context Packs and Phase 2 validation](docs/CONTEXT_PACKS.md). This is a new Rust implementation inspired by workflow concepts from Artex; it does not include Artex source code.

## Official connection references

- [Registration and sign-in](https://developers.openai.com/siwc/token-sharing-open-source/sign-in)
- [Models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)
- [Accounts and sessions](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions)
- [Preview limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)

MIT License.

See [central versus Task-local retention](docs/RETENTION.md) for decision-utility requirements, legacy review, local preservation and verification. Central adoption counts are historical receipts, distinct from current placement.
