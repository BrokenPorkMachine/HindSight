"""Bounded reconstruction-batch evidence and export validation."""
import hashlib
import json
import os
import re
import stat
import unicodedata

from reconstruction_artifact import ReconstructionArtifact


def read_bounded(path, limit):
    if type(limit) is not int or limit < 0:
        raise ValueError("invalid batch input byte limit")
    if not hasattr(os, "O_DIRECTORY") or not hasattr(os, "O_NOFOLLOW"):
        raise ValueError("platform lacks safe no-follow batch input support")
    directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    file_flags = os.O_RDONLY | os.O_NOFOLLOW
    directory_fd = os.open(path.parent, directory_flags)
    try:
        directory_info = os.fstat(directory_fd)
        if not stat.S_ISDIR(directory_info.st_mode):
            raise ValueError("batch input parent is not a directory")
        descriptor = os.open(path.name, file_flags, dir_fd=directory_fd)
        try:
            before = os.fstat(descriptor)
            if not stat.S_ISREG(before.st_mode) or before.st_size < 0 or before.st_size > limit:
                raise ValueError("batch input is not a bounded regular file")
            remaining = before.st_size
            chunks = []
            while remaining:
                chunk = os.read(descriptor, min(remaining, 1024 * 1024))
                if not chunk:
                    raise ValueError("batch input ended during reading")
                chunks.append(chunk)
                remaining -= len(chunk)
            after = os.fstat(descriptor)
            if ((after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns) !=
                    (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns)):
                raise ValueError("batch input changed during reading")
            return b"".join(chunks)
        finally:
            os.close(descriptor)
    finally:
        os.close(directory_fd)


def safe_sdk_function_declaration(value, name):
    if value is None:
        return True
    if (not isinstance(value, str) or not value or len(value.encode()) > 1024
            or not value.endswith(";") or any(unicodedata.category(char) == "Cc" for char in value)
            or any(char in value for char in "\\{}[]#=\"")):
        return False
    if value.count("(") != 1 or value.count(")") != 1:
        return False
    head, tail = value.split("(", 1)
    if not head.split() or head.split()[-1] != name or not tail.endswith(");"):
        return False
    return "(" not in tail and ")" not in tail[:-2] and ";" not in value[:-1]


