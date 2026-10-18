"""Reference tooling for the reconstruction v1 artifact contract.

This is an offline artifact gate, not a decompiler or an equivalence solver.
Schema validation is limited deliberately to the local schema's vocabulary;
unknown schema keywords fail closed rather than being silently ignored.
"""

from __future__ import annotations

from collections import deque
import hashlib
import json
import math
from pathlib import Path
import re
import struct


SCHEMA_PATH = Path(__file__).resolve().parents[1] / "schemas/reconstruction.schema.json"
MAX_ARTIFACT_BYTES = 32 * 1024 * 1024
MAX_DEPTH = 64
MAX_VALUES = 2_000_000
MAX_QUERY_VISITS = 100_000


class ArtifactError(ValueError):
    """Invalid, inconsistent, or over-budget reconstruction artifact."""


def _require(condition, message):
    if not condition:
        raise ArtifactError(message)


def _walk_values(value):
    """Bound nesting and aggregate cost before recursive processing."""
    stack = [(value, 0)]
    count = 0
    while stack:
        item, depth = stack.pop()
        count += 1
        _require(depth <= MAX_DEPTH and count <= MAX_VALUES, "artifact nesting/value budget exceeded")
        if isinstance(item, dict):
            _require(all(isinstance(key, str) for key in item), "object keys must be strings")
            stack.extend((child, depth + 1) for child in item.values())
        elif isinstance(item, list):
            stack.extend((child, depth + 1) for child in item)
        else:
            _require(item is None or type(item) in (str, bool, int, float), "non-JSON value")
            if type(item) is float:
                _require(math.isfinite(item), "nonfinite JSON number")


def canonical_bytes(value):
    """HindSight content encoding v1: typed, length-framed, order-stable bytes.

    Null and booleans use n/f/t; integers use i + framed ASCII decimal; finite
    floats use d + IEEE754 binary64 big endian (negative zero normalized).
    Strings use s + framed UTF-8. Arrays and objects use a/o + uint64 element
    count; objects sort keys by UTF-8 bytes and encode each key then value.
    A frame is uint64 big-endian byte length followed by the bytes.
    """
    _walk_values(value)

    def framed(raw):
        return struct.pack(">Q", len(raw)) + raw

    def encode(item):
        if item is None:
            return b"n"
        if type(item) is bool:
            return b"t" if item else b"f"
        if type(item) is int:
            return b"i" + framed(str(item).encode("ascii"))
        if type(item) is float:
            # Normalize integral JSON numbers as integers so 1 and 1.0 have
            # the same identity across producers and parsers.
            if item.is_integer():
                return b"i" + framed(str(int(item)).encode("ascii"))
            return b"d" + struct.pack(">d", item)
        if isinstance(item, str):
            try:
                return b"s" + framed(item.encode("utf-8"))
            except UnicodeEncodeError as error:
                raise ArtifactError("invalid Unicode string") from error
        if isinstance(item, list):
            return b"a" + struct.pack(">Q", len(item)) + b"".join(encode(child) for child in item)
        keys = sorted(item, key=lambda key: key.encode("utf-8"))
        return b"o" + struct.pack(">Q", len(keys)) + b"".join(encode(key) + encode(item[key]) for key in keys)

    try:
        return encode(value)
    except (UnicodeError, ValueError, OverflowError) as error:
        raise ArtifactError("value cannot be encoded canonically") from error


def content_id(domain, source_sha256, function_id, record):
    """Bind a record's content (excluding its id) to its input and function."""
    body = {key: value for key, value in record.items() if key != "id"}
    payload = ["hindsight-content-v1", domain, source_sha256, function_id, body]
    return "sha256:" + hashlib.sha256(canonical_bytes(payload)).hexdigest()


def reconstruction_digest(artifact):
    """Digest the candidate independently of validation observations.

    Validation results cannot hash themselves. Eligibility is derived from
    results, so it is excluded as well. All source, function, evidence, AST,
    alternatives, settings, budgets, and completeness content remains bound.
    """
    candidate = {key: value for key, value in artifact.items() if key not in {"validation", "eligibility"}}
    return hashlib.sha256(canonical_bytes(["hindsight-candidate-v1", candidate])).hexdigest()


