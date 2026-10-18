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
AnalysisScope = MODULE.AnalysisScope
ReconstructionArtifact = MODULE.ReconstructionArtifact
content_id = MODULE.content_id
settings_digest = MODULE.settings_digest
compute_eligibility = MODULE.compute_eligibility

PASS_VERSION = "reconstruct-1"
SETTINGS = {"memory_budget": 4096, "variable_recovery": True}


def fixture(pass_version=PASS_VERSION, settings=None, source="1" * 64):
    """A review-only return example; no invented compilation result."""
    settings = SETTINGS if settings is None else settings
    scope = AnalysisScope.from_settings(pass_version, settings)
    function = {"start": "0x1000", "end": "0x1008", "architecture": "arm64", "abi": "apple-aapcs64", "name": "identity"}
    function["id"] = content_id("function", source, None, function, scope)

    def identified(domain, record):
        record["id"] = content_id(domain, source, function["id"], record, scope)
        return record

    span = [{"start": "0x1000", "end": "0x1008"}]
    raw = identified("evidence", {"kind": "bytes", "epistemic": "fact", "producer": "fixture-v1",
                                 "confidence": 1, "payload": {"hex": "e00301aac0035fd6"}, "ranges": span})
    ir = identified("evidence", {"kind": "ir-operation", "epistemic": "derived", "producer": "fixture-v1",
                                "confidence": 1, "payload": {"operation": "return", "register": "x0"}, "ranges": span})
    edge = identified("edge", {"source": raw["id"], "target": ir["id"], "relation": "lifts-to", "producer": "fixture-v1"})
    ast = identified("ast", {"kind": "return", "type": "uint64_t", "children": [], "evidence": [ir["id"]], "ranges": span,
                             "confidence": 1, "attributes": {"value": "x0"}})
    return {"schema": 1, "producer": "hindsight-reconstruction-v1", "source_sha256": source,
            "analysis_engine": "fixture-v1",
            "scope": {"pass_version": scope.pass_version, "settings_digest": scope.settings_digest},
            "function": function, "eligibility": {"state": "review-only", "reasons": []},
            "nodes": [raw, ir], "edges": [edge],
            "ast": {"language": "c17", "root": ast["id"], "nodes": [ast], "rendering": "return x0;", "compilable": False},
            "alternatives": [], "budgets": {"limits": {"instructions": 128}, "consumed": {"instructions": 2}, "exhausted": []},
            "validation": [], "truncated": False}


