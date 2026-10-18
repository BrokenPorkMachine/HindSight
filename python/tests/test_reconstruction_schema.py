from pathlib import Path
import json
import re


ROOT = Path(__file__).resolve().parents[2]
SCHEMA_PATH = ROOT / "schemas" / "reconstruction.schema.json"


def schema():
    return json.loads(SCHEMA_PATH.read_text())


def test_reconstruction_schema_is_versioned_closed_and_bounded():
    value = schema()
    assert value["$schema"].endswith("2020-12/schema")
    assert value["properties"]["schema"]["const"] == 1
    assert value["properties"]["producer"]["const"] == "hindsight-reconstruction-v1"
    assert value["additionalProperties"] is False
    for collection in ("nodes", "edges", "alternatives", "validation"):
        assert 0 < value["properties"][collection]["maxItems"] <= 500_000


def test_reconstruction_schema_separates_evidence_epistemology_and_relations():
    value = schema()
    node = value["$defs"]["evidenceNode"]
    assert set(node["properties"]["epistemic"]["enum"]) == {
        "fact", "derived", "assumption", "analyst-assertion", "rejected"
    }
    relations = set(value["$defs"]["evidenceEdge"]["properties"]["relation"]["enum"])
    assert {"supports", "contradicts", "assumes", "invalidates", "alternative-to", "validates"} <= relations
    assert {"bytes", "ir-operation", "phi", "memory-effect", "ast-node", "unsupported-effect"} <= set(
        node["properties"]["kind"]["enum"]
    )


def test_reconstruction_schema_requires_content_ids_traceability_and_honest_eligibility():
    value = schema()
    assert value["$defs"]["contentId"]["pattern"] == "^sha256:[0-9a-f]{64}$"
    ast_node = value["$defs"]["ast"]["properties"]["nodes"]["items"]
    assert {"id", "children", "evidence", "ranges", "confidence"} <= set(ast_node["required"])
    assert set(value["properties"]["eligibility"]["properties"]["state"]["enum"]) == {
        "equivalent", "recompilable-with-intrinsics", "review-only", "blocked"
    }
    required = set(value["required"])
    assert {"source_sha256", "analysis_engine", "budgets", "validation", "truncated"} <= required


def test_reconstruction_first_roadmap_orders_validation_after_semantic_dependencies():
    roadmap = (ROOT / "docs" / "RECONSTRUCTION_FIRST_ROADMAP.md").read_text()
    headings = re.findall(r"^## (R\d+) —", roadmap, re.MULTILINE)
    assert headings == [f"R{index}" for index in range(11)]
    assert roadmap.index("## R1 — Complete the semantic substrate") < roadmap.index("## R8 — Prove equivalence")
    assert "literal 1:1" not in roadmap.lower()
    assert "information erased by compilation" in roadmap
