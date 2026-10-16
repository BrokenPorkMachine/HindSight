import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
CONT = (ROOT / "App/AnalysisContinuity.swift").read_text()
NAV = (ROOT / "App/AnalysisNavigation.swift").read_text()
MODEL = (ROOT / "App/WorkbenchModel.swift").read_text()
TOPOLOGY = (ROOT / "Config/project-topology.json").read_text()
BRIDGE = (ROOT / "rust/crates/bridge/src/lib.rs").read_text()


def test_project_local_continuity_is_bounded_atomic_and_symlink_aware():
    assert 'maximumBytes = 1 * 1024 * 1024' in CONT
    assert 'maximumPins = 128' in CONT
    assert 'maximumRecent = 96' in CONT
    assert 'analysis-continuity-v1.json' in CONT
    assert 'options: .atomic' in CONT
    assert '.isSymbolicLinkKey' in CONT
    assert '.isRegularFileKey' in CONT
    assert 'decoder.dateDecodingStrategy = .iso8601' in CONT


def test_triage_exposes_focus_pinned_recent_and_changes():
    for token in ['Text("Focus").tag("Focus")', 'Text("Pinned").tag("Pinned")', 'Text("Recent").tag("Recent")', 'Text("Changes").tag("Changes")']:
        assert token in NAV
    assert 'VGAnalysisDeltaView' in NAV
    assert 'toggleAnalysisPin(hit)' in NAV
    assert 'recordAnalysisVisit(hit, destination: destination)' in NAV


def test_change_tracking_is_explicitly_nonsemantic_and_engine_stable():
    assert 'arm64-analysis-v19-schema72' in CONT
    assert 'continuity signal, not semantic diffing' in NAV
    assert 'vgContinuityFingerprint' in CONT
    assert 'sourceSHA256' in CONT


def test_launch_session_schema_two_restores_context_and_accepts_schema_one():
    assert 'schema: 2' in MODEL
    assert '[1, 2].contains(session.schema)' in MODEL
    for token in ['addressText: String(addressText.prefix(128))', 'cfgFocus: String(cfgFocus.prefix(128))', 'analysisSearch: String(analysisSearch.prefix(1024))', 'selectedSymbol: String(selectedSymbol.prefix(1024))']:
        assert token in MODEL
    for token in ['addressText = session.addressText ?? ""', 'cfgFocus = session.cfgFocus ?? ""', 'analysisSearch = session.analysisSearch ?? ""', 'selectedSymbol = session.selectedSymbol ?? ""']:
        assert token in MODEL


def test_continuity_is_shipped_and_capability_advertised():
    assert 'App/AnalysisContinuity.swift' in TOPOLOGY
    for capability in ['persistent_analysis_pins', 'recent_evidence_history', 'analysis_change_ledger', 'launch_context_continuity']:
        assert f'\\"{capability}\\":true' in BRIDGE
