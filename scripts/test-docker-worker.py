#!/usr/bin/env python3
"""Exercise the packaged Linux runtime, readonly mounts and source identity.

Requires an already built trusted image and a local Linux Docker daemon.
All binary contents are parsed/interpreted, never launched as native code.
"""
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "python/tests"))
sys.path.insert(0, str(ROOT / "scripts"))
from test_decoder import image
from reconstruction_artifact import ReconstructionArtifact

IMAGE = sys.argv[1] if len(sys.argv) > 1 else "hindsight-worker:local"


def invoke(*args, success=True):
    process = subprocess.run([sys.executable, str(ROOT / "scripts/docker-worker.py"),
                              *map(str, args), "--image", IMAGE],
                             capture_output=True, timeout=210)
    assert (process.returncode == 0) == success, process.stderr.decode(errors="replace")
    return json.loads(process.stdout)


assert invoke("health")["worker"] is True
with tempfile.TemporaryDirectory(prefix="hindsight-docker-") as directory:
    source = Path(directory) / "private fixture.macho"
    # MOV x0,x1; RET: supported scalar subset, no unknown flags or external calls.
    data = image(words=(0xaa0103e0, 0xd65f03c0))
    source.write_bytes(data)
    source.chmod(0o600)
    digest = hashlib.sha256(data).hexdigest()
    report = invoke("analyze", source)
    assert report["source_sha256"] == digest
    assert report["kind"] == "analysis_result" and report["analyzed_instructions"] == 2
    page = invoke("page", source)
    assert page["word_count"] == 2 and "ret" in page["text"]
    match = re.search(r"^(0x[0-9a-f]+)\s+(0x[0-9a-f]+)\s+\d+\s+bytes", report["functions"], re.M)
    assert match, report["functions"]
    bounds = ("--start", match[1], "--end", match[2])
    document = invoke("reconstruct", source, *bounds)
    validated = ReconstructionArtifact(document)
    assert document["eligibility"]["state"] == "review-only"
    assert not document["ast"]["compilable"]
    interpreted = invoke("interpret", source, *bounds, "--inputs", "0,42,0,0,0,0,0,0")
    ReconstructionArtifact(interpreted)
    assert interpreted["source_sha256"] == digest
    # Framing must survive Docker unchanged (no TTY/progress on stdout).
    process = subprocess.run([sys.executable, str(ROOT / "scripts/docker-worker.py"),
                              "health", "--image", IMAGE, "--framed"],
                             capture_output=True, check=True, timeout=210)
    assert struct.unpack_from(">I", process.stdout)[0] == len(process.stdout)-4
    assert json.loads(process.stdout[4:])["kind"] == "health"
    assert source.read_bytes() == data
    source.write_bytes(b"not an executable")
    assert invoke("analyze", source, success=False)["kind"] == "error"
print("Docker Linux runtime: health, private read-only input, analysis, paging, reconstruction, interpretation, framing and rejection passed")
