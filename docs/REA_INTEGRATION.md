# REA Agent Integration

HindSight will support [REA](https://github.com/morluto/rea) as an optional local analysis provider. REA is an external Node-based MCP server, so it should remain a separately versioned process rather than being embedded into the HindSight binary.

## Integration boundary

The bridge will:

1. Launch a user-approved `rea-agents` command as a child process over stdio.
2. Initialize MCP and call `binary_session` to discover the available providers and tools.
3. Require an explicit target path and operation before starting analysis.
4. Stream bounded progress and results without blocking SwiftUI or the coordinator.
5. Preserve REA evidence IDs, target hashes, provider identity, analysis profile, limitations, and provenance in the HindSight ledger.
6. Keep raw REA output on disk and pass references through XPC instead of placing unbounded payloads in the UI process.

The bridge must fail closed when REA, Node, or the selected provider is unavailable. It must never silently upload target contents or broaden a target scope. REA remains opt-in and local-first.

## Delivery slices

- [x] Document the MCP/evidence boundary and local-first safety rules.
- [x] Expose an opt-in REA readiness panel in Settings.
- [x] Add a coordinator-owned stdio MCP probe with timeout cleanup and bounded I/O.
- [x] Add initial provider discovery through REA's `binary_session` tool.
- [x] Add explicit target/provider selection for bounded overview, procedure, and name operations.
- [ ] Add capability-aware operation selection for discovered providers.
- [x] Add a dedicated REA Agent workspace with explicit target/provider controls.
- [x] Add explicit cancellation for active REA operations.
- [x] Include bounded REA responses and SHA-256 identities in HindSight case exports.
- [x] Expose retained REA records as provenance nodes in the Evidence Graph.
- [x] Restore retained REA records when importing a `.vgcase`.
- [x] Add bounded streamed progress history and retained evidence to the REA Agent workspace.
- [ ] Import REA evidence into cases and the evidence graph with compatibility checks.
- [ ] Add contract tests using deterministic MCP fixtures and release qualification coverage.

The first bridge implementation should pin a tested `rea-agents` version instead of using an unbounded `latest` dependency. Users can install REA with the upstream setup flow, then select the executable/package in HindSight.
