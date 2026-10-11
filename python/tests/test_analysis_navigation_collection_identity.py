from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
NAV = (ROOT / 'App' / 'AnalysisNavigation.swift').read_text()


def test_collection_foreach_uses_explicit_identity():
    assert 'ForEach(filteredCollections, id: \\.id) { collection in' in NAV
    assert 'ForEach(filteredCollections) { collection in' not in NAV
    assert 'ForEach(model.analysisCollections) { collection in' not in NAV
    assert NAV.count('ForEach(model.analysisCollections, id: \\.id) { collection in') >= 4
