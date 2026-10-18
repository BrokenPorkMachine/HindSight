"""Exercise real artifact integrity and evidence-query behavior."""

import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("reconstruction_artifact", ROOT / "scripts/reconstruction_artifact.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
ArtifactError = MODULE.ArtifactError
ReconstructionArtifact = MODULE.ReconstructionArtifact
content_id = MODULE.content_id


def fixture():
    """A review-only return example; no invented compilation result."""
    source = "1" * 64
    function = {"start": "0x1000", "end": "0x1008", "architecture": "arm64", "abi": "apple-aapcs64", "name": "identity"}
    function["id"] = content_id("function", source, None, function)

    def identified(domain, record):
        record["id"] = content_id(domain, source, function["id"], record)
        return record

    span = [{"start": "0x1000", "end": "0x1008"}]
    raw = identified("evidence", {"kind": "bytes", "epistemic": "fact", "producer": "fixture-v1",
                                 "confidence": 1, "payload": {"hex": "e00301aac0035fd6"}, "ranges": span})
    ir = identified("evidence", {"kind": "ir-operation", "epistemic": "derived", "producer": "fixture-v1",
                                "confidence": 1, "payload": {"operation": "return", "register": "x0"}, "ranges": span})
    edge = identified("edge", {"source": raw["id"], "target": ir["id"], "relation": "lifts-to", "producer": "fixture-v1"})
    ast = identified("ast", {"kind": "return", "type": "uint64_t", "children": [], "evidence": [ir["id"]], "ranges": span,
                             "confidence": 1, "attributes": {"value": "x0"}})
    return {"schema": 1, "producer": "hindsight-reconstruction-v1", "source_sha256": source, "analysis_engine": "fixture-v1",
            "function": function, "eligibility": {"state": "review-only", "reasons": []},
            "nodes": [raw, ir], "edges": [edge],
            "ast": {"language": "c17", "root": ast["id"], "nodes": [ast], "rendering": "return x0;", "compilable": False},
            "alternatives": [], "budgets": {"limits": {"instructions": 128}, "consumed": {"instructions": 2}, "exhausted": []},
            "validation": [], "truncated": False}


def reidentify(artifact, domain, record):
    record["id"] = content_id(domain, artifact["source_sha256"], artifact["function"]["id"], record)


def validation_run(artifact, kind, status="passed", mismatches=None):
    run = {"kind": kind, "status": status, "artifact_sha256": MODULE.reconstruction_digest(artifact),
           "assumptions": [], "mismatches": mismatches or []}
    reidentify(artifact, "validation", run)
    return run


class ReconstructionArtifactTests(unittest.TestCase):
    def assertInvalid(self, artifact, message):
        with self.assertRaisesRegex(ArtifactError, message):
            ReconstructionArtifact(artifact)

    def test_valid_review_artifact_and_graph_queries(self):
        document = fixture()
        artifact = ReconstructionArtifact(document)
        result = artifact.query(document["nodes"][1]["id"])
        self.assertEqual(len(result["nodes"]), 2)
        self.assertEqual(len(result["edges"]), 1)
        self.assertFalse(result["truncated"])
        self.assertEqual(len(artifact.query(document["nodes"][0]["id"], "lifts-to", "outgoing")["nodes"]), 2)
        self.assertEqual(len(artifact.query(document["nodes"][1]["id"], "contradicts")["nodes"]), 1)

    def test_query_bounds_are_explicit(self):
        document = fixture()
        artifact = ReconstructionArtifact(document)
        for limit, depth in [(1, 8), (1000, 0)]:
            result = artifact.query(document["nodes"][1]["id"], max_nodes=limit, max_depth=depth)
            self.assertTrue(result["truncated"])
            self.assertEqual(len(result["nodes"]), 1)
        with self.assertRaises(ArtifactError):
            artifact.query("sha256:" + "0" * 64)
        with self.assertRaises(ArtifactError):
            artifact.query(document["nodes"][0]["id"], max_nodes=True)

    def test_canonical_identity_is_order_independent_and_type_sensitive(self):
        self.assertEqual(MODULE.canonical_bytes({"a": [1, "é"], "b": False}), MODULE.canonical_bytes({"b": False, "a": [1.0, "é"]}))
        self.assertNotEqual(MODULE.canonical_bytes(True), MODULE.canonical_bytes(1))
        self.assertNotEqual(MODULE.canonical_bytes(["ab", "c"]), MODULE.canonical_bytes(["a", "bc"]))
        self.assertEqual(MODULE.canonical_bytes(-0.0), MODULE.canonical_bytes(0))
        for value in [float("nan"), float("inf"), {"bad": object()}, {1: "bad"}, chr(0xd800)]:
            with self.assertRaises(ArtifactError):
                MODULE.canonical_bytes(value)

    def test_content_tampering_and_binary_rebinding_are_rejected(self):
        document = fixture()
        document["nodes"][0]["payload"]["hex"] = "00000000"
        self.assertInvalid(document, "evidence content identity mismatch")
        document = fixture()
        document["source_sha256"] = "2" * 64
        self.assertInvalid(document, "function content identity mismatch")

    def test_duplicate_ids_and_dangling_graph_edges_are_rejected(self):
        document = fixture()
        document["nodes"].append(copy.deepcopy(document["nodes"][0]))
        self.assertInvalid(document, "duplicate evidence identity")
        document = fixture()
        document["edges"][0]["target"] = "sha256:" + "a" * 64
        reidentify(document, "edge", document["edges"][0])
        self.assertInvalid(document, "dangling evidence edge")

    def test_ast_requires_evidence_valid_children_and_local_spans(self):
        for field, value, message in [
            ("evidence", [], "missing AST evidence"),
            ("children", ["sha256:" + "0" * 64], "dangling AST child"),
            ("ranges", [{"start": "0x1004", "end": "0x1004"}], "empty or reversed"),
            ("ranges", [{"start": "0x1000", "end": "0x2000"}], "outside function"),
        ]:
            with self.subTest(field=field, value=value):
                document = fixture()
                node = document["ast"]["nodes"][0]
                node[field] = value
                reidentify(document, "ast", node)
                document["ast"]["root"] = node["id"]
                self.assertInvalid(document, message)

    def test_orphaned_ast_cannot_be_hidden_from_rendering(self):
        document = fixture()
        node = copy.deepcopy(document["ast"]["nodes"][0])
        node["attributes"]["value"] = "unreachable"
        reidentify(document, "ast", node)
        document["ast"]["nodes"].append(node)
        self.assertInvalid(document, "orphaned AST node")

    def test_schema_rejects_unknown_fields_booleans_as_integers_and_bad_addresses(self):
        document = fixture()
        document["invented"] = "field"
        self.assertInvalid(document, "unknown field")
        document = fixture()
        document["schema"] = True
        self.assertInvalid(document, "incorrect constant")
        document = fixture()
        document["budgets"]["consumed"]["instructions"] = True
        self.assertInvalid(document, "invalid value type")
        document = fixture()
        document["function"]["start"] = "0x1000\n"
        self.assertInvalid(document, "malformed string")

    def test_budget_exhaustion_and_success_claims_cannot_disagree(self):
        document = fixture()
        document["budgets"]["consumed"]["instructions"] = 129
        self.assertInvalid(document, "budget exhaustion")
        document["budgets"]["exhausted"] = ["instructions"]
        ReconstructionArtifact(document)
        document["ast"]["compilable"] = True
        document["eligibility"]["state"] = "recompilable-with-intrinsics"
        document["validation"] = [validation_run(document, "recompile")]
        self.assertInvalid(document, "incomplete reconstruction")

    def test_compilation_is_required_but_does_not_prove_equivalence(self):
        document = fixture()
        document["ast"]["compilable"] = True
        document["eligibility"]["state"] = "recompilable-with-intrinsics"
        self.assertInvalid(document, "lacks successful compilation")
        document["validation"] = [validation_run(document, "recompile")]
        ReconstructionArtifact(document)
        document["eligibility"]["state"] = "equivalent"
        document["validation"].append(validation_run(document, "differential"))
        self.assertInvalid(document, "lacks structural and formal evidence")

    def test_validation_is_bound_to_candidate_and_cannot_pass_with_mismatches(self):
        document = fixture()
        document["validation"] = [validation_run(document, "differential")]
        document["ast"]["rendering"] = "return 0;"
        self.assertInvalid(document, "different reconstruction")
        document = fixture()
        document["validation"] = [validation_run(document, "differential", mismatches=["return value differs"])]
        self.assertInvalid(document, "passing validation contains mismatches")

    def test_load_rejects_duplicate_json_keys_and_excessive_nesting(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "artifact.json"
            path.write_text('{"schema":1,"schema":1}')
            with self.assertRaisesRegex(ArtifactError, "duplicate JSON key"):
                ReconstructionArtifact.load(path)
        value = 0
        for _ in range(66):
            value = [value]
        with self.assertRaisesRegex(ArtifactError, "nesting/value budget"):
            MODULE.canonical_bytes(value)

    def test_validated_graph_is_an_immutable_snapshot(self):
        document = fixture()
        artifact = ReconstructionArtifact(document)
        identifier = document["nodes"][0]["id"]
        document["nodes"][0]["payload"]["hex"] = "tampered"
        artifact.data["nodes"][0]["payload"]["hex"] = "tampered again"
        artifact.query(identifier)["nodes"][0]["payload"]["hex"] = "tampered query"
        self.assertNotEqual(artifact.data["nodes"][0]["payload"]["hex"], "tampered query")

    def test_cli_validates_and_queries_artifacts(self):
        document = fixture()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "artifact.json"
            path.write_text(json.dumps(document))
            command = [sys.executable, str(ROOT / "scripts/validate-reconstruction.py"), str(path)]
            result = subprocess.run(command, capture_output=True, text=True, check=True)
            self.assertTrue(json.loads(result.stdout)["valid"])
            result = subprocess.run(command + ["--node", document["nodes"][1]["id"]], capture_output=True, text=True, check=True)
            self.assertEqual(len(json.loads(result.stdout)["nodes"]), 2)
            path.write_text('{"schema":false}')
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(result.returncode, 1)
            self.assertIn("invalid reconstruction", result.stderr)


if __name__ == "__main__":
    unittest.main()
