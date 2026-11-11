#!/usr/bin/env python3
"""Fail if active HindSight release documentation contradicts source identities.

This deliberately checks only *current* source-of-truth documentation, never
historical release notes. It is not a substitute for native macOS qualification.
"""
import argparse
import pathlib
import plistlib
import re
import sys


def check(root: pathlib.Path) -> list[str]:
    errors: list[str] = []

    def require(ok: bool, message: str) -> None:
        if not ok:
            errors.append(message)

    try:
        app = plistlib.loads((root / "Config/App-Info.plist").read_bytes())
        coordinator = plistlib.loads((root / "Config/Coordinator-Info.plist").read_bytes())
        schema_sql = (root / "Resources/schema.sql").read_text()
        store = (root / "PlatformServices/CoordinatorService/ProjectStore.swift").read_text()
        validator = (root / "scripts/validate-source.py").read_text()
        status = (root / "docs/STATUS.md").read_text()
        readiness = (root / "docs/RELEASE_READINESS.md").read_text()
        validation = (root / "docs/VALIDATION.md").read_text()
        roadmap = (root / "docs/MASTER_ROADMAP.md").read_text()
        advisory = (root / "RELEASE_ADVISORY.md").read_text()
    except (OSError, ValueError, TypeError) as exc:
        return [f"cannot read source/release identity: {exc}"]

    version = app.get("CFBundleShortVersionString")
    build = app.get("CFBundleVersion")
    require(isinstance(version, str) and re.fullmatch(r"\d+\.\d+\.\d+", version) is not None,
            "app release version must be SemVer x.y.z")
    require(isinstance(build, str) and build.isdecimal(), "app build must be numeric")
    require(coordinator.get("CFBundleShortVersionString") == version,
            "app/Coordinator product versions disagree")
    require(coordinator.get("CFBundleVersion") == build,
            "app/Coordinator bundle builds disagree")
    if not isinstance(version, str):
        return errors

    schema_match = re.search(r"PRAGMA\s+user_version\s*=\s*(\d+)\s*;", schema_sql)
    require(schema_match is not None, "project SQLite schema version missing")
    if schema_match:
        schema = schema_match.group(1)
        require(f"version != {schema} || metadataVersion != {schema}" in store,
                "ProjectStore does not recognize Resources/schema.sql current schema")
        require(f"Project SQLite schema {schema}" in status,
                "STATUS.md current project SQLite schema is stale")
        require(f"Project schema remains version {schema}" in status,
                "STATUS.md stable-source project schema is stale")

    engine_match = re.search(r'analysisCacheEngineVersion\s*=\s*"([^"]+)"', validator)
    require(engine_match is not None, "unable to determine current analysis engine identity from source validator")
    if engine_match:
        engine = engine_match.group(1)
        for name, document in [
            ("STATUS.md", status),
            ("RELEASE_READINESS.md", readiness),
            ("VALIDATION.md", validation),
        ]:
            require(engine in document, f"{name} does not identify current analysis engine {engine}")

    for name, document in [
        ("STATUS.md", status),
        ("RELEASE_READINESS.md", readiness),
        ("VALIDATION.md", validation),
        ("MASTER_ROADMAP.md", roadmap),
        ("RELEASE_ADVISORY.md", advisory),
    ]:
        require(version in document, f"{name} omits current product version {version}")

    require("Apple Silicon" in readiness and "Intel" in readiness,
            "RELEASE_READINESS.md must distinguish official Apple Silicon and optional Intel")
    require("every distributed executable must contain both" not in readiness,
            "official Apple Silicon release cannot require both arm64 and x86_64")
    require("both release hosts" not in readiness,
            "official release matrix cannot require an Intel host")
    require("docs/MASTER_ROADMAP.md" not in roadmap, "master roadmap should be self-contained")

    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=pathlib.Path, default=pathlib.Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    errors = check(args.root.resolve())
    for error in errors:
        print("FAIL:", error, file=sys.stderr)
    if errors:
        return 1
    print("HindSight current release identities and documented platform policy: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
