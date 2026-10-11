#!/usr/bin/env python3
"""Fail fast when shipped Cargo.lock files do not describe their manifests.

This is intentionally a structural preflight, not a replacement for Cargo's
`--locked` verification. It catches the release-packaging regression where a
workspace version/manifests were changed without regenerating Cargo.lock.
"""
from __future__ import annotations
import pathlib
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]


def package_dependencies(manifest: dict) -> set[str]:
    names: set[str] = set()
    for section in ("dependencies", "dev-dependencies", "build-dependencies"):
        for name in manifest.get(section, {}):
            names.add(name)
    return names


def check_workspace(root: pathlib.Path, label: str) -> list[str]:
    errors: list[str] = []
    workspace_manifest = tomllib.loads((root / "Cargo.toml").read_text())
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    packages = lock.get("package", [])
    locked = {(p["name"], p["version"]): p for p in packages}
    by_name: dict[str, list[dict]] = {}
    for package in packages:
        by_name.setdefault(package["name"], []).append(package)

    workspace_version = workspace_manifest.get("workspace", {}).get("package", {}).get("version")
    workspace_names: set[str] = set()
    for member in workspace_manifest.get("workspace", {}).get("members", []):
        path = root / member / "Cargo.toml"
        if not path.is_file():
            errors.append(f"{label}: missing workspace member manifest: {member}/Cargo.toml")
            continue
        manifest = tomllib.loads(path.read_text())
        package = manifest.get("package", {})
        name = package.get("name")
        version = package.get("version")
        if isinstance(version, dict) and version.get("workspace") is True:
            version = workspace_version
        if not name or not version:
            errors.append(f"{label}: cannot resolve package name/version for {member}")
            continue
        workspace_names.add(name)
        entry = locked.get((name, str(version)))
        if entry is None:
            versions = ", ".join(sorted(p["version"] for p in by_name.get(name, []))) or "absent"
            errors.append(f"{label}: Cargo.lock has {name} version(s) [{versions}], manifest requires {version}")
            continue
        declared = package_dependencies(manifest)
        locked_names = {str(d).split(" ", 1)[0] for d in entry.get("dependencies", [])}
        missing = declared - locked_names
        if missing:
            errors.append(f"{label}: lock entry for {name} is missing dependencies: {', '.join(sorted(missing))}")

    # A pathless lock entry represents a local/path package. In these shipped
    # workspaces every local package is an explicit member, so stale inherited
    # workspace packages are a release error rather than harmless lock noise.
    local_locked = {p["name"] for p in packages if "source" not in p}
    stale_local = local_locked - workspace_names
    if stale_local:
        errors.append(f"{label}: Cargo.lock contains stale local package(s): {', '.join(sorted(stale_local))}")

    # Validate the complete lock graph: every dependency edge must resolve to
    # at least one locked package, and version-qualified edges must resolve to
    # that exact version.
    for package in packages:
        for dependency in package.get("dependencies", []):
            parts = str(dependency).split()
            dep_name = parts[0]
            candidates = by_name.get(dep_name, [])
            if not candidates:
                errors.append(f"{label}: {package['name']} references missing locked dependency {dependency}")
                continue
            if len(parts) >= 2 and parts[1][:1].isdigit() and not any(p["version"] == parts[1] for p in candidates):
                errors.append(f"{label}: {package['name']} requires locked dependency {dependency}, but that version is absent")
    return errors


def main() -> int:
    errors = check_workspace(ROOT / "rust", "HindSight core")
    errors += check_workspace(ROOT / "Engines" / "VectorEmu", "VectorEmu")
    if errors:
        print("Cargo lockfile preflight failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        print("Regenerate both lockfiles with scripts/regenerate-lockfiles.sh, review the diff, then commit them.", file=sys.stderr)
        return 2
    print("Cargo lockfile structural preflight passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
