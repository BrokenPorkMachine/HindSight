import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
CONT = (ROOT / "App/AnalysisContinuity.swift").read_text()
NAV = (ROOT / "App/AnalysisNavigation.swift").read_text()
MODEL = (ROOT / "App/WorkbenchModel.swift").read_text()
BRIDGE = (ROOT / "rust/crates/bridge/src/lib.rs").read_text()


def test_continuity_schema4_migrates_prior_state_and_bounds_review_journal():
    assert "var schema: Int = 7" in CONT
    assert "[1, 2, 3, 4, 5, 6, 7].contains(document.schema)" in CONT
    assert "decodeIfPresent([VGInvestigationReviewEntry].self" in CONT
    assert "maximumReviewEntries = 256" in CONT
    assert "maximumReviewEntriesPerCollection = 64" in CONT
    assert "maximumReviewNoteBytes = 2_048" in CONT
    assert "validReviewEntry" in CONT


def test_review_journal_is_project_local_bounded_and_exact_membership_aware():
    assert "struct VGInvestigationReviewEntry" in CONT
    assert "enum VGInvestigationReviewState" in CONT
    assert "memberPinIDs: collection.pinIDs" in CONT
    assert "vgBoundedUTF8" in CONT
    assert "appendAnalysisReviewEntry" in CONT
    assert "analysisReviewEntries" in MODEL
    assert "reviewEntries.removeAll { $0.collectionID == collectionID }" in CONT


def test_review_workflow_states_are_explicit_analyst_context():
    for state in ['case open = "Open"', 'case inReview = "In Review"', 'case followUp = "Follow Up"', 'case reviewed = "Reviewed"']:
        assert state in CONT
    assert "Review notes and workflow states are analyst-authored project context" in NAV
    assert "do not imply vulnerability, severity, correctness, runtime reachability, or semantic importance" in NAV
    assert "They also do not imply exploitability." in NAV


def test_handoff_is_bounded_and_includes_provenance_review_and_evidence():
    assert "func analysisCollectionHandoff(" in CONT
    assert '"# HindSight Investigation Handoff"' in CONT
    assert '"Source SHA-256:' in CONT
    assert '"Workflow state:' in CONT
    assert '"Collection health:' in CONT
    assert '"Review drift:' in CONT
    assert "hits.prefix(VGAnalysisContinuityStore.maximumPinsPerCollection)" in CONT
    assert "entries.prefix(16)" in CONT
    assert "copyAnalysisCollectionHandoff" in CONT
    assert 'Button("Copy Handoff")' in NAV


def test_review_ui_works_with_one_collection_and_stays_bounded():
    assert '"No collections to review"' in NAV
    assert '"Create a second collection to compare"' in NAV
    assert 'Label("Review Journal & Handoff"' in NAV
    assert 'Button("Record Review")' in NAV
    assert "entries.prefix(12)" in NAV
    assert "VGFlowLayout(horizontalSpacing: 8, verticalSpacing: 8)" in NAV


def test_capabilities_advertise_review_journal_and_handoff():
    for capability in [
        "collection_review_journal",
        "collection_workflow_state",
        "investigation_handoff",
        "review_timeline",
    ]:
        assert f'\\"{capability}\\":true' in BRIDGE
