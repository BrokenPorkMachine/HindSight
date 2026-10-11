import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
CONT = (ROOT / "App/AnalysisContinuity.swift").read_text()
NAV = (ROOT / "App/AnalysisNavigation.swift").read_text()
MODEL = (ROOT / "App/WorkbenchModel.swift").read_text()
BRIDGE = (ROOT / "rust/crates/bridge/src/lib.rs").read_text()


def test_continuity_schema5_migrates_prior_state_and_bounds_followups():
    assert "var schema: Int = 5" in CONT
    assert "[1, 2, 3, 4, 5].contains(document.schema)" in CONT
    assert "decodeIfPresent([VGInvestigationFollowUp].self" in CONT
    assert "maximumFollowUps = 128" in CONT
    assert "maximumFollowUpsPerCollection = 32" in CONT
    assert "maximumFollowUpTitleBytes = 256" in CONT
    assert "maximumFollowUpNoteBytes = 1_024" in CONT
    assert "maximumFollowUpEvidenceIDs = 16" in CONT
    assert "validFollowUp" in CONT


def test_followups_are_explicit_workflow_tasks_with_bounded_evidence_identity_context():
    assert "struct VGInvestigationFollowUp" in CONT
    assert "enum VGInvestigationFollowUpState" in CONT
    assert "relatedPinIDs" in CONT
    assert "Array(collection.pinIDs.prefix(VGAnalysisContinuityStore.maximumFollowUpEvidenceIDs))" in CONT
    assert "createAnalysisFollowUp" in CONT
    assert "toggleAnalysisFollowUpCompleted" in CONT
    assert "deleteAnalysisFollowUp" in CONT
    assert "analysisFollowUps" in MODEL


def test_followup_ui_is_responsive_and_separate_from_evidence_semantics():
    assert 'Label("Follow-Up Queue", systemImage: "checklist")' in NAV
    assert 'Button("Add Follow-Up")' in NAV
    assert "VGFlowLayout(horizontalSpacing: 8, verticalSpacing: 8)" in NAV
    assert "Follow-ups are analyst-authored workflow tasks" in NAV
    assert "They also do not imply exploitability." in NAV
    assert "items.prefix(32)" in NAV


def test_handoff_carries_open_actions_without_reclassifying_them_as_findings():
    assert '"## Follow-Up Queue"' in CONT
    assert '"- [ ] \\(item.title)' in CONT
    assert "followUps.filter { $0.state == .open }" in CONT
    assert "Follow-ups are analyst-authored workflow tasks, not findings." in CONT
    assert '"Review summary:' in CONT


def test_collection_delete_removes_collection_scoped_followups_but_unpin_does_not_silently_destroy_tasks():
    assert "followUps.removeAll { $0.collectionID == collectionID }" in CONT
    toggle_pin = CONT[CONT.index("func toggleAnalysisPin"):CONT.index("func createAnalysisCollection")]
    assert "followUps.removeAll" not in toggle_pin


def test_capabilities_advertise_review_actions():
    for capability in [
        "review_follow_up_queue",
        "follow_up_evidence_binding",
        "collection_review_summary",
        "handoff_follow_up_context",
    ]:
        assert f'\\"{capability}\\":true' in BRIDGE
