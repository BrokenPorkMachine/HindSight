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
MAX_SETTINGS_KEYS = 128
SETTINGS_KEY = re.compile(r"[a-z][a-z0-9_]{0,63}")
PASS_VERSION = re.compile(r"[a-z0-9][a-z0-9.+-]{0,63}")
# Record kinds whose source spans necessarily refer to the enclosing function.
# Metadata, data, symbol and external evidence may legitimately refer elsewhere
# in the captured image.
LOCAL_SPAN_KINDS = {"decode", "ir-operation", "definition", "use", "phi",
                    "cfg-block", "cfg-edge", "ast-node"}


class AnalysisScope:
    """Identity scope shared by every record in one reconstruction run.

    Records are only comparable within a scope. Two runs that share a binary
    and a function still produce different identities when they ran a
    different pass version or under different analysis settings, so a cached
    or replayed artifact cannot silently answer for the wrong analysis.
    """

    __slots__ = ("pass_version", "settings_digest")

    def __init__(self, pass_version, settings_digest):
        _require(isinstance(pass_version, str) and PASS_VERSION.fullmatch(pass_version) is not None,
                 "malformed analysis pass version")
        _require(isinstance(settings_digest, str) and re.fullmatch(r"sha256:[0-9a-f]{64}", settings_digest) is not None,
                 "malformed analysis settings digest")
        self.pass_version = pass_version
        self.settings_digest = settings_digest

    @classmethod
    def from_settings(cls, pass_version, settings):
        """Derive the scope from the settings that actually shaped the run."""
        return cls(pass_version, settings_digest(settings))

    def key(self):
        return [self.pass_version, self.settings_digest]

    def __eq__(self, other):
        return isinstance(other, AnalysisScope) and self.key() == other.key()

    def __hash__(self):
        return hash(tuple(self.key()))

    def __repr__(self):
        return f"AnalysisScope(pass_version={self.pass_version!r}, settings_digest={self.settings_digest!r})"


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


def settings_digest(settings):
    """Content-derived identity of the analysis settings that shaped a run.

    Settings are recorded as well as digested: the digest binds every identity
    in the artifact, while the retained value keeps the run reproducible.
    """
    _require(isinstance(settings, dict), "analysis settings must be an object")
    _require(0 < len(settings) <= MAX_SETTINGS_KEYS, "analysis settings key limit exceeded")
    for key in settings:
        _require(SETTINGS_KEY.fullmatch(key) is not None, f"malformed analysis settings key: {key}")
    return "sha256:" + hashlib.sha256(canonical_bytes(["hindsight-settings-v1", settings])).hexdigest()


def content_id(domain, source_sha256, function_id, record, scope):
    """Bind a record's content to its binary, function, pass, and settings.

    The scope is part of the identity, not merely metadata: without it, two
    passes that emit byte-identical records for one function would share an
    identity, and a consumer could not tell which analysis produced an answer.
    """
    _require(domain, "identity domain is required")
    _require(scope is not None, "identity scope is required")
    body = {key: value for key, value in record.items() if key != "id"}
    payload = ["hindsight-content-v2", domain, source_sha256, function_id, scope.key(), body]
    return "sha256:" + hashlib.sha256(canonical_bytes(payload)).hexdigest()


def compute_eligibility(artifact):
    """Derive per-function eligibility from an artifact's completeness state.

    Incomplete or unsupported analyses cannot claim export/equivalence. The
    result is a candidate for the artifact's ``eligibility`` field and is
    validated by ``ReconstructionArtifact`` before acceptance.
    """
    nodes = artifact.get("nodes", [])
    ast = artifact.get("ast", {}) or {}
    reasons = []
    truncated = bool(artifact.get("truncated", False))
    exhausted = bool((artifact.get("budgets") or {}).get("exhausted"))
    unsupported = [node for node in nodes if node.get("kind") == "unsupported-effect"]
    validation = artifact.get("validation", []) or []
    has_failed = any(run.get("status") == "failed" for run in validation)
    passed = {run.get("kind") for run in validation if run.get("status") == "passed"}

    if truncated:
        reasons.append({"code": "truncated", "message": "analysis was truncated", "blocking": True,
                       "evidence": []})
    if exhausted:
        reasons.append({"code": "budget-exhausted", "message": "analysis budget was exhausted", "blocking": True,
                       "evidence": []})
    for node in unsupported:
        reasons.append({"code": "unsupported-effect", "message": "unsupported effect blocks export", "blocking": True,
                       "evidence": [node["id"]]})
    if has_failed:
        reasons.append({"code": "validation-failed", "message": "validation run failed", "blocking": True,
                       "evidence": []})

    compilable = bool(ast.get("compilable")) and ast.get("root") is not None
    blocking = any(reason["blocking"] for reason in reasons)

    if blocking:
        state = "blocked"
    elif not compilable:
        state = "review-only"
    elif {"cfg", "def-use", "memory-effects", "smt"} <= passed and not any(
            node.get("epistemic") in {"assumption", "analyst-assertion"} for node in nodes):
        state = "equivalent"
    else:
        state = "recompilable-with-intrinsics"

    return {"state": state, "reasons": reasons}


