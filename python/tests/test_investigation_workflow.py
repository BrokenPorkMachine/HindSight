import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
CONT = (ROOT / "App/AnalysisContinuity.swift").read_text()
NAV = (ROOT / "App/AnalysisNavigation.swift").read_text()
APP = (ROOT / "App/HindSightApp.swift").read_text()
BRIDGE = (ROOT / "rust/crates/bridge/src/lib.rs").read_text()


def test_collections_are_searchable_and_renameable_without_rekeying_pins():
    assert "func filteredAnalysisCollections(query:" in CONT
    assert "func renameAnalysisCollection(" in CONT
    assert "collection.name.localizedCaseInsensitiveContains" in CONT
    assert "pin.text.localizedCaseInsensitiveContains" in CONT
    assert "Renamed investigation collection" in CONT


def test_collection_health_is_bounded_and_nonsemantic():
    assert "func currentAnalysisPinIDs(limit: Int = 25_000)" in CONT
    assert "index.corpus(limit: limit)" in CONT
    assert "guard let recordID = selectedID, let index = currentEvidenceIndex" in CONT
    assert "func analysisCollectionHealth(" in CONT
    assert "Collection health compares preserved pin identities with a bounded corpus" in NAV
    assert "it does not mean the finding is invalid or semantically obsolete" in NAV


def test_recent_evidence_has_back_forward_traversal_without_new_visits():
    assert "func navigateAnalysisHistoryBackward()" in NAV
    assert "func navigateAnalysisHistoryForward()" in NAV
    assert "resumeAnalysisVisit(analysisRecentVisits[analysisHistoryCursor])" in NAV
    assert 'Button("Previous Evidence"' in APP
    assert 'Button("Next Evidence"' in APP


def test_capabilities_advertise_investigation_workflow():
    for capability in ["collection_search", "collection_health", "collection_rename", "evidence_history_navigation"]:
        assert f'\\"{capability}\\":true' in BRIDGE
