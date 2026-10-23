"""Apple Clang declarations, retaining source types and canonical ABI types."""

import hashlib
import argparse
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import uuid
import tempfile


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_HEADER = ROOT / "native/reconstruction/sdk_declarations.h"
MAX_AST_BYTES = 4 * 1024 * 1024


def sdk_import_candidates(linkage_records):
    """Select public-system imports, retaining the recorded provider identity.

    This is declaration lookup eligibility, not proof of runtime resolution.
    Special ordinals, private frameworks, addends and conflicting providers
    must not seed automatic SDK prototypes.
    """
    if not isinstance(linkage_records, str) or len(linkage_records.encode()) > 65535:
        raise ValueError("linkage records exceed section budget")
    providers = {}
    for line in linkage_records.splitlines():
        match = re.match(r"^symbol=(_[A-Za-z][A-Za-z0-9_]*) import\[\d+\] dylib=(\S+) addend=(-?\d+) weak=(yes|no)(?:\s|$)", line)
        if not match:
            continue
        symbol, provider, addend, _ = match.groups()
        public = (re.fullmatch(r"/usr/lib/(?:system/)?lib[A-Za-z0-9_.+-]+\.dylib", provider)
                  or re.fullmatch(r"/System/Library/Frameworks/([A-Za-z][A-Za-z0-9_]*)\.framework/(?:Versions/[A-Za-z0-9]+/)?\1", provider))
        providers.setdefault(symbol, set()).add((provider, addend, bool(public)))
    result = []
    for symbol, identities in sorted(providers.items()):
        if len(identities) != 1:
            continue
        provider, addend, public = next(iter(identities))
        if public and int(addend) == 0:
            result.append({"name": symbol[1:], "symbol": symbol, "provider": provider})
    if len(result) > 128:
        raise ValueError("SDK import request exceeds 128-symbol budget")
    return result


def collect_import_declarations(binary, report, header=DEFAULT_HEADER, sdk="macosx", target="arm64-apple-macos14.0"):
    """Collect SDK declarations for eligible imports of one verified capture."""
    digest = verify_macho_target(binary, target)
    if report.get("source_sha256") != digest:
        raise ValueError("analysis source differs from SDK declaration capture")
    candidates = sdk_import_candidates(report.get("linkage_records", ""))
    if not candidates:
        raise ValueError("no eligible public-system imports for SDK lookup")
    catalog = collect_declarations([item["name"] for item in candidates], header, sdk, target)
    identities = {item["name"]: item for item in candidates}
    for record in catalog["functions"]:
        identity = identities[record["name"]]
        if record["symbol"] != identity["symbol"]:
            raise ValueError("SDK declaration linkage differs from captured import")
        record["captured_provider"] = identity["provider"]
    catalog["source_sha256"] = digest
    catalog["import_candidates"] = candidates
    catalog["scope"] = "Public SDK declarations matched to captured import names/providers; runtime implementation identity and body equivalence remain unproven"
    return catalog


