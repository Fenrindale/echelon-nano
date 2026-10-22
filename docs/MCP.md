# Workspace MCP

Open **MCP → MCP 추가** to install a pinned npm/Python package or connect a Streamable HTTP endpoint. **설치하고 연결** installs the package, initializes a real MCP session, discovers tools and saves their schemas. Failed initialization stays visible and never counts as a ready installation.

Link environment variable names to existing Vault bundle variables. PostgreSQL stores references; Broker resolves values only at execution time. Remote Bearer authentication uses `MCP_AUTH_TOKEN`. Credential flags and credential-bearing URLs are rejected; use Vault bindings. Known authentication fields in MCP results are masked even for server-generated tokens. There is no per-Task connector registration or approval form.

Handler inspects relevant installed schemas and freezes `mcp_tools:[{server_id,version,tools:[name]}]` plus `mcp.call` in the Task contract. Agent receives selected schemas in its immutable Work Order. Analyst and Master cannot inspect or execute MCP; Handler cannot execute it. Disabling a server prevents later calls. Configuration changes create versions; Work Orders retain their pinned schemas/configuration.

Completed effects reuse a durable receipt for the same request. A lost reply leaves an unknown outcome and cannot be automatically replayed; inspect external state first. MCP `isError` remains a tool failure. Sessions and `work_order.read.tool_evidence.mcp_calls` expose actual results.

The separate stdio host has a persistent package volume and no DB, Vault or Docker socket access. Installation receives no credentials; npm lifecycle scripts and Python source builds are disabled. Each call uses a new unprivileged UID, temporary directory, constructed environment, resource limits and MCP session. The offline Worker is unchanged. Packages requiring build scripts/native compilation or persistent stdio conversations need a separate adapter. Remote servers use bounded JSON/SSE messages, exact endpoints, session IDs, protocol negotiation and pinned-address network policy.

The client advertises no sampling, elicitation, roots or browser-login capability. Interactive MCP OAuth is not implemented; configured Vault Bearer references are supported. Binary MCP output stays bounded JSON rather than being downloaded/executed automatically.

## Human input and continuation

Handler uses `credential.inspect {bundle_id}` for safe workspace metadata without opening another Task's raw artifact. Automatic input requests require a missing business field, why it cannot be inferred, checked alternatives and `can_continue:false`. Technical setup requests are rejected. For reversible incident collection, new defaults to unresolved since the last recorded review, otherwise all unresolved; report the assumption. Original requested actions and Work Order scope remain authoritative.

Pinned evidence survives replans/context rounds. `context.read {op:'resolve_many',ref_ids:[...]}` resolves up to 16 references per model turn. Artifact-backed completion accepts actual saved artifacts without forcing an unrelated central citation. Token usage stays observe-only.

The local Artex comparison examined `agent/planner.go`: planning consumes actual Worker progress and continues useful remaining work while avoiding duplicate work. Echelon's missing shared runtime and credential metadata scope mismatch required implementation changes beyond prompt wording. No Artex source is copied.

## Verification

`scripts/Verify-Mcp.ps1` targets the isolated port-18432 copy: real HTTP/SSE discovery/calls, pinned schemas, role/tenant denial, Vault masking, cross-Task bundle metadata, effect replay, unknown write outcomes and a pinned official npm stdio example. An owned OAuth Task verifies automatic tool selection and execution without additional input. Synthetic tests do not establish ESET account access or incident resolution.

Protocol reference: [transports](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports), [tools](https://modelcontextprotocol.io/specification/2025-03-26/server/tools).
