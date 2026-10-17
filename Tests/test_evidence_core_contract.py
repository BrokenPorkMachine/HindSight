from pathlib import Path
import codecs
import json
import re

ROOT = Path(__file__).resolve().parents[1]

def read(path: str) -> str:
    return (ROOT / path).read_text()

def test_evidence_core_is_in_generated_app_target():
    topology = json.loads(read('Config/project-topology.json'))
    assert 'App/AnalysisEvidenceCore.swift' in topology['targets']['HindSight']

def test_evidence_core_is_bounded_and_indexed():
    core = read('App/AnalysisEvidenceCore.swift')
    for token in ['maxRecords = 50_000', 'addressIndex', 'symbolIndex', 'focusOrder', 'sourceRanges', 'normalizedText', 'semanticIdentity']:
        assert token in core
    assert 'if records.count >= maxRecords' in core

def test_render_path_uses_index_not_report_reparse():
    nav = read('App/AnalysisNavigation.swift')
    model = read('App/WorkbenchModel.swift')
    assert 'currentEvidenceIndex?.search' in nav
    assert 'currentEvidenceIndex?.focus' in nav
    assert 'currentEvidenceIndex?.related' in nav
    assert 'index.search(query, limit: 500)' in model
    assert 'Task.detached(priority: .utility)' in model

def test_compatibility_transport_remains_schema66():
    messages = read('Shared/WorkbenchMessages.swift')
    assert 'static let jsonFields: Set<String>' in messages
    assert 'function_neighborhoods' in messages and 'analysis_insights' in messages
    assert 'arm64-analysis-v71-schema117' in read('PlatformServices/CoordinatorService/main.swift')

def test_capabilities_publish_evidence_core():
    bridge = read('rust/crates/bridge/src/lib.rs')
    for capability in ['typed_evidence_core','off_main_evidence_index','semantic_evidence_identity','indexed_evidence_navigation','evidence_core_compat_adapter']:
        assert f'\\"{capability}\\":true' in bridge

def test_analysis_report_memory_is_bounded():
    model = read('App/WorkbenchModel.swift')
    assert 'maxResidentAnalysisReports = 8' in model
    assert 'analysisReportLRU' in model
    assert 'cacheAnalysisReport(report, recordID:' in model
    assert 'analysisReportsByRecordID.removeValue(forKey: evicted)' in model

def test_launch_session_writes_are_debounced():
    model = read('App/WorkbenchModel.swift')
    assert 'launchSessionSaveTask?.cancel()' in model
    assert 'Task.sleep(nanoseconds: 350_000_000)' in model
    assert 'writeLaunchSessionNow()' in model


def test_unified_symbol_resolver_capability():
    caps = read('rust/crates/bridge/src/lib.rs')
    match = re.search(r'static CAPABILITIES:.*?= b"(.*?)\\0";', caps, re.S)
    assert match, 'bridge capability payload is missing'
    payload = json.loads(codecs.decode(match.group(1), 'unicode_escape'))
    assert payload.get('unified_symbol_resolver') is True
