#!/usr/bin/env python3
"""Qualify declaration-only headers without claiming FP/stack body recovery."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

from reconstruction_artifact import ReconstructionArtifact

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("header_requests", ROOT / "scripts/test-worker-reconstruction.py")
requests = importlib.util.module_from_spec(spec)
spec.loader.exec_module(requests)


def run(worker):
    if sys.platform != "darwin":
        raise SystemExit("Header qualification requires the Apple SDK/toolchain.")
    declarations = ["double hs_double(double first, float second)",
                    "float hs_float(float value)", "long double hs_long_double(long double value)",
                    "uint64_t hs_stack(uint64_t a, uint64_t b, uint64_t c, uint64_t d, uint64_t e, uint64_t f, uint64_t g, uint64_t h, uint64_t i)"]
    names = ["hs_double", "hs_float", "hs_long_double", "hs_stack"]
    aggregate_types = ["NSRange", "CFRange", "CGPoint", "CGSize", "CGRect", "CGAffineTransform"]
    declarations.extend(f"{ty} hs_sdk_{ty}({ty} value)" for ty in aggregate_types)
    names.extend("hs_sdk_" + ty for ty in aggregate_types)
    with tempfile.TemporaryDirectory(prefix="hindsight-headers-") as temporary:
        root = Path(temporary)
        contract = root / "native-header-contract"
        environment = dict(os.environ, CLANG_MODULE_CACHE_PATH=str(root / "module-cache"))
        subprocess.run(["xcrun", "swiftc", str(ROOT / "Shared/ReconstructionArtifact.swift"),
                        str(ROOT / "Tests/reconstruction_artifact_contract.swift"), "-o", str(contract)],
                       check=True, capture_output=True, timeout=120, env=environment)
        fixture = root / "original.c"
        fixture.write_text("#include <stdint.h>\n#import <Foundation/Foundation.h>\n#include <CoreGraphics/CoreGraphics.h>\n" + "\n".join(
            declaration + (" { return value; }" if name.startswith("hs_sdk_") else " { return 0; }")
            for name, declaration in zip(names, declarations)))
        count = syntax_checks = language_rejections = 0
        for optimization in ["-O0", "-O1", "-O2", "-Os"]:
            binary = root / "original.dylib"
            subprocess.run(["xcrun", "clang", "-x", "objective-c", "-dynamiclib", optimization,
                            str(fixture), "-framework", "Foundation", "-o", str(binary)], check=True)
            digest = hashlib.sha256(binary.read_bytes()).hexdigest()
            prototypes = root / "prototypes.tsv"
            prototypes.write_text(f"# source_sha256={digest}\n" + "".join(
                f"_{name}\t{declaration};\n" for name, declaration in zip(names, declarations)))
            report = requests.request(worker, binary, "--analyze")
            ranges = {}
            for line in report["functions"].splitlines():
                match = re.match(r"^(0x[0-9a-f]+)\s+(0x[0-9a-f]+)\s+\d+\s+bytes\s+(\S+)", line)
                if match:
                    ranges[match[3]] = match[1], match[2]
            for name, declaration in zip(names, declarations):
                artifact = requests.request(worker, binary, "--reconstruct", *ranges["_" + name], prototypes=prototypes)
                ReconstructionArtifact(artifact)
                headers = [node["payload"] for node in artifact["nodes"]
                           if node["payload"].get("artifact_kind") == "recovered-header"]
                assert len(headers) == 1, (optimization, name)
                header = headers[0]
                expected_languages = (["objective-c", "objective-c++"] if name == "hs_sdk_NSRange" else
                                      ["c", "c++", "objective-c", "objective-c++"])
                assert header["languages"] == expected_languages
                if optimization == "-Os" and name in {"hs_double", "hs_sdk_NSRange"}:
                    artifact_file = root / (name + ".json")
                    artifact_file.write_text(json.dumps(artifact))
                    subprocess.run([str(contract), str(artifact_file), digest,
                                    artifact["function"]["start"][2:], artifact["function"]["end"][2:]],
                                   check=True, timeout=60, env=environment)
                assert header["status"] == "declaration-only-uncompiled" and "ast_root" not in header
                assert declaration + ";" in header["source"], (name, header)
                assert not any(node["payload"].get("artifact_kind") == "recovered-source" for node in artifact["nodes"])
                path = root / (name + ".h")
                path.write_text(header["source"])
                for language, standard in [("c-header", "c17"), ("c++-header", "c++17"),
                                           ("objective-c-header", "c17"), ("objective-c++-header", "c++17")]:
                    if name == "hs_sdk_NSRange" and not language.startswith("objective-c"):
                        assert "Foundation SDK declarations require Objective-C" in header["source"]
                        refused = subprocess.run(["xcrun", "clang", "-fsyntax-only", "-ferror-limit=1", "-x", language,
                                                  "-std=" + standard, str(path)], capture_output=True)
                        assert refused.returncode != 0 and b"Foundation SDK declarations require Objective-C" in refused.stderr
                        language_rejections += 1
                        continue
                    subprocess.run(["xcrun", "clang", "-fsyntax-only", "-x", language, "-std=" + standard,
                                    "-Wall", "-Wextra", "-Werror", str(path)], check=True)
                    syntax_checks += 1
                count += 1
            batch = root / ("batch" + optimization)
            subprocess.run([sys.executable, str(ROOT / "scripts/reconstruct-binary.py"), str(binary), str(batch),
                            "--worker", str(worker), "--prototypes", str(prototypes)], check=True, timeout=120)
            manifest = json.loads((batch / "manifest.json").read_bytes())
            assert len(manifest["functions"]) == len(names)
            assert all(entry["state"] == "review-only" for entry in manifest["functions"])
            subprocess.run([sys.executable, str(ROOT / "scripts/qualify-reconstruction-batch.py"),
                            str(batch), "--headers-only"], check=True, timeout=120)
            qualification = json.loads((batch / "header-qualification/report.json").read_bytes())
            assert qualification["status"] == "passed" and len(qualification["headers"]) == len(names)
            assert len(qualification["header_checks"]) == 38
            for entry in manifest["functions"]:
                artifact_file = next(file for file in entry["files"] if file["path"].endswith(".json"))
                artifact = json.loads((batch / artifact_file["path"]).read_bytes())
                languages = next(node["payload"]["languages"] for node in artifact["nodes"]
                                 if node["payload"].get("artifact_kind") == "recovered-header")
                header_file = next(file for file in entry["files"] if file["path"].endswith(".h"))
                checks = [check for check in qualification["header_checks"] if check["path"] == header_file["path"]]
                assert [check["language"] for check in checks] == languages
                assert all(check["language_origin"] == "artifact" and check["status"] == "passed" and
                           check["sha256"] == header_file["sha256"] for check in checks)
            dumped = root / ("dump" + optimization)
            subprocess.run([sys.executable, str(ROOT / "scripts/export-reconstruction-batch.py"),
                            str(batch), str(dumped), "--format", "header"], check=True, timeout=120)
            exported = json.loads((dumped / "export-manifest.json").read_bytes())
            assert exported["available"] == len(names) and exported["unavailable"] == 0
            for entry in exported["functions"]:
                graph_file = next(file for file in entry["files"] if file["path"].endswith(".json"))
                graph = json.loads((dumped / graph_file["path"]).read_bytes())
                payload = next(node["payload"] for node in graph["nodes"]
                               if node["payload"].get("artifact_kind") == "recovered-header")
                assert entry["languages"] == payload["languages"]
                for file in entry["files"]:
                    raw = (dumped / file["path"]).read_bytes()
                    assert hashlib.sha256(raw).hexdigest() == file["sha256"]
                    assert raw == (batch / file["path"]).read_bytes()
        print(json.dumps({"declaration_only_configurations": count, "header_syntax_checks": syntax_checks,
                          "explicit_language_rejections": language_rejections,
                          "body_recovery": "not claimed", "status": "passed"}))


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else ROOT / "rust/target/debug/reworker").resolve())