def _validate_schema(value, schema, root, path="$", depth=0):
    _require(depth <= MAX_DEPTH, f"{path}: schema nesting budget exceeded")
    allowed = {
        "$schema", "$id", "$defs", "$ref", "title", "description", "type", "const", "enum", "anyOf",
        "additionalProperties", "required", "properties", "items", "maxItems", "minItems", "uniqueItems",
        "maxLength", "minLength", "pattern", "maximum", "minimum", "maxProperties",
    }
    _require(not (set(schema) - allowed), f"{path}: unsupported schema vocabulary")
    if "$ref" in schema:
        reference = schema["$ref"]
        _require(reference.startswith("#/$defs/"), f"{path}: external schema reference forbidden")
        name = reference.removeprefix("#/$defs/")
        _require(name in root["$defs"], f"{path}: missing schema definition")
        _validate_schema(value, root["$defs"][name], root, path, depth + 1)
    if "anyOf" in schema:
        for alternative in schema["anyOf"]:
            try:
                _validate_schema(value, alternative, root, path, depth + 1)
                break
            except ArtifactError:
                pass
        else:
            raise ArtifactError(f"{path}: no permitted schema variant")
    matches = {
        "null": value is None, "boolean": type(value) is bool, "string": isinstance(value, str),
        "integer": type(value) is int or (type(value) is float and value.is_integer()),
        "number": type(value) in (int, float), "array": isinstance(value, list), "object": isinstance(value, dict),
    }
    if "type" in schema:
        types = schema["type"] if isinstance(schema["type"], list) else [schema["type"]]
        _require(any(matches.get(kind, False) for kind in types), f"{path}: invalid value type")
    if "const" in schema:
        _require(canonical_bytes(value) == canonical_bytes(schema["const"]), f"{path}: incorrect constant")
    if "enum" in schema:
        _require(any(canonical_bytes(value) == canonical_bytes(choice) for choice in schema["enum"]), f"{path}: unknown enum value")
    if isinstance(value, dict):
        _require(set(schema.get("required", [])) <= value.keys(), f"{path}: missing required fields")
        _require(len(value) <= schema.get("maxProperties", len(value)), f"{path}: property limit exceeded")
        properties = schema.get("properties", {})
        for key, child in value.items():
            child_schema = properties.get(key, schema.get("additionalProperties", True))
            _require(child_schema is not False, f"{path}.{key}: unknown field")
            if isinstance(child_schema, dict):
                _validate_schema(child, child_schema, root, f"{path}.{key}", depth + 1)
    if isinstance(value, list):
        _require(schema.get("minItems", 0) <= len(value) <= schema.get("maxItems", len(value)), f"{path}: array limit exceeded")
        if schema.get("uniqueItems"):
            _require(len({canonical_bytes(child) for child in value}) == len(value), f"{path}: duplicate array items")
        for index, child in enumerate(value):
            if "items" in schema:
                _validate_schema(child, schema["items"], root, f"{path}[{index}]", depth + 1)
    if isinstance(value, str):
        _require(schema.get("minLength", 0) <= len(value) <= schema.get("maxLength", len(value)), f"{path}: string limit exceeded")
        if "pattern" in schema:
            _require(re.fullmatch(schema["pattern"], value) is not None, f"{path}: malformed string")
    if type(value) in (int, float):
        _require(value >= schema.get("minimum", value) and value <= schema.get("maximum", value), f"{path}: number out of range")


