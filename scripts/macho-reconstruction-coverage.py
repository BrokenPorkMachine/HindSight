#!/usr/bin/env python3
"""Evidence-gated Mach-O reconstruction batch coverage report.

This report does not assert source fidelity or behavior equivalence. It verifies
an existing reconstruction batch and reports precisely what was recovered.
"""
import argparse
import hashlib
import json
import pathlib
import stat
import os
import sys

from reconstruction_batch import read_batch_manifest, verified_outputs


def inspect_batch(directory: pathlib.Path, source: pathlib.Path | None = None) -> dict:
    manifest, raw = read_batch_manifest(directory)
    verified = verified_outputs(directory, manifest)
    if source is not None:
        if source.is_symlink() or not source.is_file():
            raise ValueError("source must be a regular, non-symlink file")
        # A single bounded streaming read avoids allocating a full firmware or
        # large Mach-O image on the 8 GiB release host. Open without following
        # the final symlink and reject mutation while hashing.
        fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW)
        try:
            before = os.fstat(fd)
            if not stat.S_ISREG(before.st_mode) or not 0 < before.st_size <= 1024 * 1024 * 1024:
                raise ValueError("source is not a bounded regular file")
            hasher = hashlib.sha256()
            header = b""
            remaining = before.st_size
            while remaining:
                chunk = os.read(fd, min(1024 * 1024, remaining))
                if not chunk:
                    raise ValueError("source ended during verification")
                if len(header) < 8:
                    header += chunk[:8 - len(header)]
                hasher.update(chunk)
                remaining -= len(chunk)
            after = os.fstat(fd)
            identity_before = (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns)
            identity_after = (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns)
            if identity_before != identity_after:
                raise ValueError("source changed during verification")
        finally:
            os.close(fd)
        if hasher.hexdigest() != manifest["source_sha256"]:
            raise ValueError("source SHA-256 does not match the reconstruction batch")
        if header[:4] != bytes.fromhex("cffaedfe"):
            raise ValueError("source is not a little-endian thin 64-bit Mach-O")
        # The header was captured from the same verified fd; do not reopen an
        # attacker-replaceable path after identity verification.
        if len(header) != 8 or int.from_bytes(header[4:8], "little") not in (0x0100000c, 0x01000007):
            raise ValueError("Mach-O CPU type is outside this decompiler milestone")
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
        "macho_format": "verified thin 64-bit" if source is not None else "UNVERIFIED (source not supplied)",
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
