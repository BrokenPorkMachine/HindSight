#!/usr/bin/env python3
"""Bounded ECU emulator qualification against a corpus of ME7-class images.

This does not claim full checksum-routine semantic equivalence without
documented runtime state for each routine. It reports, for images with
identified multipoint checksum code signatures, whether the emulator can
execute from the reported entry candidate without hitting an unsupported
instruction within a bounded budget, and records stop reasons/evidence.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import json
import pathlib
import struct
import subprocess
import sys
import uuid

MAX_FILES = 256


def analyze(worker: pathlib.Path, path: pathlib.Path) -> dict:
    import hashlib
    data = path.read_bytes()
    request_id = str(uuid.uuid4())
    cmd = [str(worker), "--analyze-image", str(path), hashlib.sha256(data).hexdigest(),
           str(len(data)), request_id, "ecu", "c167", "0", "0"]
    res = subprocess.run(cmd, capture_output=True, timeout=120, check=True)
    out = res.stdout
    idx = out.rfind(b'{"protocol":')
    if idx < 4:
        raise RuntimeError(f"no JSON result for {path}")
    n = struct.unpack(">I", out[idx-4:idx])[0]
    return json.loads(out[idx:idx+n].decode("utf-8", "replace"))


def emulate(worker: pathlib.Path, path: pathlib.Path, entry: int, steps: int = 20000) -> dict:
    request_id = str(uuid.uuid4())
    cmd = [str(worker), "--emulate-c167", str(path), hex(entry), str(steps), "0xfc00",
           "none", "none", "none", request_id]
    res = subprocess.run(cmd, capture_output=True, timeout=120)
    if res.returncode != 0:
        err = res.stderr.decode("utf-8", "replace").strip() or res.stdout.decode("utf-8", "replace").strip()
        return {"error": err, "request_id": request_id}
    out = res.stdout
    idx = out.rfind(b'{"protocol":')
    if idx < 4:
        return {"error": "no JSON result", "raw": out[:200].decode("utf-8", "replace")}
    n = struct.unpack(">I", out[idx-4:idx])[0]
    try:
        return json.loads(out[idx:idx+n].decode("utf-8", "replace"))
    except Exception:
        return json.loads(out[idx:idx+n].decode("utf-8", "replace"))


def images(root: pathlib.Path) -> list[pathlib.Path]:
    found = sorted({*root.rglob("*.bin"), *root.rglob("*.Bin"), *root.rglob("*.BIN")})
    if len(found) > MAX_FILES:
        found = found[:MAX_FILES]
    return [p for p in found if p.is_file()]


def qualify_one(worker: pathlib.Path, path: pathlib.Path, corpus: pathlib.Path) -> dict:
    rec = {"path": str(path.relative_to(corpus))}
    try:
        a = analyze(worker, path)
    except Exception as e:
        rec["analysis_error"] = str(e)
        return rec
    meta = a.get("metadata", "")
    # parse signature/entry candidate
    entry = None
    sig_off = None
    for line in meta.splitlines():
        if "multipoint checksum code signature: exact fixed-byte match; containing-function entry candidate" in line:
            # extract 0x... at end
            parts = line.rsplit(" ", 1)
            try:
                entry = int(parts[-1].rstrip("."), 16)
            except Exception:
                pass
            # signature offset from leading address
            try:
                sig_off = int(line.split()[0], 16)
            except Exception:
                pass
    verified = "checksum table:" in meta and " 0 invalid" in meta and "verified" in meta
    rec["checksum_table_verified"] = verified
    rec["signature_offset"] = sig_off
    rec["entry_candidate"] = entry
    if entry is None:
        rec["emulation"] = None
        return rec
    try:
        e = emulate(worker, path, entry)
    except Exception as ex:
        rec["emulation_error"] = str(ex)
        return rec
    rec["emulation"] = {
        "stop": e.get("stop"),
        "pc": e.get("pc"),
        "steps": e.get("steps"),
        "error": e.get("error"),
    }
    # keep unsupported details if present
    stop = str(e.get("stop", ""))
    if "Unsupported" in stop:
        rec["emulation"]["unsupported"] = stop
    return rec


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("corpus", type=pathlib.Path)
    ap.add_argument("--worker", type=pathlib.Path, required=True)
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--output", type=pathlib.Path)
    ap.add_argument("--limit", type=int, default=0)
    args = ap.parse_args()
    corpus = args.corpus.resolve()
    worker = args.worker.resolve()
    paths = images(corpus)
    if args.limit > 0:
        paths = paths[:args.limit]
    records = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, min(8, args.jobs))) as pool:
        futs = {pool.submit(qualify_one, worker, p, corpus): p for p in paths}
        for i, fut in enumerate(concurrent.futures.as_completed(futs), 1):
            rec = fut.result()
            records.append(rec)
            print(f"[{i}/{len(paths)}] {rec['path']}", file=sys.stderr, flush=True)
    records.sort(key=lambda r: r["path"])
    with_entry = [r for r in records if r.get("entry_candidate") is not None]
    stops = {}
    for r in with_entry:
        s = ((r.get("emulation") or {}).get("stop") or "none")
        # normalize
        key = s.split("{")[0].split("(")[0]
        stops[key] = stops.get(key, 0) + 1
    report = {
        "schema": 1, "files": len(records), "with_entry_candidate": len(with_entry),
        "stop_summary": stops, "records": records,
    }
    out = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(out)
    else:
        sys.stdout.write(out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
