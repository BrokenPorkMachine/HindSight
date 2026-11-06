# REA Agent Integration

HindSight will support [REA](https://github.com/morluto/rea) as an optional local analysis provider. REA is an external Node-based MCP server, so it should remain a separately versioned process rather than being embedded into the HindSight binary.

## Integration boundary

The bridge will:

1. Launch a user-configured, already-installed `rea-agents` command as a child process over stdio. Normal operation never downloads a package.
2. Initialize MCP and call `binary_session` to discover the available providers and tools.
3. Require an explicit target path and operation before starting analysis.
4. Stream bounded progress and results without blocking SwiftUI or the coordinator.
5. Preserve REA evidence IDs, target hashes, provider identity, analysis profile, limitations, and provenance in the HindSight ledger.
6. Keep every payload bounded across XPC and case storage. A future external evidence store may replace inline case payloads without changing provider identity fields.

The bridge must fail closed when REA, Node, or the selected provider is unavailable. It must never silently upload target contents or broaden a target scope. REA remains opt-in and local-first.

## Delivery slices

- [x] Document the MCP/evidence boundary and local-first safety rules.
- [x] Expose an opt-in REA readiness panel in Settings.
- [x] Add a coordinator-owned stdio MCP probe with timeout cleanup and bounded I/O.
- [x] Add initial provider discovery through REA's `binary_session` tool.
- [x] Add explicit target/provider selection for bounded overview, procedure, and name operations.
- [x] Gate target operations on explicit `binary_session.tool_availability` results when supplied.
- [x] Add a dedicated REA Agent workspace with explicit target/provider controls.
- [x] Add explicit cancellation for active REA operations.
- [x] Include bounded REA responses and SHA-256 identities in HindSight case exports.
- [x] Expose retained REA records as provenance nodes in the Evidence Graph.
- [x] Restore retained REA records when importing a `.vgcase`.
- [x] Add bounded streamed progress history and retained evidence to the REA Agent workspace.
- [x] Import REA evidence into cases and the evidence graph with compatibility checks.
- [x] Add a deterministic MCP fixture contract for handshake, capability gating, and progress retention.
- [x] Add deterministic release-preflight qualification coverage for the bridge; real-provider qualification remains a manual release-host gate.

Release qualification pins the supported REA version. Users install REA with the upstream setup flow, then configure its local executable and arguments in HindSight. The default `/usr/bin/env rea-agents mcp` resolves only an already-installed command and never invokes `npx`.

## Live qualification

The deterministic fixture remains part of release preflight. On a release host with an approved local target, qualify the actual provider through the same Swift coordinator bridge:

```bash
python3 scripts/inspect-rea-mcp.py
python3 scripts/qualify-rea-live.py /absolute/path/to/rea-agents /absolute/path/to/approved-target
```

For the bundled Warhead surface, start the configured local model endpoint and run:

```bash
python3 scripts/qualify-warhead-live.py model-tag
```

This verifies model transport and bounded REA evidence delivery. It is not an assessment of the selected model's analytical quality.
