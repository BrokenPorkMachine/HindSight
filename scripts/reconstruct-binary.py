#!/usr/bin/env python3
"""Export independent worker reconstructions and a binary/range coverage manifest."""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import tempfile
import uuid

from reconstruction_artifact import ReconstructionArtifact
from reconstruction_batch import read_bounded, verified_outputs


def file_hash(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def publish(path, value):
    """Replace only the batch's own metadata after flushing its new contents."""
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".batch-", mode="w", delete=False) as stream:
        temporary = Path(stream.name)
        json.dump(value, stream, sort_keys=True, indent=2)
        stream.flush()
        os.fsync(stream.fileno())
    try:
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def request(worker, binary, digest, size, command, arguments, prototypes):
    environment = dict(os.environ)
    environment.pop("HINDSIGHT_PROTOTYPE_DB", None)
    if prototypes:
        environment["HINDSIGHT_PROTOTYPE_DB"] = str(prototypes)
    result = subprocess.run(
        [str(worker), command, str(binary), digest, str(size), *arguments, str(uuid.uuid4())],
        check=True, capture_output=True, timeout=120, env=environment)
    raw = result.stdout
    if len(raw) < 4 or len(raw) > 32 * 1024 * 1024 + 4:
        raise ValueError("worker reply exceeds frame limits")
    if struct.unpack_from(">I", raw)[0] != len(raw) - 4:
        raise ValueError("worker reply frame length mismatch")
    return json.loads(raw[4:])


def run(args):
    if not getattr(args, "sdk_imports", False):
        return run_batch(args)
    if args.prototypes:
        raise ValueError("--sdk-imports cannot be combined with --prototypes")
    if args.output.is_symlink() or (not args.resume and args.output.exists()):
        raise ValueError("choose a new regular output directory; existing output is preserved")
    from darwin_sdk_catalog import collect_import_declarations, worker_prototypes
    binary, worker = args.binary.resolve(), args.worker.resolve()
    if args.binary.is_symlink() or not binary.is_file():
        raise ValueError("input must be a regular non-symlink file")
    digest = file_hash(binary)
    report = request(worker, binary, digest, binary.stat().st_size, "--analyze", [], None)
    catalog = collect_import_declarations(binary, report, sdk=args.sdk, target=args.sdk_target)
    eligible = [record for record in catalog["functions"] if record["worker_eligible"] and record["abi_declaration"]]
    # Missing or structured declarations stay in the evidence catalog. Only
    # eligible declarations enter the worker's bounded scalar TSV interface.
    with tempfile.TemporaryDirectory(prefix="hindsight-sdk-prototypes-") as temporary:
        prepared = copy.copy(args)
        prepared.prototypes = None
        if eligible:
            prepared.prototypes = Path(temporary) / "prototypes.tsv"
            prepared.prototypes.write_text(worker_prototypes({**catalog, "functions": eligible, "missing_functions": []}, digest))
        return run_batch(prepared, report=report, sdk_catalog=catalog)


