"""Bounded reconstruction-batch evidence and export validation."""
import hashlib
import json
import re

from reconstruction_artifact import ReconstructionArtifact


def read_bounded(path, limit):
    with path.open("rb") as source:
        raw = source.read(limit + 1)
    if len(raw) > limit:
        raise ValueError("batch input exceeds byte budget")
    return raw


def verified_outputs(directory, manifest):
    """Bind every exported byte to validated function/source evidence first."""
    seen = set()
    starts = set()
    total = 0
    result = []
    for entry in manifest["functions"]:
        start = int(entry["start"], 16)
        end = int(entry["end"], 16)
        if not 0 <= start < end <= 2**64 - 1:
            raise ValueError("invalid function bounds")
        if start in starts or entry["state"] not in {"recovered", "review-only", "failed"}:
            raise ValueError("duplicate function or unsupported recovery state")
        starts.add(start)
        prefix = f"function-{start:016x}"
        files = {}
        if len(entry["files"]) > 5:
            raise ValueError("too many function outputs")
        for exported in entry["files"]:
            name = exported["path"]
            if name in seen or not re.fullmatch(re.escape(prefix) + r"\.(?:c|h|m|machine\.c|reconstruction\.json)", name):
                raise ValueError("unsafe, duplicate or mismatched function output path")
            seen.add(name)
            size = exported["bytes"]
            limit = 16 * 1024 * 1024 if name.endswith(".json") else 1024 * 1024
            if type(size) is not int or not 0 < size <= limit:
                raise ValueError("function output exceeds byte budget")
            total += size
            if total > 256 * 1024 * 1024:
                raise ValueError("batch output exceeds byte budget")
            path = directory / name
            if path.is_symlink() or not path.is_file() or path.stat().st_size != size:
                raise ValueError("missing or changed batch output")
            raw = read_bounded(path, size)
            if len(raw) != size or hashlib.sha256(raw).hexdigest() != exported["sha256"]:
                raise ValueError("batch output hash mismatch")
            files[name] = raw
        if not files and entry["state"] == "failed":
            result.append((entry, files))
            continue
        artifact_name = prefix + ".reconstruction.json"
        if artifact_name not in files:
            raise ValueError("function reconstruction evidence is missing")
        artifact = json.loads(files[artifact_name])
        ReconstructionArtifact(artifact)
        if (artifact["source_sha256"] != manifest["source_sha256"] or
                int(artifact["function"]["start"], 16) != start or
                int(artifact["function"]["end"], 16) != int(entry["end"], 16)):
            raise ValueError("function reconstruction identity mismatch")
        expected = {artifact_name: files[artifact_name]}
        kinds = {"recovered-source": ".c", "recovered-objective-c": ".m", "best-effort-source": ".c", "recovered-header": ".h", "best-effort-header": ".h", "machine-state-c": ".machine.c"}
        has_recovered_source = False
        for node in artifact["nodes"]:
            payload = node["payload"]
            kind = payload.get("artifact_kind")
            has_recovered_source |= kind == "recovered-source"
            if kind in {"recovered-header", "best-effort-header"} and "languages" in payload:
                languages = payload["languages"]
                if (not isinstance(languages, list) or not languages or
                        any(not isinstance(language, str) or language not in
                            {"c", "c++", "objective-c", "objective-c++"} for language in languages) or
                        len(set(languages)) != len(languages)):
                    raise ValueError("invalid header language contract")
            if kind in kinds:
                name = prefix + kinds[kind]
                if name in expected:
                    raise ValueError("ambiguous duplicate generated output kind")
                expected[name] = payload["source"].encode("utf-8")
        if any(name not in expected or raw != expected[name] for name, raw in files.items()):
            raise ValueError("export differs from validated reconstruction evidence")
        if entry["state"] != "failed":
            state = "recovered" if has_recovered_source else "review-only"
            if files != expected or entry["state"] != state:
                raise ValueError("incomplete outputs or recovery state disagrees with evidence")
        result.append((entry, files))
    return result



def read_batch_manifest(directory):
    path = directory / "manifest.json"
    if directory.is_symlink() or path.is_symlink() or not path.is_file():
        raise ValueError("unsafe or missing batch manifest")
    raw = read_bounded(path, 16 * 1024 * 1024)
    manifest = json.loads(raw)
    if (not isinstance(manifest, dict) or manifest.get("schema") != 1 or
            not isinstance(manifest.get("functions"), list) or not 1 <= len(manifest["functions"]) <= 4096 or
            not isinstance(manifest.get("source_sha256"), str) or not re.fullmatch(r"[0-9a-f]{64}", manifest["source_sha256"])):
        raise ValueError("unsupported batch manifest")
    return manifest, raw
