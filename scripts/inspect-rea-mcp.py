#!/usr/bin/env python3
"""Inspect the installed REA MCP identity and the small tool surface HindSight uses."""
import json
import subprocess
import sys

command = sys.argv[1:] or ["rea-agents", "mcp"]
process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                           stderr=subprocess.PIPE, text=True, bufsize=1)

def send(message):
    process.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
    process.stdin.flush()

def response(request_id):
    for line in process.stdout:
        message = json.loads(line)
        if message.get("id") == request_id:
            return message
    raise RuntimeError("REA MCP server closed before replying")

try:
    send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "hindsight-qualification", "version": "0.1"}}})
    initialized = response(1)
    send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    send({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    listed = response(2)
    tools = listed.get("result", {}).get("tools", [])
    wanted = {"binary_session", "open_binary", "binary_overview", "list_procedures", "list_names",
              "inspect_binary_layout", "inspect_artifact", "inspect_macho", "close_binary"}
    selected = [{"name": tool.get("name"), "inputSchema": tool.get("inputSchema", {})}
                for tool in tools if tool.get("name") in wanted]
    print(json.dumps({"serverInfo": initialized.get("result", {}).get("serverInfo"),
                      "protocolVersion": initialized.get("result", {}).get("protocolVersion"),
                      "toolCount": len(tools), "selectedTools": selected}, indent=2, sort_keys=True))
finally:
    if process.stdin:
        process.stdin.close()
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        process.terminate()
        process.wait(timeout=2)
