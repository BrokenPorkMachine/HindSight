import json
import pathlib
import subprocess
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


class ReleaseGateCoverageTests(unittest.TestCase):
    def target_sources(self, target: str):
        topology = json.loads((ROOT / "Config/project-topology.json").read_text())
        return [str((ROOT / item).resolve()) for item in topology["targets"][target] if item.endswith(".swift")]

    def helper_sources(self, target: str):
        result = subprocess.run(
            [sys.executable, str(ROOT / "scripts/list-target-swift-sources.py"), target],
            check=True,
            capture_output=True,
            text=True,
        )
        return [line for line in result.stdout.splitlines() if line]

    def test_app_typecheck_covers_generated_target(self):
        expected = self.target_sources("HindSight")
        self.assertEqual(self.helper_sources("HindSight"), expected)
        self.assertIn(str((ROOT / "App/AnalysisNavigation.swift").resolve()), expected)
        self.assertIn(str((ROOT / "App/AnalysisContinuity.swift").resolve()), expected)
        gate = (ROOT / "scripts/test-app-typecheck.sh").read_text()
        self.assertIn('list-target-swift-sources.py" HindSight', gate)

    def test_coordinator_typecheck_covers_generated_target(self):
        expected = self.target_sources("CoordinatorService")
        self.assertEqual(self.helper_sources("CoordinatorService"), expected)
        gate = (ROOT / "scripts/test-coordinator-typecheck.sh").read_text()
        self.assertIn('list-target-swift-sources.py" CoordinatorService', gate)

    def test_unknown_target_fails_closed(self):
        result = subprocess.run(
            [sys.executable, str(ROOT / "scripts/list-target-swift-sources.py"), "NotARealTarget"],
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown generated target", result.stderr)


if __name__ == "__main__":
    unittest.main()
