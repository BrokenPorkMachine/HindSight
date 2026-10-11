from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def test_runtime_pivots_keep_stop_context_visible():
    workspace = (ROOT / "App/RuntimeCorrelationWorkspace.swift").read_text()
    assert 'runtimePill("STOP", value: sample.event' in workspace
    assert 'Text(sample.reason)' in workspace
    assert 'richer breakpoint-condition context remains open' in (ROOT / "docs/DEVELOPMENT_PLAN.md").read_text()
