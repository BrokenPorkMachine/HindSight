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

    def test_runtime_contract_includes_case_event_dependency(self):
        validator = (ROOT / "scripts/validate-source.py").read_text()
        runtime_block = validator.split("vectorglass-runtime-correlation-", 1)[1].split("vectorglass-trace-explorer-", 1)[0]
        self.assertIn("App/InvestigationLedgerModel.swift", runtime_block)
        trace_block = validator.split("vectorglass-trace-explorer-", 1)[1].split("vectorglass-evidence-graph-", 1)[0]
        self.assertIn("App/InvestigationLedgerModel.swift", trace_block)

    def test_bridge_sources_are_ready_for_single_source_and_test_module_lints(self):
        build_script = (ROOT / "rust/crates/bridge/build.rs").read_text()
        bridge_source = (ROOT / "rust/crates/bridge/src/lib.rs").read_text()
        self.assertIn('let source="cache";', build_script)
        self.assertGreater(bridge_source.rfind("#[cfg(test)]"), bridge_source.rfind("pub unsafe extern"))

    def test_beta_gate_handles_optional_release_arguments_and_current_capability(self):
        gate = (ROOT / "scripts/test-beta-macos.sh").read_text()
        smoke = (ROOT / "Tests/bridge_smoke.c").read_text()
        self.assertIn('if ((${#release_build_args[@]})); then', gate)
        self.assertIn('"\\"analysis\\":true"', smoke)

    def test_release_preflight_is_non_secret_and_checks_universal_bundle(self):
        preflight = (ROOT / "scripts/release-preflight.sh").read_text()
        self.assertIn("security find-identity", preflight)
        self.assertNotIn("notarytool submit", preflight)
        self.assertIn("lipo -archs", preflight)
        self.assertIn("manual Intel/Apple-Silicon qualification matrix", preflight)


if __name__ == "__main__":
    unittest.main()
