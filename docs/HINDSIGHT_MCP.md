# HindSight MCP server

`mcp/hindsight-mcp.mjs` presents the authenticated HTTPS gateway of a running
HindSight instance as MCP tools (agent skills). The server is stdio-only and
does not expose imported binaries, the local Coordinator XPC service, or
ungranted projects outside HindSight's established authorization boundary.

## Warhead local integration

When Warhead's CLI and HindSight are running as the same macOS user, Warhead
automatically discovers HindSight's private control socket at
`~/Library/Caches/HindSight/local-gateway.sock`. Select a project and wait for
its session to become ready in HindSight; the next Warhead launch asks the app
to ensure its HTTPS gateway is running and receives a fresh URL, bearer token,
and certificate pin over that socket. Warhead starts the bundled stdio MCP
server with those values in its child environment. No token is written to
argv, config, logs, or tool output.

The control socket and its directory are restricted to the current macOS user.
The automatic path binds HTTPS only to `127.0.0.1`; if a non-local gateway is
already active, Warhead reports that condition and does not use it. Existing
project grants and gateway authorization remain in force. Stop the gateway in
HindSight to revoke the current token; the next Warhead session requests a new
one. If HindSight is closed or no project is ready, Warhead starts without the
optional MCP tools and prints the connection status.
If the gateway token is rotated or the gateway is stopped while a Warhead
session is already running, its current MCP child reports the revoked session;
restart Warhead after the gateway is available to obtain the fresh token.

## Manual client setup

For non-Warhead MCP clients, open the intended project, start the HTTPS gateway
in Settings, and either use its local token or enroll an identity and grant
that identity access to the project/record. The gateway URL is normally
`https://127.0.0.1:8443/v1`.

## Automatic registration

From this repository, run:

```sh
scripts/configure-hindsight-mcp.sh \
  --url https://127.0.0.1:8443/v1 \
  --token 'token-shown-by-hindsight'
```

It registers a `hindsight` stdio MCP server with Codex. Use `--client claude`
for Claude Code, or `--client print` to print the exact command for another
MCP client. For a private TLS issuer, also pass `--ca-file /path/to/issuer.pem`.
Tokens are placed in the MCP client's local configuration; treat that file as
a credential store and revoke/rotate the token in HindSight when needed.

For platform-specific setup, run `scripts/setup-warhead-mcp-macos.sh` or
`scripts/setup-warhead-mcp-linux.sh`; Windows users can run
`scripts/setup-warhead-mcp-windows.ps1` from PowerShell. These scripts prompt
for the gateway token instead of accepting it as a command-line argument. They
register the stdio server with the Codex CLI. When bundled through Warhead,
the setup scripts are staged under the plugin's `setup/` directory and the MCP
server under `runtime/mcp/`.

## Tool surface

The server exposes the complete currently supported remote gateway surface:

- `hindsight_capabilities`
- `hindsight_project`
- `hindsight_analysis_submit`
- `hindsight_analysis_status`
- `hindsight_analysis_result`
- `hindsight_analysis_cancel`

Analysis requests are source-bound by the project path, record ID, and
captured SHA-256 supplied by `hindsight_project`. An agent should discover
capabilities, inspect the project, submit a job, poll it, and then fetch its
result. The gateway retains its authorization, ownership checks, concurrency
limits, and audit trail.
