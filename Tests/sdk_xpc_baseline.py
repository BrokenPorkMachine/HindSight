#!/usr/bin/env python3
"""Control: the fixture's SDK call signature is absent without a catalog."""

import hashlib
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import uuid


def worker_request(worker, arguments):
    environment = dict(os.environ)
    environment.pop("HINDSIGHT_PROTOTYPE_DB", None)
    result = subprocess.run([worker, *arguments], env=environment, capture_output=True, check=True, timeout=120)
    if len(result.stdout) < 4:
        raise AssertionError("worker control returned no frame")
    size = struct.unpack(">I", result.stdout[:4])[0]
    if size > 4 * 1024 * 1024 or len(result.stdout) != size + 4:
        raise AssertionError("worker control returned an invalid frame")
    return json.loads(result.stdout[4:])


def main(worker, binary):
    source = Path(binary).read_bytes()
    digest = hashlib.sha256(source).hexdigest()
    common = [binary, digest, str(len(source))]
    analysis = worker_request(worker, ["--analyze", *common, str(uuid.uuid4())])
    boundary = next(re.match(r"^(0x[0-9a-f]+)\s+(0x[0-9a-f]+)\s+.*_HSWritePrefix\s", line)
                    for line in analysis["functions"].splitlines()
                    if re.match(r"^0x[0-9a-f]+\s+0x[0-9a-f]+\s+.*_HSWritePrefix\s", line))
    artifact = worker_request(worker, ["--reconstruct", *common, boundary[1], boundary[2], str(uuid.uuid4())])
    candidates = [candidate for node in artifact["nodes"]
                  for candidate in node.get("payload", {}).get("analysis_settings", {}).get("call_signature_candidates", [])]
    if any(candidate.get("symbol") == "_fwrite" for candidate in candidates):
        raise AssertionError("uncataloged worker acquired an SDK fwrite signature")
    print("No-catalog worker control retained an unknown fwrite signature.")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: sdk_xpc_baseline.py /absolute/reworker /absolute/binary")
    main(*sys.argv[1:])
