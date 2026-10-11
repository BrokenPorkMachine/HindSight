import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
CONT = (ROOT / "App/AnalysisContinuity.swift").read_text()
NAV = (ROOT / "App/AnalysisNavigation.swift").read_text()
MODEL = (ROOT / "App/WorkbenchModel.swift").read_text()
BRIDGE = (ROOT / "rust/crates/bridge/src/lib.rs").read_text()


def test_continuity_schema4_keeps_schema3_compatibility_and_bounds_review_baselines():
    assert "var schema: Int = 5" in CONT
    assert "[1, 2, 3, 4, 5].contains(document.schema)" in CONT
    assert "decodeIfPresent([String: VGInvestigationReviewBaseline].self" in CONT
    assert "maximumReviewBaselines = 256" in CONT
    assert "validReviewBaseline" in CONT
    assert "Set(currentPinIDs).isSubset(of: Set(value.memberPinIDs))" in CONT


def test_collection_review_baseline_is_exact_identity_and_project_local():
    assert "struct VGInvestigationReviewBaseline" in CONT
    assert "func markAnalysisCollectionReviewed(" in CONT
    assert "memberPinIDs: Array(collection.pinIDs.prefix" in CONT
    assert "sourceSHA256: selected?.result.source_sha256" in CONT
    assert "analysisReviewBaselines" in MODEL


def test_collection_review_drift_is_identity_based_not_semantic_scoring():
    assert "func analysisCollectionReviewDrift(" in CONT
    assert "newMembers.subtracting(oldMembers).count" in CONT
    assert "nowCurrent.subtracting(oldCurrent).count" in CONT
    assert "oldCurrent.subtracting(nowCurrent).count" in CONT
    assert "exact-identity/provenance tools" in NAV
    assert "do not imply vulnerability, severity, correctness, runtime reachability, or semantic importance" in NAV


def test_pairwise_collection_comparison_exposes_shared_and_unique_evidence():
    assert "func compareAnalysisCollections(" in CONT
    assert "let sharedIDs = left.pinIDs.filter" in CONT
    assert "let leftOnlyIDs = left.pinIDs.filter" in CONT
    assert "let rightOnlyIDs = right.pinIDs.filter" in CONT
    assert 'Text("Review").tag("Review")' in NAV
    assert "struct VGCollectionReviewView" in NAV
    assert "Shared evidence" in NAV
    assert 'Only in \\(comparison.leftName)' in NAV
    assert 'Only in \\(comparison.rightName)' in NAV


def test_review_ui_remains_responsive_and_bounded():
    assert "ViewThatFits(in: .horizontal)" in NAV
    assert "VGFlowLayout(horizontalSpacing: 8, verticalSpacing: 8)" in NAV
    assert "hits.prefix(24)" in NAV
    assert "additional findings omitted from this bounded review surface" in NAV


def test_capabilities_advertise_collection_review():
    for capability in [
        "collection_comparison",
        "collection_review_baselines",
        "collection_evidence_drift",
        "investigation_review_mode",
    ]:
        assert f'\\\"{capability}\\\":true' in BRIDGE
