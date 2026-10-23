#!/usr/bin/env python3
"""Check a real worker analysis against native protocol section bounds."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("analysis_requests", ROOT / "scripts/test-worker-reconstruction.py")
requests = importlib.util.module_from_spec(spec)
spec.loader.exec_module(requests)


def run(worker, binary):
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    report = requests.request(worker, binary, "--analyze", digest=digest)
    sizes = {key: len(value.encode("utf-8")) for key, value in report.items() if isinstance(value, str)}
    for key, size in sizes.items():
        limit = 32768 if key == "summary" else 65536
        if size >= limit:
            raise ValueError(f"{key} exceeds native section bound: {size} >= {limit}")
    with tempfile.TemporaryDirectory(prefix="hindsight-analysis-bounds-") as temporary:
        root = Path(temporary)
        artifact = root / "analysis.json"
        artifact.write_text(json.dumps(report, ensure_ascii=False))
        executable = root / "native-contract"
        cache = root / "module-cache"
        cache.mkdir()
        environment = dict(os.environ, CLANG_MODULE_CACHE_PATH=str(cache))
        subprocess.run(["xcrun", "swiftc", str(ROOT / "Shared/WorkbenchMessages.swift"),
                        str(ROOT / "Tests/analysis_protocol_contract.swift"), "-module-cache-path", str(cache),
                        "-o", str(executable)], check=True, env=environment, timeout=120)
        subprocess.run([str(executable), str(artifact), digest], check=True, timeout=30)
    print(json.dumps({"status": "passed", "prototype_conflicts_bytes": sizes["prototype_conflicts"],
                      "largest_section_bytes": max(sizes.values()),
                      "prototype_report_truncated": "Report truncated:" in report["prototype_conflicts"]}))


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: test-analysis-section-bounds.py WORKER BINARY")
    run(Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve())
