#!/usr/bin/env python3
"""Evidence-gated Mach-O reconstruction batch coverage report.

This report does not assert source fidelity or behavior equivalence. It verifies
an existing reconstruction batch and reports precisely what was recovered.
"""
import argparse
import hashlib
import json
import pathlib
import re
import sys

from reconstruction_batch import read_batch_manifest, verified_outputs, read_bounded


def inspect_batch(directory: pathlib.Path, source: pathlib.Path | None = None) -> dict:
    manifest, raw = read_batch_manifest(directory)
    verified = verified_outputs(directory, manifest)
    if source is not None:
        if source.is_symlink() or not source.is_file():
            raise ValueError("source must be a regular, non-symlink file")
        digest = hashlib.sha256(read_bounded(source, 1024 * 1024 * 1024)).hexdigest()
        if digest != manifest["source_sha256"]:
            raise ValueError("source SHA-256 does not match the reconstruction batch")
        # These thin Mach-O magic bytes are independent of filename/extension.
        magic = read_bounded(source, 1024 * 1024 * 1024)[:4]
        if magic not in (bytes.fromhex("cffaedfe"), bytes.fromhex("cefaedfe")):
            raise ValueError("source is not a supported little-endian thin Mach-O")
    stats = {"recovered": 0, "review_only": 0, "failed": 0}
    entries = []
    for entry, files in verified:
        state = entry["state"]
        stats[state.replace("-", "_")] += 1
        types = sorted(
            name.rsplit(".", 1)[-1] for name in files
            if not name.endswith(".reconstruction.json")
        )
        blockers = entry.get("source_blockers", [])
        if not isinstance(blockers, list):
            raise ValueError("source blockers must be a list")
        entries.append({
            "start": entry["start"], "end": entry["end"],
            "state": state, "output_formats": types,
            "blockers": [str(b)[:256] for b in blockers[:32]],
            "unresolved_indirect_calls": entry.get("unresolved_indirect_calls", 0),
            "unresolved_dependencies": entry.get("unresolved_dependencies", [])[:64],
        })
    attempted = len(entries)
    if sum(stats.values()) != attempted:
        raise ValueError("inconsistent batch states")
    return {
        "schema": 1,
        "claim": "validated reconstruction artifact coverage, NOT original source recovery or behavioral equivalence",
        "source_sha256": manifest["source_sha256"],
        "manifest_sha256": hashlib.sha256(raw).hexdigest(),
        "attempted": attempted,
        "discovered_in_scope": manifest.get("discovered_in_scope"),
        "omitted_by_limit": manifest.get("omitted_by_limit"),
        "states": stats,
        "recovery_fraction": round(stats["recovered"] / attempted, 4) if attempted else 0,
        "functions": entries,
        "behavioral_equivalence": "NOT TESTED",
        "compilation_linkage": "NOT TESTED",
    }


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("batch", type=pathlib.Path)
    parser.add_argument("--source", type=pathlib.Path,
                        help="verify SHA and thin Mach-O magic against the original binary")
    parser.add_argument("--minimum-recovered", type=int, default=0,
                        help="fail if fewer than N function sources were verified")
    parser.add_argument("--require-no-failures", action="store_true")
    args = parser.parse_args(argv)
    if args.minimum_recovered < 0:
        parser.error("minimum-recovered must be nonnegative")
    try:
        report = inspect_batch(args.batch, args.source)
    except (ValueError, OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        print(f"invalid Mach-O reconstruction coverage: {error}", file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2, sort_keys=True))
    if report["states"]["recovered"] < args.minimum_recovered:
        return 1
    if args.require_no_failures and report["states"]["failed"] != 0:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
