#!/usr/bin/env python3
"""Compile and qualitatively score the redistributable decompiler corpus."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import uuid


ROOT = pathlib.Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "Tests" / "Fixtures" / "Decompilation"
CASES = {
    "c": ("flow_sample.c", "clang"),
    "objective-c": ("objective_c_sample.m", "clang"),
    "cpp": ("cpp_sample.cpp", "clang++"),
    "objective-cpp": ("objective_cpp_sample.mm", "clang++"),
}


def read_analysis_frame(data: bytes) -> dict:
    if len(data) < 4:
        raise RuntimeError("worker returned a truncated analysis frame")
    size = struct.unpack(">I", data[:4])[0]
    if size > 32 * 1024 * 1024 or len(data) != size + 4:
        raise RuntimeError(f"invalid worker frame length: header={size}, payload={len(data) - 4}")
    return json.loads(data[4:])


def compile_fixture(source_type: str, source: pathlib.Path, output: pathlib.Path, optimization: str) -> None:
    compiler = shutil.which(CASES[source_type][1])
    if compiler is None:
        raise RuntimeError(f"required compiler is unavailable: {CASES[source_type][1]}")
    command = [compiler, "-target", "arm64-apple-macos14.0", optimization, "-g", "-Wl,-no_fixup_chains", "-o", str(output), str(source)]
    if source_type in {"objective-c", "objective-cpp"}:
        command.extend(["-fobjc-arc", "-framework", "Foundation"])
    subprocess.run(command, check=True, capture_output=True, text=True)


def analyze(worker: pathlib.Path, binary: pathlib.Path) -> dict:
    raw = binary.read_bytes()
    result = subprocess.run(
        [str(worker), "--analyze", str(binary), hashlib.sha256(raw).hexdigest(), str(len(raw)), str(uuid.uuid4())],
        check=True,
        capture_output=True,
        timeout=180,
    )
    report = read_analysis_frame(result.stdout)
    if report.get("kind") != "analysis_result":
        raise RuntimeError(f"worker rejected {binary.name}: {report}")
    return report


def score(source_type: str, report: dict) -> dict[str, int]:
    semantic = report.get("semantic_preview") or ""
    functions = report.get("functions") or ""
    cfg = report.get("cfg") or ""
    ir = report.get("ir") or ""
    symbol_resolver = report.get("symbol_resolver") or ""
    symbolication = report.get("symbolication") or ""
    function_starts = {
        int(match.group(1), 16)
        for line in functions.splitlines()
        if (match := re.match(r"^(0x[0-9a-fA-F]+)\s+0x[0-9a-fA-F]+\s+", line))
    }
    resolver_starts = {
        int(match.group(1), 16)
        for line in symbol_resolver.splitlines()
        if (match := re.match(r"^range start=(0x[0-9a-fA-F]+)\s", line))
    }
    symbolication_starts = {
        int(match.group(1), 16)
        for line in symbolication.splitlines()
        if (match := re.match(r"^range start=(0x[0-9a-fA-F]+)\s", line))
    }
    pseudocode = semantic.split("Reconstructed pseudocode\n========================\n", 1)
    pseudocode = pseudocode[1] if len(pseudocode) == 2 else ""
    address_rows = [
        line.split("/*", 1)[0]
        for line in pseudocode.splitlines()
        if re.match(r"^\s+0x[0-9a-fA-F]+:", line)
    ]
    pseudocode_sites = {
        int(match.group(1), 16)
        for line in pseudocode.splitlines()
        if (match := re.match(r"^\s+(0x[0-9a-fA-F]+):", line))
    }
    ir_sites = {
        int(match.group(1), 16)
        for line in ir.splitlines()
        if (match := re.match(r"^(0x[0-9a-fA-F]+)\s+(?:op|branch|cbranch|term|opaque)\s", line))
    }
    metrics = {
        "functions": int(report.get("function_count", 0)),
        "blocks": int(report.get("basic_block_count", 0)),
        "instructions": int(report.get("analyzed_instructions", 0)),
        "decoded_instructions": sum(1 for line in ir.splitlines() if " op " in line or " branch " in line or " cbranch " in line or " term " in line),
        "pseudocode_functions": semantic.count("decoded machine operations translated to C-like statements"),
        "objc_method_records": (report.get("objc_classes") or "").count("confidence=validated"),
        "objc_selector_calls": (report.get("semantic_preview") or "").count("objc_msgSend selector validated through __objc_selrefs"),
        "abi_parameter_bindings": semantic.count("AAPCS64 parameter bindings:"),
        "resolved_parameter_name_uses": sum(
            len(re.findall(r"\b(?:self|_cmd|arg\d+)\b", line.split("/*", 1)[0]))
            for line in semantic.splitlines()
            if re.match(r"^\s+0x[0-9a-fA-F]+:", line)
        ),
        "pseudocode_address_rows": len(pseudocode_sites),
        "source_mapping_valid_rows": len(pseudocode_sites & ir_sites),
        "expression_rows": sum(
            1 for line in address_rows
            if " = " in line or re.search(r"\b(?:cmp|tst|fcmp)\s*\(", line)
        ),
        "resolved_call_target_rows": sum(
            1 for line in address_rows
            if re.search(r"\b[A-Za-z_$][A-Za-z0-9_$:]*\s*\([^;]*\)\s*;", line)
        ),
        "type_evidence_rows": sum(
            1 for line in (report.get("call_values") or "").splitlines()
            if " declaration=" in line and " source=" in line and " language=" in line
        ),
        "cpp_signature_records": (report.get("call_values") or "").count("Itanium mangled parameter types"),
        "function_rows": sum(1 for line in functions.splitlines() if "confidence=" in line),
        "cfg_blocks": sum(1 for line in cfg.splitlines() if line.startswith("block B")),
    }
    if metrics["functions"] < 1 or metrics["blocks"] < 1 or metrics["instructions"] < 1:
        raise RuntimeError(f"{source_type} fixture produced no bounded function analysis: {metrics}")
    if metrics["function_rows"] < 1 or metrics["cfg_blocks"] < 1 or metrics["decoded_instructions"] < 1:
        raise RuntimeError(f"{source_type} fixture did not produce function, CFG, and IR evidence: {metrics}")
    if metrics["pseudocode_functions"] < 1:
        raise RuntimeError(f"{source_type} fixture produced no function pseudocode: {metrics}")
    if not pseudocode_sites or pseudocode_sites - ir_sites:
        missing = sorted(pseudocode_sites - ir_sites)
        raise RuntimeError(f"{source_type} pseudocode source addresses did not map to lifted IR: {missing[:8]}")
    if not function_starts or function_starts != resolver_starts or function_starts != symbolication_starts:
        raise RuntimeError(
            f"{source_type} fixture reports inconsistent normalized function boundaries: "
            f"functions={len(function_starts)}, resolver={len(resolver_starts)}, "
            f"symbolication={len(symbolication_starts)}"
        )
    metrics["normalized_boundary_sets"] = 1
    if source_type in {"objective-c", "objective-cpp"} and metrics["objc_method_records"] < 1:
        raise RuntimeError(f"{source_type} fixture produced no validated Objective-C method metadata: {metrics}")
    if source_type in {"objective-c", "objective-cpp"} and metrics["objc_selector_calls"] < 1:
        raise RuntimeError(f"{source_type} fixture produced no validated selector message-send: {metrics}")
    if source_type in {"objective-c", "objective-cpp"} and ("self=x0" not in semantic or "_cmd=x1" not in semantic):
        raise RuntimeError(f"{source_type} fixture did not reconstruct the Objective-C hidden ABI parameters: {metrics}")
    if source_type in {"c", "objective-c"} and metrics["resolved_parameter_name_uses"] < 1:
        raise RuntimeError(f"{source_type} fixture did not use a recovered parameter name in pseudocode: {metrics}")
    if metrics["expression_rows"] < 1:
        raise RuntimeError(f"{source_type} fixture produced no measurable pseudocode expressions: {metrics}")
    return metrics


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", type=pathlib.Path, default=ROOT / "rust" / "target" / "debug" / "reworker")
    parser.add_argument("--keep", action="store_true", help="keep compiled corpus binaries in a temporary directory")
    args = parser.parse_args()
    worker = args.worker.expanduser().resolve()
    if not worker.is_file():
        parser.error(f"worker binary not found: {worker}; build it with cargo build -p reworkbench-worker")
    if sys.platform != "darwin":
        parser.error("the corpus compiles Apple Mach-O fixtures and requires macOS with the Apple SDK")

    temporary = tempfile.TemporaryDirectory(prefix="hindsight-decompilation-corpus-")
    directory = pathlib.Path(temporary.name)
    try:
        rows = []
        for source_type, (filename, _) in CASES.items():
            source = FIXTURES / filename
            for optimization in ("-O0", "-O2"):
                name = f"{source_type.replace('-', '_')}-{optimization[1:]}"
                original = directory / name
                compile_fixture(source_type, source, original, optimization)
                for stripped in (False, True):
                    binary = original
                    if stripped:
                        binary = directory / f"{name}-stripped"
                        shutil.copy2(original, binary)
                        subprocess.run(["/usr/bin/strip", "-S", "-x", str(binary)], check=True, capture_output=True, text=True)
                    report = analyze(worker, binary)
                    metrics = score(source_type, report)
                    rows.append({"language": source_type, "optimization": optimization, "stripped": stripped, **metrics})
        totals = {
            metric: sum(row[metric] for row in rows)
            for metric in ("expression_rows", "resolved_call_target_rows", "type_evidence_rows", "source_mapping_valid_rows")
        }
        if totals["resolved_call_target_rows"] < 1:
            raise RuntimeError(f"corpus produced no measurable resolved call targets: {totals}")
        if totals["type_evidence_rows"] < 1:
            raise RuntimeError(f"corpus produced no measurable type evidence: {totals}")
        print(json.dumps({"corpus": str(FIXTURES), "variants": len(rows), "totals": totals, "results": rows}, indent=2, sort_keys=True))
        if args.keep:
            kept = pathlib.Path(tempfile.mkdtemp(prefix="hindsight-decompilation-corpus-kept-"))
            for item in directory.iterdir():
                shutil.copy2(item, kept / item.name)
            print(f"kept binaries: {kept}")
        return 0
    finally:
        temporary.cleanup()


if __name__ == "__main__":
    raise SystemExit(main())