def verified_outputs(directory, manifest):
    """Bind every exported byte to validated function/source evidence first."""
    sdk_catalog = manifest.get("sdk_catalog")
    if "sdk_catalog_sha256" in manifest or "sdk_catalog" in manifest:
        catalog = sdk_catalog
        if not isinstance(catalog, dict) or catalog.get("source_sha256") != manifest.get("source_sha256"):
            raise ValueError("SDK catalog source binding differs from batch")
        if catalog.get("producer") == "hindsight-apple-clang-declarations-v1":
            candidates = catalog.get("import_candidates")
            functions = catalog.get("functions")
            missing = catalog.get("missing_functions")
            if (type(catalog.get("schema")) is not int or catalog["schema"] != 1
                    or not re.fullmatch(r"[0-9a-f]{64}", str(catalog.get("header_sha256", "")))
                    or catalog.get("sdk") not in {"macosx", "iphoneos", "iphonesimulator"}
                    or not isinstance(catalog.get("target"), str)
                    or not re.fullmatch(r"(arm64|arm64e|x86_64)-apple-(macos|ios)[0-9]+\.[0-9]+\.[0-9]+(-simulator)?",
                                        catalog["target"])
                    or not isinstance(catalog.get("sdk_path"), str)
                    or not catalog["sdk_path"].startswith("/") or len(catalog["sdk_path"]) > 4096
                    or any(unicodedata.category(char) == "Cc" for char in catalog["sdk_path"])
                    or not isinstance(catalog.get("sdk_build"), str)
                    or not re.fullmatch(r"[A-Za-z0-9_.-]{1,128}", catalog["sdk_build"])
                    or not isinstance(catalog.get("compiler"), str)
                    or not catalog["compiler"] or len(catalog["compiler"].encode()) > 512
                    or any(unicodedata.category(char) == "Cc" for char in catalog["compiler"])
                    or not isinstance(candidates, list) or not 1 <= len(candidates) <= 128
                    or not isinstance(functions, list) or len(functions) > 128
                    or not isinstance(missing, list) or len(missing) > 128):
                raise ValueError("SDK catalog schema or declaration bounds are invalid")
            target_match = re.fullmatch(
                r"(arm64|arm64e|x86_64)-apple-(macos|ios)[0-9]+\.[0-9]+\.[0-9]+(-simulator)?",
                catalog["target"])
            platform, simulator = target_match.group(2), target_match.group(3) is not None
            sdk_leaf = catalog["sdk_path"].rsplit("/", 1)[-1]
            if ((catalog["sdk"] == "macosx") != (platform == "macos")
                    or (catalog["sdk"] == "iphoneos") != (platform == "ios" and not simulator)
                    or (catalog["sdk"] == "iphonesimulator") != (platform == "ios" and simulator)
                    or (platform == "ios" and not simulator and target_match.group(1) == "x86_64")
                    or sdk_leaf != {"macosx": "MacOSX.sdk", "iphoneos": "iPhoneOS.sdk",
                                    "iphonesimulator": "iPhoneSimulator.sdk"}[catalog["sdk"]]):
                raise ValueError("SDK catalog target and SDK identity disagree")
            candidate_symbols = {}
            for candidate in candidates:
                if (not isinstance(candidate, dict) or not all(isinstance(candidate.get(key), str) and candidate[key]
                        for key in ("name", "symbol", "provider"))
                        or not re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", candidate["name"])
                        or len(candidate["name"]) > 256 or len(candidate["symbol"]) > 1024
                        or candidate["symbol"] != "_" + candidate["name"]
                        or not (re.fullmatch(r"/usr/lib/(?:system/)?lib[A-Za-z0-9_.+-]+\.dylib", candidate["provider"])
                                or re.fullmatch(r"/System/Library/Frameworks/([A-Za-z][A-Za-z0-9_]*)\.framework/(?:Versions/[A-Za-z0-9]+/)?\1", candidate["provider"]))
                        or candidate["name"] in candidate_symbols):
                    raise ValueError("SDK catalog import candidates are invalid")
                candidate_symbols[candidate["name"]] = candidate["symbol"]
            function_symbols = {}
            for function in functions:
                if (not isinstance(function, dict) or not isinstance(function.get("name"), str)
                        or not re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", function["name"])
                        or len(function["name"]) > 256
                        or not isinstance(function.get("symbol"), str)
                        or len(function["symbol"]) > 1024
                        or function["name"] in function_symbols or type(function.get("worker_eligible")) is not bool
                        or not safe_sdk_function_declaration(function.get("declaration"), function["name"])
                        or not safe_sdk_function_declaration(function.get("abi_declaration"), function["name"])
                        or (function.get("worker_eligible") and not function.get("abi_declaration"))):
                    raise ValueError("SDK catalog function declarations are invalid")
                function_symbols[function["name"]] = function.get("symbol")
            if (any(not isinstance(name, str) or len(name) > 256
                    or not re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", name)
                    for name in missing) or len(set(missing)) != len(missing)
                    or set(function_symbols) & set(missing)
                    or set(candidate_symbols) != set(function_symbols) | set(missing)
                    or any(candidate_symbols[name] != symbol for name, symbol in function_symbols.items())):
                raise ValueError("SDK catalog declarations do not match their import candidates")
        raw = json.dumps(catalog, sort_keys=True, separators=(",", ":")).encode()
        if len(raw) > 4 * 1024 * 1024 or hashlib.sha256(raw).hexdigest() != manifest.get("sdk_catalog_sha256"):
            raise ValueError("SDK evidence catalog hash or budget mismatch")
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
        settings_payloads = [node["payload"]["analysis_settings"]
                             for node in artifact.get("nodes", [])
                             if isinstance(node, dict) and isinstance(node.get("payload"), dict)
                             and isinstance(node["payload"].get("analysis_settings"), dict)]
        call_candidates = []
        for settings in settings_payloads:
            raw_candidates = settings.get("call_signature_candidates", [])
            if (not isinstance(raw_candidates, list) or len(raw_candidates) > 100_000
                    or len(call_candidates) > 100_000 - len(raw_candidates)):
                raise ValueError("invalid call signature candidate collection")
            call_candidates.extend(raw_candidates)
        for candidate in call_candidates:
            if (not isinstance(candidate, dict)
                    or set(candidate) != {"target", "symbol", "language", "declaration", "source"}
                    or not isinstance(candidate.get("target"), str)
                    or not re.fullmatch(r"0x[0-9a-fA-F]{1,16}", candidate["target"])
                    or not isinstance(candidate.get("symbol"), str) or not candidate["symbol"]
                    or len(candidate["symbol"].encode()) > 1024
                    or any(unicodedata.category(char) == "Cc" for char in candidate["symbol"])
                    or not isinstance(candidate.get("language"), str) or not candidate["language"]
                    or len(candidate["language"].encode()) > 64
                    or not isinstance(candidate.get("declaration"), str) or not candidate["declaration"]
                    or len(candidate["declaration"].encode()) > 2048
                    or any(unicodedata.category(char) == "Cc" for char in candidate["declaration"])
                    or not isinstance(candidate.get("source"), str) or not candidate["source"]
                    or len(candidate["source"].encode()) > 512
                    or any(unicodedata.category(char) == "Cc" for char in candidate["source"])):
                raise ValueError("invalid call signature candidate")
            source = candidate["source"]
            if "hash-bound Apple SDK declaration catalog" not in source:
                continue
            if candidate["language"] != "C":
                raise ValueError("SDK catalog provenance is only valid for C declarations")
            if not isinstance(sdk_catalog, dict):
                raise ValueError("SDK-derived call declaration lacks its retained catalog")
            symbol, declaration = candidate.get("symbol"), candidate.get("declaration")
            functions = sdk_catalog.get("functions")
            if (not isinstance(symbol, str) or not isinstance(declaration, str)
                    or not isinstance(functions, list) or len(functions) > 128
                    or not any(isinstance(function, dict) and function.get("worker_eligible") is True
                               and function.get("symbol") == symbol
                               and function.get("abi_declaration") == declaration
                               for function in functions)):
                raise ValueError("SDK catalog does not support an attributed call declaration")
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
