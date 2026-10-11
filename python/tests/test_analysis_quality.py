import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKER = ROOT / "rust/crates/worker/src/preview.rs"
MESSAGES = ROOT / "Shared/WorkbenchMessages.swift"
CONTENT = ROOT / "App/ContentView.swift"

class AnalysisQualityContract(unittest.TestCase):
    def test_external_calls_are_not_misattributed_to_last_internal_function(self):
        source = WORKER.read_text()
        body = source[source.index("fn function_intelligence"):source.index("fn sign_extend_u64")]
        self.assertIn("admitted_function_for(starts,target,scan_end)", body)
        self.assertIn("external_calls", body)
        self.assertNotIn("let callee=function_for(starts,target)", body)

    def test_tail_candidates_require_exact_function_entry(self):
        source = WORKER.read_text()
        body = source[source.index("fn cross_function_transfer_candidates"):source.index("fn function_neighborhood_report")]
        self.assertIn('kind!="branch"', body)
        self.assertIn("entries.contains(&target)", body)
        self.assertIn("caller==target", body)
        self.assertIn("classification=tail-call-candidate", body)

    def test_quality_ledger_reports_boundedness(self):
        source = WORKER.read_text()
        body = source[source.index("fn analysis_quality_report"):source.index("fn sign_extend_u64")]
        self.assertIn("Decoder coverage", body)
        self.assertIn("budget-skipped functions", body)
        self.assertIn("not silently treated as negative evidence", body)

    def test_transport_and_ui_expose_quality_surfaces(self):
        messages = MESSAGES.read_text()
        content = CONTENT.read_text()
        for field in ["tail_call_candidates", "function_neighborhoods", "analysis_quality"]:
            self.assertIn(f'let {field}: String', messages)
        for case in ['case "Analysis Quality"', 'case "Function Neighborhoods"', 'case "Tail-Call Candidates"']:
            self.assertIn(case, content)

    def test_call_signature_evidence_keeps_abi_and_observations_separate(self):
        source = WORKER.read_text()
        body = source[source.index("fn call_argument_provenance"):source.index("fn annotated_disassembly")]
        self.assertIn("AArch64 PCS signature evidence", body)
        self.assertIn("return-register=x0", body)
        self.assertIn("No source-level types or callee prototypes are inferred", body)

if __name__ == "__main__":
    unittest.main()
