#!/usr/bin/env python3
"""Run bounded firmware-container qualification against user-supplied fixtures."""
import argparse
import hashlib
import json
import pathlib
import struct
import subprocess
import tempfile
import uuid


def invoke(worker: pathlib.Path, source: pathlib.Path, destination: pathlib.Path) -> dict:
    digest = hashlib.sha256(source.read_bytes()).hexdigest()
    identity = str(uuid.uuid4())
    result = subprocess.run(
        [
            str(worker),
            "--capture-image",
            str(source),
            str(destination),
            identity,
            "auto",
            "auto",
            "0",
            "0",
        ],
        capture_output=True,
        timeout=300,
        check=False,
    )
    if result.returncode:
        return {"status": "error", "stderr": result.stderr.decode(errors="replace")}
    if len(result.stdout) < 4:
        return {"status": "error", "stderr": "worker returned no framed response"}
    length = struct.unpack(">I", result.stdout[:4])[0]
    if length != len(result.stdout) - 4:
        return {"status": "error", "stderr": "invalid worker frame length"}
    reply = json.loads(result.stdout[4:])
    if reply.get("kind") == "error":
        return {"status": "error", "stderr": reply.get("message", "worker error")}
    if reply.get("source_sha256") != digest:
        return {"status": "error", "stderr": "source SHA-256 was not preserved"}
    return {
        "status": "passed",
        "file_kind": reply.get("file_kind"),
        "architecture": reply.get("architecture"),
        "structure_status": reply.get("structure_status"),
        "structure": reply.get("structure", ""),
        "disassembly_status": reply.get("disassembly_status"),
        "source_sha256": digest,
        "size": source.stat().st_size,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("worker", type=pathlib.Path)
    parser.add_argument("fixtures", type=pathlib.Path)
    parser.add_argument("--report", type=pathlib.Path)
    args = parser.parse_args()
    worker = args.worker.resolve()
    fixture_dir = args.fixtures.resolve()
    if not worker.is_file() or not fixture_dir.is_dir():
        parser.error("worker must be a file and fixtures must be a directory")
    fixtures = sorted(path for path in fixture_dir.iterdir() if path.is_file())
    if not fixtures:
        parser.error("fixtures directory is empty")
    records = []
    with tempfile.TemporaryDirectory(prefix="hindsight-firmware-qualification-") as temporary:
        for source in fixtures:
            record = {"fixture": str(source), **invoke(worker, source, pathlib.Path(temporary) / source.name)}
            records.append(record)
    report = {
        "schema": 1,
        "worker": str(worker),
        "fixture_count": len(records),
        "passed": sum(record["status"] == "passed" for record in records),
        "failed": sum(record["status"] != "passed" for record in records),
        "records": records,
    }
    encoded = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(encoded)
    print(encoded, end="")
    return 0 if report["failed"] == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
