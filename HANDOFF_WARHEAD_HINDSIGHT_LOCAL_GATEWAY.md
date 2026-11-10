# Handoff: Warhead local HindSight gateway

## User request

Enable Warhead, when run locally, to start HindSight's HTTPS gateway and obtain
the ephemeral token automatically. The user plans to move this work to a folder
where both the Warhead and HindSight repositories are available.

## Current state

- At handoff creation, no local app-to-Warhead handoff had been implemented.
- HindSight already has a loopback-default HTTPS gateway, generates a fresh
  32-byte bearer token on start, binds it to the `local-gateway` user, and grants
  that user access to the open project.
- Gateway lifecycle/token handling is in `App/WorkbenchModel.swift` around
  `startRemoteGateway()`, `bindGatewaySession(_:)`, and `stopRemoteGateway()`.
- Gateway startup and HTTP behavior are in `App/RemoteGateway.swift`.
- Warhead's experimental HindSight MCP client is
  `BundledPlugins/Warhead.vgplugin/runtime/rea/hindsight-mcp-client.js` and is
  currently enabled only when `WARHEAD_HINDSIGHT_MCP_SERVER` is set; it also
  requires `HINDSIGHT_URL` and `HINDSIGHT_TOKEN` in the environment.
- The protocol bridge is in `BundledPlugins/Warhead.vgplugin/runtime/cli/repl.js`.
- MCP server source is `mcp/hindsight-mcp.mjs`; the bundle copy is
  `BundledPlugins/Warhead.vgplugin/runtime/mcp/hindsight-mcp.mjs`.
- MCP/setup documentation is `docs/HINDSIGHT_MCP.md`; bundle contract is
  `scripts/test-warhead-bundle.py`.
- Existing setup currently expects the user to copy/paste a gateway token.

## Suggested implementation direction

With both repos available, implement a private same-user local control channel
between the Warhead CLI and the running HindSight app. Warhead should be able to
request “ensure local gateway running” and receive only its loopback URL and
ephemeral token over that channel, then launch its bundled stdio MCP server with
those values in the child environment. Keep the token out of argv, persistent
configuration, normal logs, and MCP tool output. Preserve the app's current
project grant/authorization behavior; never expose a remote bind through this
automatic path. Handle app-not-running, no-open-project, gateway failure, and
token rotation/stop cleanly.

A per-user Unix domain socket was considered, but not implemented or validated.
Before choosing it, verify macOS availability/permissions and peer identity
checks for the deployment targets. The cross-repo integration may instead use a
small authenticated helper/IPC layer already present in Warhead.

## Repository state and validation

The HindSight worktree contains substantial uncommitted Warhead/MCP edits from
before this handoff. Preserve them; do not reset or overwrite them. In the prior
turn, `python3 scripts/test-warhead-bundle.py`, MCP stdio initialization/tool
listing, `git diff --check`, and `python3 scripts/validate-source.py` passed
(272 passed, 6 skipped). A small uncommitted follow-up updated the bundle test
and fixed packaged setup-script paths. Recheck the worktree after moving.

The user's latest instruction is to leave this note and continue in a folder
where both repositories are writable/available.

## Implementation update — 2026-11-10

Implemented in the shared workspace:

- HindSight starts a private Unix-domain control socket at
  `~/Library/Caches/HindSight/local-gateway.sock`, restricted to the current
  user's cache directory and socket permissions.
- Warhead requests `ensure` only when the socket exists. HindSight requires an
  open, ready project, retains the existing project grant, and starts the HTTPS
  gateway on `127.0.0.1`. An already-running non-loopback gateway is refused.
- The socket response carries the ephemeral token and the gateway certificate
  plus SHA-256 pin. Warhead passes them only to the bundled MCP child process;
  the MCP client pins TLS and does not persist or log the token.
- Manual URL/token configuration remains available. The same integration is
  present in Warhead source and the existing HindSight plugin runtime.

Validation on this host: Warhead `npm run build`; HindSight arm64 Debug
`xcodebuild`; `scripts/test-warhead-bundle.py` (93 modules); and
`scripts/validate-source.py` (272 passed, 6 skipped). Live GUI/app-to-CLI socket
qualification remains to be run with HindSight open on a ready project and its
Keychain TLS identity configured.
