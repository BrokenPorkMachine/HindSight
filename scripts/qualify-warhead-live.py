#!/usr/bin/env python3
"""Run one bounded model-backed Warhead Studio turn with REA-shaped evidence."""
import hashlib
import json
import pathlib
import subprocess
import sys

root = pathlib.Path(__file__).resolve().parents[1]
entry = root / "BundledPlugins/Warhead.vgplugin/runtime/cli/extension.js"
model = sys.argv[1] if len(sys.argv) > 1 else "llama3.2:1b"
response = json.dumps({"ok": True, "response": {"result": {
    "evidence_id": "ev_" + "1" * 64,
    "provider": {"id": "hopper", "name": "Hopper", "version": "qualified"},
    "normalized_result": {"summary": "Qualification fixture: one small executable."}}}}, sort_keys=True)
request = {
    "host": "http://127.0.0.1:11434", "model": model,
    "prompt": "Summarize the supplied REA evidence in one sentence and include its provider ID.",
    "snapshot": {
        "selection": {"workspace": "REA Agent", "symbol": "_main"},
        "analysis": {"functions": "0x1000 _main"},
        "rea_evidence": [{"id": "qualification", "operation": "binary_overview",
                          "provider": "hopper", "response_sha256": hashlib.sha256(response.encode()).hexdigest(),
                          "response": response}],
    },
}
completed = subprocess.run(["node", str(entry)], input=json.dumps(request), text=True,
                           capture_output=True, timeout=30, check=False)
try:
    reply = json.loads(completed.stdout)
except json.JSONDecodeError as error:
    raise SystemExit(f"Warhead returned invalid JSON: {error}: stdout={completed.stdout[:1000]!r} stderr={completed.stderr[:3000]!r}")
if completed.returncode or not reply.get("ok") or not str(reply.get("text", "")).strip():
    raise SystemExit("Warhead live qualification failed: " + json.dumps(reply, sort_keys=True)[:4000])
print(f"Warhead live qualification passed • model {model} • response {len(reply['text'].encode())} bytes")
print(reply["text"].strip()[:1000])
