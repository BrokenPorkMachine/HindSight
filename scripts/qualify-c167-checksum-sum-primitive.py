#!/usr/bin/env python3
"""Evidence-based descriptor-sum emulation for the recurring ME7 C167 sum primitive.

Targets corpus images containing the exact primitive pattern:
  f2f1d0a9 f2f2d2a9 f2f3d6a9 f2f4d8a9 f2f5dea9 (mov r1-r5 from [0xa9d0..0xa9de])
This primitive computes a 32-bit little-endian 16-bit word sum over a parameterized
range and returns via RETS. Params are seeded per descriptor entry (start/end/len)
using evidence-derived address/count mapping; results are compared to static
descriptor sums. This is a bounded, partial corpus-own-routine qualification.
"""
from __future__ import annotations
import json, pathlib, struct, subprocess, sys, uuid

TABLE = 0x1F810
PATTERN = bytes.fromhex("f2f1d0a9f2f2d2a9f2f3d6a9f2f4d8a9f2f5dea9")

def w(v): return struct.pack("<H", v & 0xffff)

def run(worker, path, entry, start, end):
    count_pairs = (end - start + 1) // 4
    ram = [
        (0xa9d0, (w(0) + w(0)).hex()),
        (0xa9d6, (w(start & 0xffff) + w((start >> 16) & 0xffff)).hex()),
        (0xa9de, w(count_pairs).hex()),
        (0xfbfe, w(0).hex()),
    ]
    ram_arg = ",".join(f"0x{a:x}={b}" for a, b in ram)
    cmd = [str(worker), "--emulate-c167", str(path), hex(entry), "1000000",
           "0xfc00", hex(entry + 0x58), "none", ram_arg, str(uuid.uuid4())]
    p = subprocess.run(cmd, capture_output=True)
    out = p.stdout
    idx = out.rfind(b'{"protocol":')
    if idx < 4: return {"error": "no_json", "stderr": p.stderr.decode("utf-8","replace")[:200]}
    n = struct.unpack(">I", out[idx-4:idx])[0]
    obj = json.loads(out[idx:idx+n].decode("utf-8", "replace"))
    stop = obj.get("stop")
    if "Breakpoint" not in str(stop):
        return {"stop": stop, "steps": obj.get("steps")}
    r1 = obj["registers"]["r1"]; r2 = obj["registers"]["r2"]
    return {"ok": True, "sum": (r2 << 16) | r1, "steps": obj.get("steps"), "stop": stop}

def main():
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument("corpus", type=pathlib.Path)
    ap.add_argument("--worker", type=pathlib.Path, required=True)
    ap.add_argument("--output", type=pathlib.Path)
    args = ap.parse_args()
    corpus = args.corpus.resolve(); worker = args.worker.resolve()
    paths = sorted({*corpus.rglob("*.bin"), *corpus.rglob("*.Bin"), *corpus.rglob("*.BIN")})
    records = []
    for path in paths:
        data = path.read_bytes()
        off = data.find(PATTERN)
        if off < 0: continue
        rel = str(path.relative_to(corpus))
        rec = {"path": rel, "primitive": hex(off), "entries": [], "ok": 0, "bad": 0, "skip": 0}
        for i in range(31):
            o = TABLE + i * 16
            if o + 16 > len(data): rec["skip"] += 1; continue
            start,end,stored,comp = struct.unpack_from("<IIII", data, o)
            if start==0 and end==0 and stored==0 and comp==0: rec["skip"] += 1; continue
            if end < start or end >= len(data) or ((end-start+1) & 1): rec["skip"] += 1; continue
            expected = sum(struct.unpack_from("<H", data, a)[0] for a in range(start, end+1, 2)) & 0xffffffff
            if expected != stored: rec["skip"] += 1; continue
            res = run(worker, path, off, start, end)
            if res.get("ok") and res["sum"] == expected:
                rec["ok"] += 1
                rec["entries"].append({"index": i, "start": start, "end": end, "sum": expected, "match": True, "steps": res["steps"]})
            else:
                rec["bad"] += 1
                rec["entries"].append({"index": i, "start": start, "end": end, "expected": expected, "emulated": res.get("sum"), "match": False, "detail": res})
        records.append(rec)
        print(f"{rel}: ok={rec['ok']} bad={rec['bad']} skip={rec['skip']}", file=sys.stderr)
    report = {"schema": 1, "images": len(records),
              "total_ok": sum(r["ok"] for r in records),
              "total_bad": sum(r["bad"] for r in records),
              "total_skip": sum(r["skip"] for r in records),
              "records": records}
    out = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output: args.output.write_text(out)
    else: sys.stdout.write(out)
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