def reconstruction_digest(artifact):
    """Digest the candidate independently of validation observations.

    Validation results cannot hash themselves. Eligibility is derived from
    results, so it is excluded as well. The scope, source, function, evidence,
    AST, alternatives, settings, budgets, and completeness content remains
    bound, so two runs that differ only in pass version or settings digest
    differently and cannot reuse each other's validation results.
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
        scope_record = artifact["scope"]
        self.scope = AnalysisScope(scope_record["pass_version"], scope_record["settings_digest"])
        function = artifact["function"]
        function_id = function["id"]
        _require(function_id == content_id("function", source, None, function, self.scope), "function content identity mismatch")
        start, end = int(function["start"], 16), int(function["end"], 16)
        _require(start < end, "invalid function range")

        def check_record(domain, record):
            _require(record["id"] == content_id(domain, source, function_id, record, self.scope), f"{domain} content identity mismatch")

        def spans(record):
            claimed = []
            for span in record["ranges"]:
                lo, hi = int(span["start"], 16), int(span["end"], 16)
                _require(lo < hi, "empty or reversed source span")
                # A record cannot claim the same bytes twice or overlap itself:
                # duplicated spans would inflate apparent evidence coverage
                # without adding an independent observation.
                _require(not any(lo < other_hi and other_lo < hi for other_lo, other_hi in claimed),
                         "overlapping source span within one record")
                claimed.append((lo, hi))
                # Local executable spans refer to this function. Metadata and
                # data nodes may refer elsewhere in the captured image.
                if record.get("kind") in LOCAL_SPAN_KINDS:
                    _require(start <= lo < hi <= end, "source span outside function")

        # One identity namespace spans every record kind. Two records that hash alike
        # are the same record even if they sit in different arrays, so uniqueness
        # is checked globally rather than once per kind.
        identities = {}

        def unique(identifier, domain):
            _require(identifier not in identities, f"duplicate {domain} identity")
            identities[identifier] = domain

        for node in artifact["nodes"]:
            check_record("evidence", node)
            unique(node["id"], "evidence")
            spans(node)
            self._nodes[node["id"]] = node
        for edge in artifact["edges"]:
            check_record("edge", edge)
            unique(edge["id"], "edge")
            _require(edge["source"] in self._nodes and edge["target"] in self._nodes, "dangling evidence edge")
            self._forward.setdefault(edge["source"], []).append(edge)
            self._reverse.setdefault(edge["target"], []).append(edge)

        ast = artifact["ast"]
        ast_nodes = {}
        for node in ast["nodes"]:
            check_record("ast", node)
            unique(node["id"], "AST")
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
            unique(alternative["id"], "alternative")
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
        for run in artifact["validation"]:
            check_record("validation", run)
            unique(run["id"], "validation")
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

    def why(self, identifier, max_depth=8, max_nodes=1000):
        """Return the incoming support/derivation/render chain behind a node."""
        return self.query(identifier, relation=None, direction="incoming",
                         max_depth=max_depth, max_nodes=max_nodes)

    def conflicts_with(self, identifier, max_depth=8, max_nodes=1000):
        """Return competing claims linked by contradicts or alternative-to edges."""
        result = self.query(identifier, relation="contradicts", direction="incoming",
                           max_depth=max_depth, max_nodes=max_nodes)
        alt = self.query(identifier, relation="alternative-to", direction="incoming",
                        max_depth=max_depth, max_nodes=max_nodes)
        merged_nodes = {node["id"]: node for node in result["nodes"] + alt["nodes"]}
        merged_edges = {edge["id"]: edge for edge in result["edges"] + alt["edges"]}
        return {"nodes": [merged_nodes[key] for key in sorted(merged_nodes)],
                "edges": [merged_edges[key] for key in sorted(merged_edges)],
                "truncated": result["truncated"] or alt["truncated"]}

    def invalidated_by(self, identifier, max_depth=8, max_nodes=1000):
        """Return the outgoing impact chain that an invalidation would reach."""
        return self.query(identifier, relation=None, direction="outgoing",
                         max_depth=max_depth, max_nodes=max_nodes)
