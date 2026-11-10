#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT"
SERVER="$ROOT/mcp/hindsight-mcp.mjs"
[[ -f "$SERVER" ]] || { echo "Missing HindSight MCP server: $SERVER" >&2; exit 1; }
command -v codex >/dev/null || { echo 'Install Codex CLI first.' >&2; exit 1; }
read -r -p 'HindSight gateway URL [https://127.0.0.1:8443/v1]: ' URL
URL=${URL:-https://127.0.0.1:8443/v1}
read -r -s -p 'HindSight bearer token: ' TOKEN
printf '\n'
[[ -n "$TOKEN" ]] || { echo 'A non-empty HindSight token is required.' >&2; exit 1; }
codex mcp remove hindsight >/dev/null 2>&1 || true
codex mcp add hindsight --env "HINDSIGHT_URL=$URL" --env "HINDSIGHT_TOKEN=$TOKEN" -- node "$SERVER"
unset TOKEN
echo 'HindSight MCP registered with Codex. Restart Codex to load the tools.'
