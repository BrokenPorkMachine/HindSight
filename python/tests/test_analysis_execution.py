import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKER = ROOT / "rust/crates/worker/src/preview.rs"
SUPERVISOR = ROOT / "PlatformServices/CoordinatorService/WorkerSupervisor.swift"
STORE = ROOT / "PlatformServices/CoordinatorService/ProjectStore.swift"
SERVICE = ROOT / "PlatformServices/CoordinatorService/main.swift"
MESSAGES = ROOT / "Shared/WorkbenchMessages.swift"
CONTENT = ROOT / "App/ContentView.swift"


class AnalysisExecutionContract(unittest.TestCase):
    def test_worker_progress_is_monotonic_and_bounded(self):
        source = WORKER.read_text()
        markers = [(int(p), stage) for p, stage in re.findall(r'analysis_progress\((\d+),"([^"]+)"', source)]
        self.assertGreaterEqual(len(markers), 7)
        percentages = [p for p, _ in markers]
        self.assertEqual(percentages, sorted(percentages))
        self.assertTrue(all(0 <= p <= 100 for p in percentages))
        self.assertEqual(len(percentages), len(set(percentages)))
        self.assertEqual(markers[0], (2, "verify"))
        self.assertEqual(markers[-1], (97, "finalize"))
        self.assertTrue({"metadata", "decode", "cfg", "synthesis", "evidence"}.issubset({s for _, s in markers}))

    def test_progress_transport_does_not_pollute_diagnostics(self):
        source = SUPERVISOR.read_text()
        self.assertIn('line.hasPrefix("VG_PROGRESS\\t")', source)
        self.assertIn('onProgress(percent, stage, detail); return', source)
        self.assertIn('progressPercent', source)
        self.assertIn('progressStage', source)
        self.assertIn('process.terminationReason == .uncaughtSignal', source)

    def test_cache_v3_preserves_v2_and_adds_typed_evidence_sideband(self):
        store = STORE.read_text()
        service = SERVICE.read_text()
        self.assertIn('analysisCacheSchema = 3', store)
        self.assertIn('AnalysisCacheEnvelopeV2', store)
        self.assertIn('let evidence: WorkerEvidenceBundle?', store)
        self.assertIn('engineVersion: String', store)
        self.assertIn('producerVersion: String', store)
        self.assertIn('LegacyAnalysisCacheEnvelope', store)
        self.assertIn('analysisCacheEngineVersion = "arm64-analysis-v83-schema122"', service)
        self.assertIn('compatibleLegacyAnalysisCacheVersions: Set<String> = ["0.4.1"]', service)
        self.assertIn('Project analysis cache could not be read; running a fresh worker analysis.', service)

    def test_status_and_ui_expose_determinate_progress(self):
        messages = MESSAGES.read_text()
        content = CONTENT.read_text()
        self.assertIn('case checkingCache', messages)
        self.assertIn('let progress_percent: Int', messages)
        self.assertIn('let progress_stage: String', messages)
        self.assertIn('ProgressView(value: progress, total: 100)', content)
        self.assertIn('accessibilityValue("\\(Int(progress)) percent")', content)


if __name__ == "__main__":
    unittest.main()
