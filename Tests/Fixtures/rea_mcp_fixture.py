#!/usr/bin/env python3
"""Deterministic line-delimited MCP fixture for the HindSight REA bridge contract."""
import json
import sys
import time

mode = sys.argv[1] if len(sys.argv) > 1 else "allow"

for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    request_id = request.get("id")
    if method == "initialize":
        reply = {"jsonrpc": "2.0", "id": request_id, "result": {"protocolVersion": "2025-06-18"}}
    elif method == "tools/call" and request.get("params", {}).get("name") == "binary_session":
        available = mode != "deny"
        reply = {
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {"structuredContent": {"tool_availability": {
                "binary_overview": {"available": available, "reason": "fixture says unavailable" if not available else ""},
                "list_procedures": {"available": True},
                "list_names": {"available": True},
            }}}
        }
    elif method == "tools/call" and request.get("params", {}).get("name") == "open_binary":
        reply = {"jsonrpc": "2.0", "id": request_id, "result": {"opened": True}}
    elif method == "tools/call":
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/progress",
                                     "params": {"progress": 1, "message": "fixture progress"}}) + "\n")
        sys.stdout.flush()
        time.sleep(0.01)
        reply = {"jsonrpc": "2.0", "id": request_id, "result": {"fixture": True}}
    else:
        continue
    sys.stdout.write(json.dumps(reply) + "\n")
    sys.stdout.flush()