def verify_macho_target(path, target):
    """Keep a compiler's declarations bound to a compatible captured slice."""
    with Path(path).open("rb") as stream:
        before = os.fstat(stream.fileno())
        header = stream.read(32)
        if len(header) != 32 or header[:4] != bytes.fromhex("cffaedfe"):
            raise ValueError("prototype binding requires a thin little-endian 64-bit Mach-O slice")
        cpu, subtype, _, count, size = struct.unpack_from("<IIIII", header, 4)
        architecture = target.split("-", 1)[0]
        expected_cpu = 0x01000007 if architecture == "x86_64" else 0x0100000c
        if cpu != expected_cpu or (architecture == "arm64e" and subtype & 0xffffff != 2) or (architecture == "arm64" and subtype & 0xffffff == 2):
            raise ValueError("prototype target architecture differs from captured Mach-O")
        if count > 8192 or size > 16 * 1024 * 1024:
            raise ValueError("Mach-O load-command budget exceeded")
        commands = stream.read(size)
        if len(commands) != size:
            raise ValueError("truncated Mach-O load commands")
        stream.seek(0)
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
        after = os.fstat(stream.fileno())
        if (before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            raise ValueError("source changed during prototype binding")
    expected_platform = 7 if target.endswith("-simulator") else 2 if "-ios" in target else 1
    platforms = set()
    offset = 0
    for _ in range(count):
        if offset + 8 > size:
            raise ValueError("truncated Mach-O load command")
        command, length = struct.unpack_from("<II", commands, offset)
        if length < 8 or length % 8 or length > size - offset:
            raise ValueError("invalid Mach-O load-command framing")
        if command == 0x32:
            if length < 24:
                raise ValueError("truncated Mach-O build version")
            platforms.add(struct.unpack_from("<I", commands, offset + 8)[0])
        elif command in {0x24, 0x25}:
            if length != 16:
                raise ValueError("invalid Mach-O minimum version")
            platforms.add(1 if command == 0x24 else 2)
        offset += length
    if offset != size or platforms != {expected_platform}:
        raise ValueError("prototype target platform differs from or is absent in captured Mach-O")
    return digest


def _json_documents(text):
    decoder = json.JSONDecoder()
    cursor = 0
    while cursor < len(text):
        while cursor < len(text) and text[cursor].isspace():
            cursor += 1
        if cursor == len(text):
            return
        document, cursor = decoder.raw_decode(text, cursor)
        yield document


def _parameter(parameter, index, canonical):
    types = parameter["type"]
    type_name = types.get("desugaredQualType", types["qualType"]) if canonical else types["qualType"]
    name = parameter.get("name", f"arg{index}")
    # Full declarator trees remain in the catalog. The existing worker TSV
    # accepts simple prototypes only; callbacks and arrays must not be emitted
    # as a fabricated scalar ABI declaration.
    if any(character in type_name for character in "()[]{}\n\r\t;"):
        return None
    return f"{type_name} {name}"


def _parameter_abi_key(types):
    # Clang emits built-in and header redeclarations together. A parameter's
    # top-level restrict qualifier is omitted from the function type and does
    # not change its ABI. Keep pointed-to qualifiers and all other type shape.
    spelling = types.get("desugaredQualType", types["qualType"])
    return re.sub(r"\s*(?:\brestrict|\b__restrict|\b__restrict__)\s*$", "", spelling).strip()


def _declaration(node, canonical):
    function_type = node["type"]["qualType"]
    if "(" not in function_type or "__attribute__" in function_type:
        return None
    return_type = function_type.split("(", 1)[0].strip()
    if any(character in return_type for character in ";{}[]\n\r\t"):
        return None
    parameters = []
    for index, parameter in enumerate(child for child in node.get("inner", []) if child.get("kind") == "ParmVarDecl"):
        rendered = _parameter(parameter, index, canonical)
        if rendered is None:
            return None
        parameters.append(rendered)
    if node.get("variadic"):
        parameters.append("...")
    return f"{return_type} {node['name']}({', '.join(parameters) or 'void'});"


def collect_declarations(functions, header=DEFAULT_HEADER, sdk="macosx", target="arm64-apple-macos14.0"):
    if not functions or len(functions) > 128 or len(set(functions)) != len(functions):
        raise ValueError("request requires 1–128 distinct function names")
    if any(not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]{0,255}", name) for name in functions):
        raise ValueError("invalid C function name")
    if sdk not in {"macosx", "iphoneos", "iphonesimulator"}:
        raise ValueError("unsupported Apple SDK")
    if not re.fullmatch(r"(?:arm64|arm64e|x86_64)-apple-(?:macos|ios)[0-9.]+(?:-simulator)?", target):
        raise ValueError("invalid Apple target triple")
    expected = "iphonesimulator" if target.endswith("-simulator") else "iphoneos" if "-ios" in target else "macosx"
    if sdk != expected:
        raise ValueError("Apple SDK and target triple disagree")
    header = Path(header).resolve(strict=True)
    sdk_path = subprocess.run(["xcrun", "--sdk", sdk, "--show-sdk-path"], check=True,
                              capture_output=True, text=True, timeout=10).stdout.strip()
    compiler = subprocess.run(["xcrun", "--sdk", sdk, "clang", "--version"], check=True,
                              capture_output=True, text=True, timeout=10).stdout.splitlines()[0]
    sdk_build = subprocess.run(["xcrun", "--sdk", sdk, "--show-sdk-build-version"], check=True,
                               capture_output=True, text=True, timeout=10).stdout.strip()
    records, missing = [], []
    for name in functions:
        command = ["xcrun", "--sdk", sdk, "clang", "-target", target, "-isysroot", sdk_path,
                   "-x", "objective-c", "-std=c17", "-fblocks", "-fno-modules", "-fsyntax-only",
                   "-Xclang", "-ast-dump=json", "-Xclang", "-ast-dump-filter", "-Xclang", name, str(header)]
        # Clang's filter bounds output in normal use. Spool to disk rather
        # than allocating an arbitrarily large AST from substring matches.
        with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as diagnostics:
            result = subprocess.run(command, stdout=output, stderr=diagnostics, timeout=60)
            if result.returncode:
                diagnostics.seek(0)
                raise ValueError("Apple Clang rejected header: " + diagnostics.read(8192).decode(errors="replace"))
            output.seek(0)
            raw = output.read(MAX_AST_BYTES + 1)
            if len(raw) > MAX_AST_BYTES:
                raise ValueError("filtered Clang AST exceeds byte budget")
        matches = [node for node in _json_documents(raw.decode()) if node.get("kind") == "FunctionDecl" and node.get("name") == name]
        if not matches:
            missing.append(name)
            continue
        signatures = {(node.get("type", {}).get("qualType"), tuple(
            _parameter_abi_key(child["type"])
            for child in node.get("inner", []) if child.get("kind") == "ParmVarDecl")) for node in matches}
        if len(signatures) != 1:
            raise ValueError(f"conflicting declarations for {name}")
        node = matches[-1]
        stable_type = lambda types: {key: value for key, value in types.items() if key in {"qualType", "desugaredQualType"}}
        parameters = [{"name": child.get("name"), "type": stable_type(child["type"])} for child in node.get("inner", []) if child.get("kind") == "ParmVarDecl"]
        declaration = _declaration(node, False)
        abi_declaration = _declaration(node, True)
        symbol = node.get("mangledName", "_" + name)
        eligible = abi_declaration is not None and node.get("storageClass") != "static" and symbol.lstrip("_") == name
        records.append({"name": name, "symbol": symbol, "function_type": stable_type(node["type"]),
                        "parameters": parameters, "declaration": declaration, "abi_declaration": abi_declaration,
                        "variadic": bool(node.get("variadic")), "location": node.get("loc", {}),
                        "attributes": [child["kind"] for child in node.get("inner", []) if child.get("kind", "").endswith("Attr")],
                        "worker_eligible": eligible,
                        "limitation": None if eligible else "declarator/linkage requires structured type-library ingestion"})
    return {"schema": 1, "producer": "hindsight-apple-clang-declarations-v1", "sdk": sdk, "sdk_path": sdk_path,
            "target": target, "sdk_build": sdk_build, "compiler": compiler, "header": str(header),
            "header_sha256": hashlib.sha256(header.read_bytes()).hexdigest(), "functions": records,
            "missing_functions": missing, "scope": "SDK declaration evidence; does not establish runtime implementation identity"}


def worker_prototypes(catalog, source_sha256):
    if not re.fullmatch(r"[0-9a-f]{64}", source_sha256):
        raise ValueError("invalid source SHA-256")
    if catalog.get("missing_functions"):
        raise ValueError("cannot publish incomplete requested prototype set")
    lines = [f"# source_sha256={source_sha256}", f"# producer={catalog['producer']}",
             f"# target={catalog['target']}", f"# compiler={catalog['compiler']}", f"# sdk={catalog['sdk_path']}",
             "# Scope: SDK declarations; runtime provider identity must be qualified separately."]
    for record in catalog["functions"]:
        declaration = record["abi_declaration"]
        symbol = record["symbol"]
        if not record["worker_eligible"] or not declaration:
            raise ValueError(f"unsupported prototype declarator: {record['name']}")
        if len(declaration) > 1024 or len(symbol) > 1024 or any(ch in declaration + symbol for ch in "\n\r\t\\{}#=\""):
            raise ValueError(f"prototype exceeds worker declaration contract: {record['name']}")
        lines.append(f"{symbol}\t{declaration}")
    output = "\n".join(lines) + "\n"
    if len(output.encode()) > 4 * 1024 * 1024:
        raise ValueError("prototype catalog exceeds worker byte budget")
    return output


def main():
    parser = argparse.ArgumentParser(description="Match captured public-system imports against Apple SDK declarations.")
    parser.add_argument("binary", type=Path)
    parser.add_argument("analysis", type=Path, nargs="?", help="worker analysis JSON for the same captured slice")
    parser.add_argument("--worker", type=Path, help="analyze the capture directly instead of supplying analysis JSON")
    parser.add_argument("--sdk", choices=["macosx", "iphoneos", "iphonesimulator"], default="macosx")
    parser.add_argument("--target", default="arm64-apple-macos14.0")
    parser.add_argument("--header", type=Path, default=DEFAULT_HEADER)
    parser.add_argument("--worker-prototypes", action="store_true", help="emit source-bound TSV instead of the full evidence catalog; refuses missing/unsupported declarations")
    args = parser.parse_args()
    if (args.analysis is None) == (args.worker is None):
        parser.error("supply either analysis JSON or --worker, not both")
    if args.worker is not None:
        digest = verify_macho_target(args.binary, args.target)
        environment = dict(os.environ)
        environment.pop("HINDSIGHT_PROTOTYPE_DB", None)
        binary = args.binary.resolve(strict=True)
        result = subprocess.run([str(args.worker.resolve(strict=True)), "--analyze", str(binary), digest,
                                 str(binary.stat().st_size), str(uuid.uuid4())],
                                env=environment, check=True, capture_output=True, timeout=120)
        frame = result.stdout
        if len(frame) < 4 or len(frame) > 32 * 1024 * 1024 + 4 or struct.unpack(">I", frame[:4])[0] != len(frame)-4:
            raise ValueError("invalid bounded worker analysis frame")
        raw = frame[4:]
    else:
        with args.analysis.open("rb") as stream:
            raw = stream.read(32 * 1024 * 1024 + 1)
    if len(raw) > 32 * 1024 * 1024:
        raise ValueError("analysis report exceeds byte budget")
    report = json.loads(raw)
    if not isinstance(report, dict) or report.get("kind") != "analysis_result":
        raise ValueError("expected a worker analysis_result document")
    catalog = collect_import_declarations(args.binary, report, args.header, args.sdk, args.target)
    if args.worker_prototypes:
        sys.stdout.write(worker_prototypes(catalog, catalog["source_sha256"]))
    else:
        print(json.dumps(catalog, sort_keys=True, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"SDK declaration lookup failed: {error}", file=sys.stderr)
        raise SystemExit(1)