class ReconstructionArtifact:
    """Validated artifact plus bounded, indexed evidence graph traversal."""

    def __init__(self, artifact):
        _walk_values(artifact)
        schema = json.loads(SCHEMA_PATH.read_text())
        _validate_schema(artifact, schema, schema)
        # Own the snapshot: callers cannot mutate a previously validated graph.
        self._data = json.loads(json.dumps(artifact, allow_nan=False))
        self._nodes = {}
        self._forward = {}
        self._reverse = {}
        self._validate_graph()

    @classmethod
    def load(cls, path):
        def unique_object(pairs):
            result = {}
            for key, value in pairs:
                _require(key not in result, f"duplicate JSON key: {key}")
                result[key] = value
            return result

        with Path(path).open("rb") as stream:
            raw = stream.read(MAX_ARTIFACT_BYTES + 1)
        _require(len(raw) <= MAX_ARTIFACT_BYTES, "artifact byte limit exceeded")
        try:
            document = json.loads(raw, object_pairs_hook=unique_object)
        except (ValueError, UnicodeError, RecursionError) as error:
            raise ArtifactError(f"invalid artifact JSON: {error}") from error
        return cls(document)

    @property
    def data(self):
        return json.loads(json.dumps(self._data))

    def _validate_graph(self):
        artifact = self._data
        source = artifact["source_sha256"]
        function = artifact["function"]
        function_id = function["id"]
        _require(function_id == content_id("function", source, None, function), "function content identity mismatch")
        start, end = int(function["start"], 16), int(function["end"], 16)
        _require(start < end, "invalid function range")

        def check_record(domain, record):
            _require(record["id"] == content_id(domain, source, function_id, record), f"{domain} content identity mismatch")

        def spans(record):
            for span in record["ranges"]:
                lo, hi = int(span["start"], 16), int(span["end"], 16)
                _require(lo < hi, "empty or reversed source span")
                # Local executable spans refer to this function. Metadata and
                # data nodes may refer elsewhere in the captured image.
                if record.get("kind") in {"decode", "ir-operation", "definition", "use", "phi", "cfg-block", "cfg-edge", "ast-node"}:
                    _require(start <= lo < hi <= end, "source span outside function")

        for node in artifact["nodes"]:
            check_record("evidence", node)
            _require(node["id"] not in self._nodes, "duplicate evidence identity")
            spans(node)
            self._nodes[node["id"]] = node
        edge_ids = set()
        for edge in artifact["edges"]:
            check_record("edge", edge)
            _require(edge["id"] not in edge_ids, "duplicate edge identity")
            edge_ids.add(edge["id"])
            _require(edge["source"] in self._nodes and edge["target"] in self._nodes, "dangling evidence edge")
            self._forward.setdefault(edge["source"], []).append(edge)
            self._reverse.setdefault(edge["target"], []).append(edge)

        ast = artifact["ast"]
        ast_nodes = {}
        for node in ast["nodes"]:
            check_record("ast", node)
            _require(node["id"] not in ast_nodes, "duplicate AST identity")
            _require(node["evidence"] and set(node["evidence"]) <= self._nodes.keys(), "missing AST evidence")
            _require(node["ranges"], "missing AST source spans")
            spans({**node, "kind": "ast-node"})
            ast_nodes[node["id"]] = node
        _require((ast["root"] is None and not ast_nodes) or ast["root"] in ast_nodes, "invalid AST root")
        for node in ast_nodes.values():
            _require(set(node["children"]) <= ast_nodes.keys(), "dangling AST child")
        # Iterative DFS accepts shared expressions but rejects cycles/orphans.
        seen, active = set(), set()
        stack = [(ast["root"], False)] if ast["root"] is not None else []
        while stack:
            identifier, leaving = stack.pop()
            if leaving:
                active.remove(identifier)
                seen.add(identifier)
            elif identifier in active:
                raise ArtifactError("cyclic source AST")
            elif identifier not in seen:
                active.add(identifier)
                stack.append((identifier, True))
                stack.extend((child, False) for child in reversed(ast_nodes[identifier]["children"]))
        _require(seen == ast_nodes.keys(), "orphaned AST node")

        for alternative in artifact["alternatives"]:
            check_record("alternative", alternative)
            _require(alternative["subject"] in self._nodes, "unknown alternative subject")
            candidates = alternative["candidates"]
            _require(len(set(candidates)) == len(candidates) and set(candidates) <= self._nodes.keys(), "invalid alternative candidates")
            _require(alternative["selected"] is None or alternative["selected"] in candidates, "selected alternative is not a candidate")
        for reason in artifact["eligibility"]["reasons"]:
            _require(set(reason["evidence"]) <= self._nodes.keys(), "unknown blocker evidence")

        budgets = artifact["budgets"]
        _require(budgets["limits"].keys() == budgets["consumed"].keys(), "budget limits/consumption mismatch")
        exceeded = {key for key, used in budgets["consumed"].items() if used > budgets["limits"][key]}
        exhausted = set(budgets["exhausted"])
        _require(exhausted <= budgets["limits"].keys() and exceeded <= exhausted, "unreported/unknown budget exhaustion")

        digest = reconstruction_digest(artifact)
        validation_ids = set()
        for run in artifact["validation"]:
            check_record("validation", run)
            _require(run["id"] not in validation_ids, "duplicate validation identity")
            validation_ids.add(run["id"])
            _require(run["artifact_sha256"] == digest, "validation applies to a different reconstruction")
            _require(run["status"] != "passed" or not run["mismatches"], "passing validation contains mismatches")
        self._validate_eligibility(ast, exhausted)

    def _validate_eligibility(self, ast, exhausted):
        artifact = self._data
        state = artifact["eligibility"]["state"]
        reasons = artifact["eligibility"]["reasons"]
        blockers = any(reason["blocking"] for reason in reasons)
        unsupported = any(node["kind"] == "unsupported-effect" for node in self._nodes.values())
        incomplete = artifact["truncated"] or exhausted or unsupported or blockers
        if state == "blocked":
            _require(blockers, "blocked artifact lacks a blocking reason")
        if state in {"review-only", "blocked"}:
            _require(not ast["compilable"], "review/blocked artifact claims compilability")
        if state in {"equivalent", "recompilable-with-intrinsics"}:
            _require(ast["compilable"] and ast["root"] is not None, "exportable artifact lacks a compilable AST")
            _require(not incomplete, "incomplete reconstruction cannot claim successful export/equivalence")
            passed = {run["kind"] for run in artifact["validation"] if run["status"] == "passed"}
            _require("recompile" in passed, "compilable claim lacks successful compilation")
            _require(not any(run["status"] == "failed" for run in artifact["validation"]), "successful eligibility contradicts failed validation")
            if state == "equivalent":
                _require({"cfg", "def-use", "memory-effects", "smt"} <= passed, "equivalence claim lacks structural and formal evidence")
                _require(not any(node["epistemic"] in {"assumption", "analyst-assertion"} for node in self._nodes.values()), "unqualified equivalence depends on unproven assertions")

    def query(self, identifier, relation=None, direction="incoming", max_depth=8, max_nodes=1000):
        """Return a deterministic bounded subgraph and explicit truncation.

        Incoming supports/derives/renders-as answer why; outgoing invalidates
        answers invalidation impact; contradicts and alternative-to expose
        competing claims. Direction is explicit; graph cycles are permitted.
        """
        _require(identifier in self._nodes, "query node is absent")
        _require(direction in {"incoming", "outgoing"}, "invalid query direction")
        _require(type(max_depth) is int and 0 <= max_depth <= 64, "invalid query depth")
        _require(type(max_nodes) is int and 1 <= max_nodes <= 10000, "invalid query node bound")
        adjacency = self._reverse if direction == "incoming" else self._forward
        endpoint = "source" if direction == "incoming" else "target"
        queue = deque([(identifier, 0)])
        visited, edges = {identifier}, {}
        truncated, scans = False, 0
        while queue:
            current, depth = queue.popleft()
            for edge in sorted(adjacency.get(current, []), key=lambda item: item["id"]):
                scans += 1
                if scans > MAX_QUERY_VISITS:
                    truncated = True
                    queue.clear()
                    break
                if relation is not None and edge["relation"] != relation:
                    continue
                neighbor = edge[endpoint]
                if neighbor not in visited:
                    if depth >= max_depth or len(visited) >= max_nodes:
                        truncated = True
                        continue
                    visited.add(neighbor)
                    queue.append((neighbor, depth + 1))
                edges[edge["id"]] = edge
        # Return copies so query results cannot mutate the validated snapshot.
        return json.loads(json.dumps({
            "nodes": [self._nodes[key] for key in sorted(visited)],
            "edges": [edges[key] for key in sorted(edges)],
            "truncated": truncated,
        }))
