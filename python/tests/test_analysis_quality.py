import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKER = ROOT / "rust/crates/worker/src/preview.rs"
MESSAGES = ROOT / "Shared/WorkbenchMessages.swift"
CONTENT = ROOT / "App/ContentView.swift"
NAVIGATION = ROOT / "App/AnalysisNavigation.swift"

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

    def test_decompilation_corpus_checks_pseudocode_source_mapping(self):
        source = (ROOT / "scripts/run-decompilation-corpus.py").read_text()
        self.assertIn('"pseudocode_address_rows": len(pseudocode_sites)', source)
        self.assertIn('"source_mapping_valid_rows": len(pseudocode_sites & ir_sites)', source)
        self.assertIn("if not pseudocode_sites or pseudocode_sites - ir_sites:", source)

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

    def test_parameters_arguments_and_casts_have_bounded_provenance(self):
        source = WORKER.read_text()
        body = source[source.index("fn explicit_integer_cast"):source.index("fn annotated_disassembly")]
        self.assertIn("incoming-parameter-candidate", body)
        self.assertIn("entry-block-use-before-modeled-definition", body)
        self.assertIn("observed-argument-register-definitions", body)
        self.assertIn("explicit-integer-cast", body)
        self.assertIn("decoded-bitfield-alias", body)
        self.assertIn("zero-extend-u32-to-u64", body)
        self.assertIn("sign-extend-i32-to-i64", body)
        self.assertIn('(\"Arguments & Types\",\"Call Values\",\"Semantic\",call_values.as_str(),80)', source)
        self.assertIn('name: "Arguments & Types", workspace: "Call Values"', NAVIGATION.read_text())

    def test_swift_type_records_carry_validation_provenance(self):
        source = WORKER.read_text()
        body = source[source.index("fn swift_context_graph"):source.index("fn chained_page_starts")]
        self.assertIn("provenance=relative-pointer+mapped-cstring", body)
        self.assertIn("confidence=validated-record", body)

    def test_swift_field_records_bound_variable_claims(self):
        source = WORKER.read_text()
        body = source[source.index("fn swift_field_records"):source.index("fn predecessor_value_flow")]
        self.assertIn("Validated Swift field variables", body)
        self.assertIn("storage layout, runtime offsets, and source-level declarations remain unresolved", body)

    def test_stack_slot_candidates_bound_variable_recovery(self):
        source = (ROOT / "rust/crates/worker/src/preview.rs").read_text()
        body = source[source.index("fn stack_slot_value_shape"):source.index("fn analysis_quality_report")]
        self.assertIn("classification=stack-slot-candidate", body)
        self.assertIn("provenance=decoded-stack-relative-address", body)
        self.assertIn("type=unknown", body)
        self.assertIn("value-shape=", body)
        self.assertIn('"integer-u8"', body)

    def test_non_arm_semantic_hints_are_bounded_and_not_decompilation(self):
        source = (ROOT / "rust/crates/worker/src/binary.rs").read_text()
        body = source[source.index("fn semantic_hint"):source.index("pub fn analyze")]
        self.assertIn('"stack-operation"', body)
        self.assertIn('"data-movement"', body)
        self.assertIn('"compare-test"', body)
        self.assertIn('"indirect-call"', body)
        self.assertIn('"conditional-control-flow"', body)
        self.assertIn('non_arm_semantics_preserve_control_flow_shape', source)

if __name__ == "__main__":
    unittest.main()
