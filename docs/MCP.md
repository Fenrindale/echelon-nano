# Workspace MCP

Open **MCP → MCP 추가** to install a pinned npm/Python package or connect a Streamable HTTP endpoint. **설치하고 연결** installs the package, initializes a real MCP session, discovers tools and saves their schemas. Failed initialization stays visible and never counts as a ready installation.

Link environment variable names to existing Vault bundle variables. PostgreSQL stores references; Broker resolves values only at execution time. Remote Bearer authentication uses `MCP_AUTH_TOKEN`. Credential flags and credential-bearing URLs are rejected; use Vault bindings. Known authentication fields in MCP results are masked even for server-generated tokens. There is no per-Task connector registration or approval form.

## Handler preparation

Handler can prepare a missing integration during configuration, delegation or result collection. It uses `mcp.search {query,transport,limit}` for public npm text search (or an exact PyPI project lookup), then `mcp.describe {package,transport,version?,offset?}` for a release, executable names and bounded registry documentation. Only fixed npm/PyPI registry origins are queried, without credentials, redirects or proxy settings. Registry descriptions are untrusted data. Python module names are inferred from the distribution name and must be checked against its documentation. See the [npm registry API](https://github.com/npm/registry/blob/main/docs/REGISTRY-API.md) and [PyPI JSON API](https://docs.pypi.org/api/json/).

`mcp.prepare {name,reason,spec}` installs a pinned npm/Python release or initializes an HTTP MCP endpoint and discovers actual tool schemas. `spec` uses the same fields as the MCP menu: `transport`, `package`, `package_version`, `executable`, `args`, `url`, `allow_private`, `allow_http`, and `environment:[{name,bundle_id,variable}]`. The execution harness assigns `request_id`; direct role API callers supply their own stable UUID. Handler chooses configuration, while the isolated MCP host installs and initializes it. This operation never invokes a business tool.

Credential bundles must first be selected in the Task contract. Bindings must belong to the same tenant and an externally accessible source Task; raw values and control-environment overrides are rejected. Private/plain-HTTP endpoints must occur in the original goal or already be exact Task URL grants. Handler can resolve registered evidence and finalize `run.configure_contract` before preparation. The package installer receives no credentials; Broker supplies mapped values only for initialization/calls.

Identical enabled workspace configurations reuse a server and its version. A different configuration creates a separate server, preserving earlier revisions and Work Orders. Durable preparation requests/results are scoped by tenant, Task and request key. Completed or failed requests replay the original receipt; conflicting payloads are rejected. Failed initialization stays visible and a corrected/new preparation can retry it. An interrupted request can reconcile an already saved revision; it never replays an unknown business call. Preparation receipts prove infrastructure readiness, not upstream authentication or incident resolution.

On `status:ready`, Handler inspects the current server, updates the full Task contract with the required `mcp.call` grants and delegates a new Work Order. The prepared ID/version remains in Handler's checkpoint across context changes and restarts. An Agent whose existing Work Order lacks these grants must return to Handler for replanning. Analyst, Agent and Master cannot search registries or prepare MCP infrastructure; Handler cannot execute MCP business tools. The offline Worker is never used as a package installer.

Handler inspects relevant installed schemas and freezes `mcp_tools:[{server_id,version,tools:[name]}]` plus `mcp.call` in the Task contract. Agent receives selected schemas in its immutable Work Order. Analyst and Master cannot inspect or execute MCP; Handler cannot execute it. Disabling a server prevents later calls. Configuration changes create versions; Work Orders retain their pinned schemas/configuration.

Completed effects reuse a durable receipt for the same request. A lost reply leaves an unknown outcome and cannot be automatically replayed; inspect external state first. MCP `isError` remains a tool failure. Sessions and `work_order.read.tool_evidence.mcp_calls` expose actual results.

The separate stdio host has a persistent package volume and no DB, Vault or Docker socket access. Installation receives no credentials; npm lifecycle scripts and Python source builds are disabled. Calls in one Work Order reuse an isolated unprivileged UID, temporary home, constructed environment and MCP session. Different Work Orders and changed configurations/secret bindings use separate sessions. Idle stdio sessions expire after 30 minutes; Work Order completion closes their processes. Streamable HTTP session headers are held in memory within the same scope. The offline Worker is unchanged. Packages requiring build scripts/native compilation need a separate adapter. Remote servers use bounded JSON/SSE messages, exact endpoints, session IDs, protocol negotiation and pinned-address network policy.

The client advertises no sampling, elicitation, roots or browser-login capability. Interactive MCP OAuth is not implemented; configured Vault Bearer references are supported. Binary MCP output stays bounded JSON rather than being downloaded/executed automatically.

## Human input and continuation

Handler uses `credential.inspect {bundle_id}` for safe workspace metadata without opening another Task's raw artifact. Automatic input requests require a missing business field, why it cannot be inferred, checked alternatives and `can_continue:false`. Technical setup requests are rejected. For reversible incident collection, new defaults to unresolved since the last recorded review, otherwise all unresolved; report the assumption. Original requested actions and Work Order scope remain authoritative.

Pinned evidence survives replans/context rounds. `context.read {op:'resolve_many',ref_ids:[...]}` resolves up to 16 references per model turn. Artifact-backed completion accepts actual saved artifacts without forcing an unrelated central citation. Token usage stays observe-only.

The local Artex comparison examined `agent/planner.go`: planning consumes actual Worker progress and continues useful remaining work while avoiding duplicate work. Echelon's missing shared runtime and credential metadata scope mismatch required implementation changes beyond prompt wording. No Artex source is copied.

## Verification

`scripts/Verify-Mcp.ps1` targets the isolated port-18432 copy: real HTTP/SSE discovery/calls, pinned schemas, role/tenant denial, Vault masking, cross-Task bundle metadata, effect replay, unknown write outcomes and a pinned official npm stdio example. An owned OAuth Task verifies automatic tool selection and execution without additional input. Synthetic tests do not establish ESET account access or incident resolution.

`scripts/Verify-McpPreparation.ps1 -WithOAuth` additionally verifies Handler-owned preparation, credential/endpoint/source restrictions, immutable failed/successful receipts, configuration reuse, no business calls during initialization, and Agent execution of prepared tools. Its OAuth scenario starts with an empty MCP catalog and requires the model to select a published npm release, prepare it, grant only the needed schema, and recover an actual echoed file without technical user input. PostgreSQL regressions verify preparation receipt ownership, success/failure evidence and immutability separately.

Protocol reference: [transports](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports), [tools](https://modelcontextprotocol.io/specification/2025-03-26/server/tools).
