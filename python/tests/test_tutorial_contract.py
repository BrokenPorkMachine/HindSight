from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def test_bundled_tutorial_fixture_is_exposed_end_to_end():
    fixture = ROOT / "Tests/Fixtures/arm64-integer-sample.macho"
    guide = (ROOT / "docs/HINDSIGHT_TUTORIAL.md").read_text()
    quick_start = (ROOT / "App/ContentView.swift").read_text()
    assert fixture.is_file() and fixture.stat().st_size > 0
    assert "arm64-integer-sample.macho" in guide
    assert "Analysis Quality" in guide
    assert "HindSight Case" in guide
    assert "Bundled tutorial fixture" in quick_start
