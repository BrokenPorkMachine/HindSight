#!/usr/bin/env python3
"""Qualify hash-verified batch headers or compile/link recovered implementations on macOS."""
import argparse
import hashlib
import json
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile

from reconstruction_batch import read_batch_manifest, verified_outputs


def execute(command):
    result = subprocess.run(command, capture_output=True, text=True, timeout=120)
    if result.returncode:
        raise RuntimeError(result.stderr[-16384:])
    return result.stdout.strip()


def compile_sources(sources, destination, report):
    with tempfile.TemporaryDirectory(prefix="hindsight-batch-build-") as temporary:
        root = Path(temporary)
        objects = []
        for index, source in enumerate(sources):
            obj = root / f"function-{index}.o"
            execute(["xcrun", "clang", "-x", "objective-c", "-std=c17", "-O2",
                     "-Wall", "-Wextra", "-Werror", "-c", str(source), "-o", str(obj)])
            objects.append(obj)
            report["compiled"].append(source.name)
        response = root / "objects.rsp"
        response.write_text("\n".join(shlex.quote(str(obj)) for obj in objects))
        library = root / "recovered.dylib"
        execute(["xcrun", "clang", "-dynamiclib", "@" + str(response),
                 "-Wl,-undefined,error", "-framework", "Foundation",
                 "-framework", "CoreFoundation", "-lobjc", "-o", str(library)])
        raw = library.read_bytes()
        with (destination / "recovered.dylib").open("xb") as output:
            output.write(raw)
        report["library_sha256"] = hashlib.sha256(raw).hexdigest()


def run(directory, headers_only=False):
    manifest, manifest_bytes = read_batch_manifest(directory)
    report = {"schema": 1, "batch_manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest(),
              "source_sha256": manifest["source_sha256"],
              "mode": "headers-only" if headers_only else "implementations",
              "claim": ("declaration syntax only; no implementation, linkage, behavior or full recovery claim" if headers_only else
                        "header syntax and compilation/linkage of recovered subset only; behavior and full recovery unqualified"),
              "compiler": execute(["xcrun", "clang", "--version"]),
              "compiled": [], "headers": [], "best_effort_headers": [], "header_checks": [], "unrecovered": [], "status": "failed"}
    # Publish into a fresh subdirectory so prior evidence is never replaced.
    destination = directory / ("header-qualification" if headers_only else "build-qualification")
    destination.mkdir(exist_ok=False)
    try:
        verified = verified_outputs(directory, manifest)
        # Compile a private snapshot of precisely the bytes just verified,
        # rather than reopening mutable input paths after their hash check.
        with tempfile.TemporaryDirectory(prefix="hindsight-verified-batch-") as temporary:
            snapshot = Path(temporary)
            for _, files in verified:
                for filename, raw in files.items():
                    with (snapshot / filename).open("xb") as output:
                        output.write(raw)
            sources = []
            for entry, files in verified:
                header_languages = {}
                for filename, raw in files.items():
                    if filename.endswith(".reconstruction.json"):
                        for node in json.loads(raw)["nodes"]:
                            payload = node["payload"]
                            if payload.get("artifact_kind") == "recovered-header":
                                header_languages = {"languages": payload.get("languages", ["objective-c", "objective-c++"]),
                                                    "origin": "artifact" if "languages" in payload else "legacy-qualification-default"}
                            elif payload.get("artifact_kind") == "best-effort-header":
                                header_languages = {"best_effort": True}
                if entry["state"] != "recovered":
                    report["unrecovered"].append(entry["start"])
                for filename in files:
                    path = snapshot / filename
                    if filename.endswith(".h"):
                        if header_languages.get("best_effort"):
                            report["best_effort_headers"].append(filename)
                            continue
                        standards = {"c": "c17", "c++": "c++17", "objective-c": "c17", "objective-c++": "c++17"}
                        for language in header_languages["languages"]:
                            standard = standards[language]
                            execute(["xcrun", "clang", "-fsyntax-only", "-x", language + "-header",
                                     "-std=" + standard, "-Wall", "-Wextra", "-Werror", str(path)])
                            report["header_checks"].append({"path": filename, "sha256": hashlib.sha256(files[filename]).hexdigest(),
                                                            "language": language, "standard": standard,
                                                            "language_origin": header_languages["origin"], "status": "passed"})
                        report["headers"].append(filename)
                    if entry["state"] == "recovered" and filename.endswith(".c") and not filename.endswith(".machine.c"):
                        sources.append(path)
            if headers_only:
                if not report["headers"]:
                    raise ValueError("no recovered declarations to qualify")
            else:
                if not sources:
                    raise ValueError("no recovered implementations to compile")
                compile_sources(sources, destination, report)
        report["status"] = "passed"
    except (ValueError, KeyError, TypeError, OSError, RuntimeError, subprocess.SubprocessError) as error:
        report["error"] = str(error)[:16384]
    with (destination / "report.json").open("x") as output:
        json.dump(report, output, sort_keys=True, indent=2)
    print(json.dumps({"status": report["status"], "mode": report["mode"], "headers": len(report["headers"]), "compiled": len(report["compiled"]),
                      "unrecovered": len(report["unrecovered"])}))
    return report["status"] == "passed"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--headers-only", action="store_true", help="qualify declaration syntax without compiling/linking implementations; writes header-qualification")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("qualification requires the Apple toolchain on macOS")
    raise SystemExit(0 if run(args.directory.absolute(), headers_only=args.headers_only) else 1)