def run_batch(args, report=None, sdk_catalog=None):
    binary, worker = args.binary.resolve(), args.worker.resolve()
    if not binary.is_file() or args.binary.is_symlink():
        raise ValueError("input must be a regular non-symlink file")
    with binary.open("rb") as captured:
        digest = hashlib.file_digest(captured, "sha256").hexdigest()
    size = binary.stat().st_size
    if report is None:
        report = request(worker, binary, digest, size, "--analyze", [], args.prototypes)
    if report.get("source_sha256") != digest or not isinstance(report.get("functions"), str):
        raise ValueError("analysis does not match captured source")
    all_functions = []
    for line in report["functions"].splitlines():
        match = re.match(r"^(0x[0-9a-f]+)\s+(0x[0-9a-f]+)\s+\d+\s+bytes\s+(.+)$", line)
        if match:
            start, end = int(match[1], 16), int(match[2], 16)
            if start < end:
                all_functions.append((start, end, match[3]))
    all_functions = sorted(set(all_functions))
    by_start = {function[0]: function for function in all_functions}
    if len(by_start) != len(all_functions):
        raise ValueError("ambiguous discovered function starts")
    functions = [function for function in all_functions
                 if (args.start is None or function[0] >= args.start)
                 and (args.end is None or function[1] <= args.end)]
    if not functions:
        raise ValueError("no discovered functions are fully contained in the requested scope")
    selected = functions[:args.max_functions]
    identity = {"source_sha256": digest, "source_bytes": size,
                "worker_sha256": file_hash(worker),
                "prototypes_sha256": file_hash(args.prototypes) if args.prototypes else None,
                "include_callees": args.include_callees,
                "closure_function_limit": args.max_functions if args.include_callees else None,
                "scope": {"start": args.start, "end": args.end}}
    if sdk_catalog is not None:
        if sdk_catalog.get("source_sha256") != digest:
            raise ValueError("SDK declarations do not match captured source")
        catalog_bytes = json.dumps(sdk_catalog, sort_keys=True, separators=(",", ":")).encode()
        if len(catalog_bytes) > 4 * 1024 * 1024:
            raise ValueError("SDK evidence catalog exceeds byte budget")
        identity["sdk_catalog_sha256"] = hashlib.sha256(catalog_bytes).hexdigest()
    if args.output.is_symlink():
        raise ValueError("output directory must not be a symlink")
    if not args.resume:
        args.output.mkdir(parents=False, exist_ok=False)
    entries, output_bytes = [], 0
    if args.resume:
        checkpoint = args.output / "checkpoint.json"
        if checkpoint.is_symlink() or not checkpoint.is_file() or checkpoint.stat().st_size > 16 * 1024 * 1024:
            raise ValueError("missing or unsafe resume checkpoint")
        retained = json.loads(read_bounded(checkpoint, 16 * 1024 * 1024))
        if (retained.get("schema") != 1 or any(retained.get(key) != value for key, value in identity.items())
                or retained.get("sdk_catalog_sha256") != identity.get("sdk_catalog_sha256")):
            raise ValueError("resume source, worker, prototype catalog or scope identity mismatch")
        if args.include_callees:
            scheduled = [tuple(value) for value in retained["scheduled"]]
            if len(scheduled) > args.max_functions or len(set(scheduled)) != len(scheduled) or (
                    scheduled[:len(selected)] != selected) or any(
                    by_start.get(value[0]) != value for value in scheduled):
                raise ValueError("resume dependency queue does not match discovery")
            selected = scheduled
        entries = retained["functions"]
        if len(entries) > len(selected):
            raise ValueError("resume function limit is smaller than retained progress")
        # Hashes alone can be reissued after an edit. Validate the retained
        # graph, exact generated representations and recovery state before
        # publishing a new checkpoint or requesting any reconstruction work.
        validated = verified_outputs(args.output, retained)
        output_bytes = sum(len(raw) for _, files in validated for raw in files.values())
        for entry, (start, end, name) in zip(entries, selected):
            if (entry.get("start"), entry.get("end"), entry.get("name")) != (hex(start), hex(end), name):
                raise ValueError("resume function discovery changed")
        if output_bytes > args.max_bytes:
            raise ValueError("resume outputs exceed requested byte budget")

    def state():
        return {"schema": 1, **identity, **({"sdk_catalog": sdk_catalog} if sdk_catalog is not None else {}), "discovered_in_scope": len(functions),
                "recovered_functions": sum(entry["state"] == "recovered" for entry in entries),
                "review_only_functions": sum(entry["state"] == "review-only" for entry in entries),
                "failed_functions": sum(entry["state"] == "failed" for entry in entries),
                "attempted": len(entries), "omitted_by_limit": sum(
                    function[0] not in {int(entry["start"], 16) for entry in entries} for function in functions),
                "scheduled": selected,
                "analysis_scope": "worker bounded discovery; undiscovered functions remain unknown",
                "validation": "generated output; compilation and behavioral equivalence unclaimed",
                "output_bytes": output_bytes, "functions": entries}

    publish(args.output / "checkpoint.json", state())
    retained_count = len(entries)
    for index, (start, end, name) in enumerate(selected):
        if index < retained_count:
            print(f"{index + 1}/{len(selected)} {hex(start)} retained", flush=True)
            continue
        entry = {"start": hex(start), "end": hex(end), "name": name, "files": []}
        try:
            artifact = request(worker, binary, digest, size, "--reconstruct",
                               [hex(start), hex(end)], args.prototypes)
            ReconstructionArtifact(artifact)
            if artifact["source_sha256"] != digest or int(artifact["function"]["start"], 16) != start or int(
                    artifact["function"]["end"], 16) != end:
                raise ValueError("reconstruction source/function identity mismatch")
            dependencies = set()
            indirect_calls = 0
            for node in artifact["nodes"]:
                if node["kind"] != "ir-operation":
                    continue
                pending = [node["payload"].get("tree")]
                while pending:
                    tree = pending.pop()
                    if not isinstance(tree, dict):
                        continue
                    if tree.get("kind") == "call":
                        dependencies.add(int(tree["attributes"]["target"], 16))
                    elif tree.get("kind") == "indirect-call":
                        indirect_calls += 1
                    pending.extend(tree.get("children", []))
            entry["direct_dependencies"] = [hex(value) for value in sorted(dependencies)]
            entry["unresolved_indirect_calls"] = indirect_calls
            entry["unresolved_dependencies"] = []
            if args.include_callees:
                scheduled_starts = {value[0] for value in selected}
                for target in sorted(dependencies):
                    if target in scheduled_starts:
                        continue
                    if target not in by_start or len(selected) >= args.max_functions:
                        entry["unresolved_dependencies"].append(hex(target))
                    else:
                        selected.append(by_start[target])
                        scheduled_starts.add(target)
            prefix = f"function-{start:016x}"
            outputs = [(prefix + ".reconstruction.json", json.dumps(artifact, sort_keys=True))]
            kinds = {"recovered-source": ".c", "recovered-objective-c": ".m", "best-effort-source": ".c", "recovered-header": ".h", "best-effort-header": ".h", "machine-state-c": ".machine.c"}
            seen = set()
            for node in artifact["nodes"]:
                payload = node["payload"]
                kind = payload.get("artifact_kind")
                if kind in kinds:
                    if kind in seen:
                        raise ValueError("ambiguous duplicate generated output kind")
                    seen.add(kind)
                    outputs.append((prefix + kinds[kind], payload["source"]))
            byte_count = sum(len(text.encode()) for _, text in outputs)
            if len({name for name, _ in outputs}) != len(outputs):
                raise ValueError("competing source representations have the same output path")
            if output_bytes + byte_count > args.max_bytes:
                raise ValueError("batch output byte budget exhausted")
            for filename, text in outputs:
                data = text.encode()
                path = args.output / filename
                if args.resume and path.exists():
                    # A crash can leave a complete file before its checkpoint.
                    # Reuse only bytes identical to freshly validated output.
                    if path.is_symlink() or not path.is_file() or path.stat().st_size != len(data) or (
                            file_hash(path) != hashlib.sha256(data).hexdigest()):
                        raise ValueError("uncheckpointed output differs from regenerated content")
                else:
                    with path.open("xb") as destination:
                        destination.write(data)
                entry["files"].append({"path": filename, "bytes": len(data),
                                       "sha256": hashlib.sha256(data).hexdigest()})
                output_bytes += len(data)
            entry["state"] = "recovered" if "recovered-source" in seen else "review-only"
            entry["eligibility"] = artifact["eligibility"]
            entry["source_blockers"] = [
                node["payload"].get("reason", "source recovery unresolved")
                for node in artifact["nodes"]
                if node["payload"].get("artifact_kind") == "recovered-source-rejection"
            ]
        except (ValueError, OSError, subprocess.SubprocessError) as error:
            entry["state"] = "failed"
            entry["error"] = str(error)[:4096]
        entries.append(entry)
        publish(args.output / "checkpoint.json", state())
        print(f"{index + 1}/{len(selected)} {hex(start)} {entry['state']}", flush=True)
    manifest = state()
    manifest["status"] = "completed-with-failures" if manifest["failed_functions"] else "completed"
    publish(args.output / "manifest.json", manifest)
    print(json.dumps({key: manifest[key] for key in ["status", "attempted", "recovered_functions",
                                                    "review_only_functions", "failed_functions", "omitted_by_limit"]}), flush=True)
    return manifest


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("output", type=Path, help="new output directory; existing paths are refused")
    parser.add_argument("--worker", type=Path, required=True)
    parser.add_argument("--resume", action="store_true", help="verify retained outputs and continue this batch")
    parser.add_argument("--include-callees", action="store_true",
                        help="include direct callees discovered elsewhere in the captured image")
    parser.add_argument("--prototypes", type=lambda value: Path(value).resolve())
    parser.add_argument("--sdk-imports", action="store_true", help="collect applicable public SDK import declarations and retain lookup evidence")
    parser.add_argument("--sdk", choices=["macosx", "iphoneos", "iphonesimulator"], default="macosx")
    parser.add_argument("--sdk-target", default="arm64-apple-macos14.0")
    parser.add_argument("--start", type=lambda value: int(value, 0))
    parser.add_argument("--end", type=lambda value: int(value, 0))
    parser.add_argument("--max-functions", type=int, default=4096)
    parser.add_argument("--max-bytes", type=int, default=256 * 1024 * 1024)
    args = parser.parse_args()
    if not 1 <= args.max_functions <= 4096 or not 1 <= args.max_bytes <= 1024 * 1024 * 1024:
        parser.error("function/byte budget out of range")
    if (args.start is None) != (args.end is None) or (
            args.start is not None and not 0 <= args.start < args.end <= 2**64 - 1):
        parser.error("range requires both --start and --end with start < end")
    manifest = run(args)
    raise SystemExit(1 if manifest["failed_functions"] else 0)
