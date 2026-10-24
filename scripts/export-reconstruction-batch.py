#!/usr/bin/env python3
"""Dump one validated reconstruction representation without executing code."""
import argparse
import hashlib
import json
from pathlib import Path

from reconstruction_batch import read_batch_manifest, verified_outputs

FORMATS = {"c": ("recovered-source", ".c", ".c"),
           "objc": ("recovered-source", ".c", ".m"),
           "header": ("recovered-header", ".h", ".h"),
           "native": ("machine-state-c", ".machine.c", ".n")}


def run(batch, output, format_name, start=None, end=None, address=None, maximum_bytes=256 * 1024 * 1024):
    if format_name not in FORMATS or not 1 <= maximum_bytes <= 256 * 1024 * 1024:
        raise ValueError("invalid export format or byte budget")
    if ((start is None) != (end is None) or
            (start is not None and not 0 <= start < end <= 2**64 - 1) or
            (address is not None and (start is not None or not 0 <= address < 2**64))):
        raise ValueError("choose a valid range or one address, not both")
    manifest, manifest_bytes = read_batch_manifest(batch)
    verified = verified_outputs(batch, manifest)
    selected = [(entry, files) for entry, files in verified
                if (start is None or start <= int(entry["start"], 16) < int(entry["end"], 16) <= end)
                and (address is None or int(entry["start"], 16) <= address < int(entry["end"], 16))]
    if not selected or (address is not None and len(selected) != 1):
        raise ValueError("scope has no functions or address has competing function bounds")
    kind, input_suffix, output_suffix = FORMATS[format_name]
    planned = {}
    entries = []
    total = 0
    for entry, files in selected:
        prefix = f"function-{int(entry['start'], 16):016x}"
        item = {"start": entry["start"], "end": entry["end"], "state": entry["state"],
                "recovery_state": entry["state"],
                "available": False, "representation": "unavailable", "languages": [], "files": [], "blockers": entry.get("source_blockers", [])}
        # Retain the complete validated derivation graph even when this
        # particular representation is unavailable. Never fabricate a body.
        artifact_name = prefix + ".reconstruction.json"
        outputs = []
        if artifact_name in files:
            outputs.append((artifact_name, files[artifact_name]))
            artifact = json.loads(files[artifact_name])
            if format_name == "header":
                header_payloads = [node["payload"] for node in artifact["nodes"]
                                   if node["payload"].get("artifact_kind") in {"recovered-header", "best-effort-header"}]
                item["languages"] = header_payloads[0].get("languages", []) if header_payloads else []
            candidates = [node["payload"] for node in artifact["nodes"] if node["payload"].get("artifact_kind") == kind]
            if format_name == "objc":
                objc = [node["payload"] for node in artifact["nodes"] if node["payload"].get("artifact_kind") == "recovered-objective-c"]
                if objc:
                    candidates = objc
            if not candidates and format_name in {"c", "objc"}:
                candidates = [node["payload"] for node in artifact["nodes"] if node["payload"].get("artifact_kind") == "machine-state-c"]
            if not candidates:
                candidates = [node["payload"] for node in artifact["nodes"] if node["payload"].get("artifact_kind") ==
                              ("best-effort-header" if format_name == "header" else "best-effort-source")]
            item["representation"] = candidates[0]["artifact_kind"] if candidates else "unavailable"
        source_name = prefix + input_suffix
        if item.get("representation") == "recovered-objective-c":
            source_name = prefix + ".m"
        if item.get("representation") == "machine-state-c":
            source_name = prefix + ".machine.c"
        if entry["state"] != "failed" and source_name in files:
            outputs.append((prefix + output_suffix, files[source_name]))
            item["available"] = True
        elif entry["state"] != "failed" and format_name == "native" and prefix + ".c" in files and item.get("representation") == "best-effort-source":
            outputs.append((prefix + output_suffix, files[prefix + ".c"]))
            item["available"] = True
        for name, raw in outputs:
            if name in planned:
                raise ValueError("duplicate export output")
            total += len(raw)
            if total > maximum_bytes:
                raise ValueError("export byte budget exhausted before publication")
            planned[name] = raw
            item["files"].append({"path": name, "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()})
        entries.append(item)
    report = {"schema": 1, "kind": "reconstruction-format-export", "format": format_name, "artifact_kind": kind,
              "source_sha256": manifest["source_sha256"],
              "worker_sha256": manifest.get("worker_sha256"),
              "prototypes_sha256": manifest.get("prototypes_sha256"),
              "source_bytes": manifest.get("source_bytes"),
              "batch_scope": manifest.get("scope"),
              "source_batch_manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest(),
              **({"sdk_catalog_sha256": manifest["sdk_catalog_sha256"]} if "sdk_catalog_sha256" in manifest else {}),
              "scope": "address" if address is not None else "range" if start is not None else "retained-batch",
              "requested_start": start, "requested_end": end, "requested_address": address,
              "coverage": "validated retained batch only; undiscovered and omitted functions remain unknown",
              "claim": "exact artifact representation export; no compilation, execution or equivalence claim",
              "available": sum(item["available"] for item in entries), "unavailable": sum(not item["available"] for item in entries),
              "best_effort": sum(item["available"] and item.get("representation", "").startswith("best-effort-") for item in entries),
              "output_bytes": total, "functions": entries}
    # All validation and byte-budget planning precede any destination writes.
    # A pre-existing directory is never reused, merged or overwritten.
    output.mkdir(exist_ok=False)
    for name, raw in planned.items():
        with (output / name).open("xb") as destination:
            destination.write(raw)
    with (output / "export-manifest.json").open("x") as destination:
        json.dump(report, destination, sort_keys=True, indent=2)
    print(json.dumps({key: report[key] for key in ["format", "available", "unavailable", "output_bytes"]}))
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("batch", type=Path)
    parser.add_argument("output", type=Path, help="new destination directory; existing paths are refused")
    parser.add_argument("--format", choices=FORMATS, required=True)
    parser.add_argument("--start", type=lambda value: int(value, 0))
    parser.add_argument("--end", type=lambda value: int(value, 0))
    parser.add_argument("--address", type=lambda value: int(value, 0))
    parser.add_argument("--max-bytes", type=int, default=256 * 1024 * 1024)
    args = parser.parse_args()
    try:
        run(args.batch.absolute(), args.output.absolute(), args.format, args.start, args.end, args.address, args.max_bytes)
    except (ValueError, KeyError, TypeError, OSError) as error:
        parser.exit(1, f"Export refused: {error}\n")