def reidentify(artifact, domain, record):
    scope = AnalysisScope(artifact["scope"]["pass_version"], artifact["scope"]["settings_digest"])
    record["id"] = content_id(domain, artifact["source_sha256"], artifact["function"]["id"], record, scope)


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

    def test_compute_eligibility_blocks_incomplete_and_unsupported(self):
        document = fixture()
        eligibility = compute_eligibility(document)
        self.assertEqual(eligibility["state"], "review-only")
        self.assertEqual(eligibility["reasons"], [])
        # truncated analysis cannot claim export
        truncated = copy.deepcopy(document)
        truncated["truncated"] = True
        self.assertEqual(compute_eligibility(truncated)["state"], "blocked")
        self.assertTrue(any(reason["blocking"] for reason in compute_eligibility(truncated)["reasons"]))
        # unsupported effect blocks export
        unsupported_doc = copy.deepcopy(document)
        unsupported_doc["nodes"].append({"id": "sha256:" + "1" * 64, "kind": "unsupported-effect",
                                       "epistemic": "derived", "producer": "fixture-v1",
                                       "confidence": 1, "payload": {}, "ranges": []})
        self.assertEqual(compute_eligibility(unsupported_doc)["state"], "blocked")

    def test_why_conflicts_and_invalidation_queries(self):
        document = fixture()
        artifact = ReconstructionArtifact(document)
        node_id = document["nodes"][1]["id"]
        why = artifact.why(node_id)
        self.assertIn(node_id, {node["id"] for node in why["nodes"]})
        self.assertFalse(why["truncated"])
        invalidated = artifact.invalidated_by(document["nodes"][0]["id"])
        self.assertIn(document["nodes"][0]["id"], {node["id"] for node in invalidated["nodes"]})
        conflicts = artifact.conflicts_with(node_id)
        self.assertIsInstance(conflicts["nodes"], list)
        self.assertIsInstance(conflicts["edges"], list)

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

    def test_identity_is_scoped_to_pass_version_and_analysis_settings(self):
        # Identical record content under a different pass or different settings
        # must not reuse an identity, or a replayed artifact could answer for
        # an analysis that never ran.
        record = {"kind": "variable", "epistemic": "derived", "producer": "p", "confidence": 1,
                  "payload": {"name": "x"}, "ranges": [{"start": "0x1000", "end": "0x1004"}]}
        base = AnalysisScope.from_settings(PASS_VERSION, SETTINGS)
        other_pass = AnalysisScope.from_settings("reconstruct-2", SETTINGS)
        other_settings = AnalysisScope.from_settings(PASS_VERSION, {"memory_budget": 8192})
        ids = {content_id("evidence", "1" * 64, "f" * 64, dict(record), scope)
               for scope in (base, other_pass, other_settings)}
        self.assertEqual(len(ids), 3)
        # Scope is deterministic and derived, never supplied by the producer.
        self.assertEqual(content_id("evidence", "1" * 64, "f" * 64, dict(record), base),
                         content_id("evidence", "1" * 64, "f" * 64, dict(record), base))
        self.assertEqual(AnalysisScope.from_settings(PASS_VERSION, SETTINGS), base)
        self.assertNotEqual(AnalysisScope.from_settings(PASS_VERSION, SETTINGS), other_settings)

    def test_artifact_scope_must_match_every_record(self):
        # Re-identifying an artifact under a foreign scope is rejected: the
        # function identity itself is scope-bound.
        document = fixture()
        document["scope"]["pass_version"] = "reconstruct-9"
        self.assertInvalid(document, "function content identity mismatch")
        document = fixture()
        document["scope"]["settings_digest"] = settings_digest({"memory_budget": 8192})
        self.assertInvalid(document, "function content identity mismatch")
        document = fixture()
        del document["scope"]
        self.assertInvalid(document, "missing required fields")

    def test_settings_digest_is_canonical_and_bounded(self):
        # Key order must not change the digest, but values must.
        self.assertEqual(settings_digest({"a": 1, "b": [2, 3]}), settings_digest({"b": [2, 3], "a": 1}))
        self.assertNotEqual(settings_digest({"a": 1}), settings_digest({"a": 2}))
        for bad, message in [
            ({}, "settings key limit"),
            ({"Bad-Key": 1}, "malformed analysis settings key"),
            ({"a" * 65: 1}, "malformed analysis settings key"),
            ({f"k{index}": 0 for index in range(129)}, "settings key limit"),
            ([], "must be an object"),
        ]:
            with self.subTest(settings=bad):
                with self.assertRaisesRegex(ArtifactError, message):
                    settings_digest(bad)
        for bad in ["", "RECONSTRUCT", "../etc", "a" * 65]:
            with self.subTest(pass_version=bad):
                with self.assertRaises(ArtifactError):
                    AnalysisScope(bad, settings_digest(SETTINGS))
        with self.assertRaises(ArtifactError):
            AnalysisScope(PASS_VERSION, "not-a-digest")
        with self.assertRaises(ArtifactError):
            content_id("evidence", "1" * 64, "f" * 64, {"kind": "bytes"}, None)

    def test_validation_results_cannot_cross_scopes(self):
        # A validation run is bound to the candidate digest, which now covers
        # pass version and settings. Re-scoping a run's own records must still
        # invalidate results that were produced by the earlier analysis.
        document = fixture()
        run = validation_run(document, "differential")
        document["validation"] = [run]
        ReconstructionArtifact(document)
        rescoped = fixture(pass_version="reconstruct-9")
        rescoped["validation"] = [dict(run)]
        self.assertNotEqual(MODULE.reconstruction_digest(rescoped), run["artifact_sha256"])
        self.assertInvalid(rescoped, "identity mismatch")

    def test_identities_are_unique_across_record_kinds(self):
        # Identity uniqueness is checked in one namespace, and the domain is
        # itself hashed in, so no record kind can shadow another kind's id.
        record = {"kind": "variable", "epistemic": "derived", "producer": "p", "confidence": 1,
                  "payload": {}, "ranges": [{"start": "0x1000", "end": "0x1004"}]}
        scope = AnalysisScope.from_settings(PASS_VERSION, SETTINGS)
        domains = ["evidence", "edge", "ast", "alternative", "validation"]
        self.assertEqual(len({content_id(domain, "1" * 64, "f" * 64, dict(record), scope) for domain in domains}),
                         len(domains))
        # A repeated record is still rejected inside every kind that can repeat.
        document = fixture()
        document["validation"] = [validation_run(document, "schema")]
        document["validation"].append(copy.deepcopy(document["validation"][0]))
        self.assertInvalid(document, "duplicate validation identity")
        document = fixture()
        alternative = {"subject": document["nodes"][0]["id"],
                      "candidates": [document["nodes"][0]["id"], document["nodes"][1]["id"]],
                      "selected": document["nodes"][1]["id"], "reason": "prototype conflict"}
        alternative["id"] = content_id("alternative", document["source_sha256"], document["function"]["id"],
                                       alternative, AnalysisScope(PASS_VERSION, document["scope"]["settings_digest"]))
        document["alternatives"] = [alternative, copy.deepcopy(alternative)]
        self.assertInvalid(document, "duplicate alternative identity")

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
            summary = json.loads(result.stdout)
            self.assertTrue(summary["valid"])
            # A valid artifact reports which analysis produced it.
            self.assertEqual(summary["scope"], document["scope"])
            result = subprocess.run(command + ["--node", document["nodes"][1]["id"]], capture_output=True, text=True, check=True)
            self.assertEqual(len(json.loads(result.stdout)["nodes"]), 2)
            path.write_text('{"schema":false}')
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(result.returncode, 1)
            self.assertIn("invalid reconstruction", result.stderr)


if __name__ == "__main__":
    unittest.main()
