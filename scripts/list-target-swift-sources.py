#!/usr/bin/env python3
"""Print the Swift source files for one generated Xcode target.

The generated project topology is the single source of truth for release-host
Swift type-check coverage.  Keeping this in a small portable helper prevents
hand-maintained shell source lists from silently omitting newly added files.
"""
from __future__ import annotations

import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
TOPOLOGY = ROOT / "Config" / "project-topology.json"


def fail(message: str) -> "NoReturn":
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(2)


def main() -> int:
    if len(sys.argv) != 2 or not sys.argv[1].strip():
        fail("usage: list-target-swift-sources.py <target>")
    target = sys.argv[1]
    try:
        topology = json.loads(TOPOLOGY.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        fail(f"cannot read generated topology: {exc}")
    targets = topology.get("targets")
    if not isinstance(targets, dict) or target not in targets:
        fail(f"unknown generated target: {target}")
    entries = targets[target]
    if not isinstance(entries, list):
        fail(f"invalid source list for target: {target}")
    swift_sources: list[pathlib.Path] = []
    for relative in entries:
        if not isinstance(relative, str):
            fail(f"non-string topology entry in target {target}")
        if not relative.endswith(".swift"):
            continue
        source = (ROOT / relative).resolve()
        try:
            source.relative_to(ROOT.resolve())
        except ValueError:
            fail(f"source escapes repository root: {relative}")
        if not source.is_file():
            fail(f"missing Swift source in target {target}: {relative}")
        swift_sources.append(source)
    if not swift_sources:
        fail(f"target has no Swift sources: {target}")
    for source in swift_sources:
        print(source)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
