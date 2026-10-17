#!/usr/bin/env python3
import json, pathlib, struct, subprocess, sys
if len(sys.argv) != 2:
    raise SystemExit("usage: test-embedded-worker.py /path/to/reworker")
worker = pathlib.Path(sys.argv[1])
if not worker.is_file():
    raise SystemExit(f"embedded worker missing: {worker}")
proc = subprocess.run([str(worker), "--self-test"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
if proc.returncode:
    raise SystemExit(f"worker self-test exited {proc.returncode}: {proc.stderr.decode(errors='replace')}")
if len(proc.stdout) < 4:
    raise SystemExit("worker self-test returned a truncated frame")
(length,) = struct.unpack(">I", proc.stdout[:4])
payload = proc.stdout[4:]
if len(payload) != length:
    raise SystemExit(f"worker frame length mismatch: header={length}, payload={len(payload)}")
obj = json.loads(payload)
expected = {"protocol": 1, "kind": "health", "version": "0.5.12", "worker": True}
if obj != expected:
    raise SystemExit(f"unexpected worker health payload: {obj!r}")
print("Embedded worker self-test passed:", obj)
