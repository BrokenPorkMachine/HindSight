import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
CONT = (ROOT / "App/AnalysisContinuity.swift").read_text()
NAV = (ROOT / "App/AnalysisNavigation.swift").read_text()
MODEL = (ROOT / "App/WorkbenchModel.swift").read_text()
APP = (ROOT / "App/HindSightApp.swift").read_text()
BRIDGE = (ROOT / "rust/crates/bridge/src/lib.rs").read_text()


def test_collections_are_bounded_and_migrate_from_continuity_v1():
    assert 'struct VGInvestigationCollection' in CONT
    assert 'var schema: Int = 7' in CONT
    assert '[1, 2, 3, 4, 5, 6, 7].contains(document.schema)' in CONT
    assert 'decodeIfPresent([VGInvestigationCollection].self' in CONT
    assert 'maximumCollections = 32' in CONT
    assert 'maximumPinsPerCollection = 64' in CONT


def test_collection_membership_is_pin_backed_and_cleanup_safe():
    assert 'func toggleAnalysisCollectionMembership(' in CONT
    assert 'analysisContinuityDocument.pins.insert(pin, at: 0)' in CONT
    assert 'pinIDs.removeAll { $0 == removedID }' in CONT
    assert 'func analysisCollectionHits(' in CONT


def test_triage_exposes_named_collections_without_new_analysis_semantics():
    assert 'Text("Collections").tag("Collections")' in NAV
    assert 'New investigation collection…' in NAV
    assert 'Menu("Collect")' in NAV
    assert 'Collection membership automatically preserves the underlying evidence' in NAV


def test_resume_replays_existing_history_without_recording_a_duplicate_visit():
    assert 'func resumeAnalysisVisit(' in NAV
    assert 'func resumeMostRecentAnalysisVisit()' in NAV
    assert 'applyAnalysisHit(visit.hit, destination: destination, recordVisit: false)' in NAV
    assert 'Button("Resume Latest"' in NAV
    assert 'Button("Resume Latest Evidence"' in APP
    assert 'analysis_collections=' in MODEL


def test_capabilities_advertise_collection_continuity():
    for capability in ['investigation_collections', 'collection_aware_pins', 'evidence_resume']:
        assert f'\\"{capability}\\":true' in BRIDGE
