#!/usr/bin/env python3
"""Bounded, read-only ME7 checksum qualification against the production worker."""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import pathlib
import re
import struct
import subprocess
import sys
import uuid

TABLE = 0x1F810
ENTRY_COUNT = 31
ENTRY_SIZE = 16
MAX_FILES = 256
MAX_IMAGE = 2 * 1024 * 1024
SUMMARY = re.compile(
    r"ecu checksum table: (?P<verified>\d+) verified, (?P<invalid>\d+) invalid, "
    r"at fixed ME7-class offset 0x1f810\."
)
SIGNATURE_SUMMARY = re.compile(r"ecu checksum code signatures: (?P<count>\d+) exact multipoint match")
MULTIPOINT_SUM = bytes.fromhex("98 24 a8 34 00 a2 10 b3 26 fa ff ff 36 fb ff ff 3d 08")


def static_verdict(data: bytes) -> dict[str, object]:
    if len(data) > MAX_IMAGE:
        raise ValueError("image exceeds 2 MiB qualification bound")
    if TABLE + ENTRY_COUNT * ENTRY_SIZE > len(data):
        return {"present": False, "verified": 0, "invalid": 0, "entries": []}
    verified = invalid = 0
    entries: list[dict[str, object]] = []
    for index in range(ENTRY_COUNT):
        offset = TABLE + index * ENTRY_SIZE
        start, end, stored, complement = struct.unpack_from("<IIII", data, offset)
        if start == 0 and end == 0:
            continue
        reason = "verified"
        computed: int | None = None
        if stored ^ 0xFFFFFFFF != complement:
            reason = "invalid-complement"
        elif start > end or end >= len(data) or (end - start + 1) % 2:
            reason = "invalid-range"
        else:
            computed = 0
            for at in range(start, end, 2):
                computed = (computed + int.from_bytes(data[at : at + 2], "little")) & 0xFFFFFFFF
            if computed != stored:
                reason = "invalid-sum"
        if reason == "verified":
            verified += 1
        else:
            invalid += 1
        entries.append(
            {"index": index, "start": start, "end": end, "stored": stored,
             "computed": computed, "status": reason}
        )
    return {"present": True, "verified": verified, "invalid": invalid, "entries": entries}


def code_signatures(data: bytes) -> list[dict[str, int | None]]:
    result = []
    cursor = 0
    while len(result) < 8:
        at = data.find(MULTIPOINT_SUM, cursor)
        if at < 0:
            break
        floor = max(0, at - 0x400)
        entry = next(
            (candidate + 2 for candidate in range(at - 2, floor - 1, -2)
             if data[candidate : candidate + 2] == b"\xDB\x00"),
            None,
        )
        result.append({"offset": at, "entry_candidate": entry})
        cursor = at + 1
    return result


def worker_verdict(worker: pathlib.Path, path: pathlib.Path, data: bytes) -> dict[str, int]:
    request_id = str(uuid.uuid4())
    command = [
        str(worker), "--analyze-image", str(path), hashlib.sha256(data).hexdigest(),
        str(len(data)), request_id, "ecu", "c167", "0", "0",
    ]
    result = subprocess.run(command, capture_output=True, timeout=120, check=True)
    if len(result.stdout) < 4:
        raise RuntimeError("worker returned a truncated frame")
    length = struct.unpack(">I", result.stdout[:4])[0]
    if len(result.stdout) != length + 4:
        raise RuntimeError("worker returned a malformed frame")
    reply = json.loads(result.stdout[4:])
    if reply.get("kind") != "analysis_result" or reply.get("request_id") != request_id:
        raise RuntimeError(f"worker analysis failed: {reply}")
    match = SUMMARY.search(reply.get("metadata", ""))
    signature_match = SIGNATURE_SUMMARY.search(reply.get("metadata", ""))
    if not match or not signature_match:
        raise RuntimeError("worker checksum evidence summary is absent")
    return {"verified": int(match.group("verified")), "invalid": int(match.group("invalid")),
            "signatures": int(signature_match.group("count"))}


def images(root: pathlib.Path) -> list[pathlib.Path]:
    found = sorted({*root.rglob("*.bin"), *root.rglob("*.Bin")})
    if len(found) > MAX_FILES:
        raise ValueError(f"corpus has {len(found)} files; bound is {MAX_FILES}")
    return [path for path in found if path.is_file()]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpus", type=pathlib.Path)
    parser.add_argument("--worker", type=pathlib.Path, help="compare every verdict with reworker")
    parser.add_argument("--output", type=pathlib.Path, help="write the deterministic JSON report")
    parser.add_argument("--jobs", type=int, default=4, help="parallel worker analyses (1-8; default 4)")
    args = parser.parse_args()
    corpus = args.corpus.resolve()
    worker = args.worker.resolve() if args.worker else None
    if not corpus.is_dir() or (worker and not worker.is_file()) or not 1 <= args.jobs <= 8:
        parser.error("corpus directory and optional worker must exist")

    paths = images(corpus)

    def qualify(path: pathlib.Path) -> dict[str, object]:
        data = path.read_bytes()
        static = static_verdict(data)
        record: dict[str, object] = {
            "path": str(path.relative_to(corpus)), "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(), "static": static,
            "code_signatures": code_signatures(data),
        }
        if worker:
            observed = worker_verdict(worker, path, data)
            record["worker"] = observed
        return record

    if worker:
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
            records = []
            for index, record in enumerate(pool.map(qualify, paths), 1):
                records.append(record)
                print(f"[{index}/{len(paths)}] {record['path']}", file=sys.stderr, flush=True)
    else:
        records = [qualify(path) for path in paths]
    mismatches = [
        record["path"] for record in records
        if worker and record["worker"] != {
            "verified": record["static"]["verified"], "invalid": record["static"]["invalid"],
            "signatures": len(record["code_signatures"]),
        }
    ]

    valid = sum(bool(r["static"]["present"]) and r["static"]["invalid"] == 0 for r in records)
    report = {
        "schema": 1, "corpus": str(corpus), "files": len(records),
        "valid_checksum_tables": valid, "invalid_checksum_tables": len(records) - valid,
        "images_with_checksum_code_signature": sum(bool(r["code_signatures"]) for r in records),
        "worker_compared": worker is not None, "mismatches": mismatches, "records": records,
    }
    encoded = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(encoded)
    else:
        sys.stdout.write(encoded)
    return 1 if mismatches or not records else 0


if __name__ == "__main__":
    raise SystemExit(main())
