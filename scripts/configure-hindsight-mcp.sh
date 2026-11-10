#!/usr/bin/env bash
# Register this checkout's HindSight MCP server with a local coding agent.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/configure-hindsight-mcp.sh --url URL --token TOKEN [options]

Options:
  --client NAME   codex (default), claude, or print
  --ca-file PATH  PEM certificate authority file for a private gateway certificate
  --url URL       HindSight gateway URL, including /v1
  --token TOKEN   Bearer token shown by HindSight's HTTPS gateway

Start the gateway and issue/grant the identity inside HindSight first. This
script only registers the MCP server; it cannot create a HindSight grant.
EOF
}

client=codex
url=${HINDSIGHT_URL:-}
token=${HINDSIGHT_TOKEN:-}
ca_file=${HINDSIGHT_CA_FILE:-}
while (($#)); do
  case "$1" in
    --client) client=${2:?missing client}; shift 2 ;;
    --url) url=${2:?missing URL}; shift 2 ;;
    --token) token=${2:?missing token}; shift 2 ;;
    --ca-file) ca_file=${2:?missing CA file}; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
done
[[ -n "$url" && -n "$token" ]] || { echo '--url and --token are required' >&2; usage >&2; exit 2; }

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
server="$root/mcp/hindsight-mcp.mjs"
[[ -f "$server" ]] || { echo "MCP server not found: $server" >&2; exit 1; }
command=(env "HINDSIGHT_URL=$url" "HINDSIGHT_TOKEN=$token")
[[ -n "$ca_file" ]] && command+=("HINDSIGHT_CA_FILE=$ca_file")
command+=(node "$server")

case "$client" in
  codex)
    command -v codex >/dev/null || { echo 'The codex CLI is not on PATH.' >&2; exit 1; }
    codex mcp remove hindsight >/dev/null 2>&1 || true
    codex mcp add hindsight -- "${command[@]}"
    ;;
  claude)
    command -v claude >/dev/null || { echo 'The claude CLI is not on PATH.' >&2; exit 1; }
    claude mcp remove hindsight >/dev/null 2>&1 || true
    claude mcp add --transport stdio hindsight -- "${command[@]}"
    ;;
  print)
    printf '%q ' "${command[@]}"; printf '\n'
    ;;
  *) echo "Unsupported client: $client (use codex, claude, or print)" >&2; exit 2 ;;
esac
echo "Configured HindSight MCP for $client. Restart the agent session if it is already running."
