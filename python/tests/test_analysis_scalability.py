from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKER = ROOT / "rust/crates/worker/src/preview.rs"
MODEL = ROOT / "App/WorkbenchModel.swift"
CLIENT = ROOT / "App/CoordinatorClient.swift"


class AnalysisScalabilityContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.worker = WORKER.read_text()
        cls.model = MODEL.read_text()
        cls.client = CLIENT.read_text()

    def test_cfg_and_predecessor_rendering_use_indexes(self):
        self.assertIn("let mut cfg_edges=vec![Vec::<(u64,u64,&str)>::new();cfg_limit]", self.worker)
        self.assertIn("let mut pred=vec![Vec::<usize>::new();limit]", self.worker)
        self.assertNotIn("edges.iter().filter(|(from,_,_)|*from>=start&&*from<end)", self.worker)
        self.assertNotIn("edges.iter().filter_map", self.worker)

    def test_structural_analysis_has_global_work_budgets(self):
        for token in [
            "MAX_STRUCTURAL_BLOCKS",
            "MAX_DOMINATOR_BLOCKS_PER_FUNCTION",
            "MAX_TOTAL_DOMINATOR_BLOCKS",
            "MAX_DOMINATOR_PASSES",
            "MAX_STRUCTURAL_PHI_BLOCKS",
            "MAX_PHI_OPERAND_BLOCKS",
        ]:
            self.assertIn(token, self.worker)
        self.assertNotIn("blocks.len().saturating_mul(2).max(1);let mut converged", self.worker)

    def test_iterative_value_flow_is_strictly_bounded(self):
        for token in [
            "MAX_ITERATIVE_VALUE_BLOCKS",
            "MAX_VALUE_FLOW_PASSES",
            "MAX_VALUE_PREDECESSORS",
            "MAX_REACHING_DEFS_PER_REGISTER",
        ]:
            self.assertIn(token, self.worker)
        self.assertNotIn("0..n.saturating_mul(4)", self.worker)

    def test_symbolication_does_not_rebuild_full_symbol_table(self):
        apple = self.worker.split("fn apple_symbolication_records", 1)[1].split("fn symbol_provenance_records", 1)[0]
        static = self.worker.split("fn symbolication_records", 1)[1].split("fn apple_symbolication_records", 1)[0]
        self.assertIn("symbolication_symbol_names", apple)
        self.assertNotIn("all_symbol_names", apple)
        self.assertIn("function_starts_present=has_function_starts(window)", static)
        self.assertEqual(self.worker.count("indirect_symbol_data(&mut file,&window,size)?"), 1)

    def test_relocation_lookup_is_indexed_in_hot_render_paths(self):
        self.assertGreaterEqual(self.worker.count("reloc_by_site"), 4)
        disassembly = self.worker.split("fn annotated_disassembly", 1)[1].split("fn classify_jump_candidate", 1)[0]
        indirect = self.worker.split("fn indirect_call_resolution", 1)[1].split("#[derive(Clone)]", 1)[0]
        self.assertNotIn("relocations.iter().find", disassembly)
        self.assertNotIn("relocations.iter().find", indirect)

    def test_top_level_name_resolution_borrows_owned_symbol_vector(self):
        # The top-level analysis function owns `symbols` as a Vec. The joined
        # resolver borrows it for exact-address lookup at each display site.
        analyze = self.worker.split('pub fn analyze', 1)[1]
        self.assertIn('resolved_function_name(start,&symbols,', analyze)
        self.assertIn('resolved_function_name(func,&symbols,', analyze)
        self.assertIn('resolved_function_name(target,&symbols,', analyze)
        self.assertIn('symbol_name_at(symbols,address)', self.worker)
        self.assertNotIn('let name=symbol_name_at(symbols,start)', analyze)
        self.assertNotIn('let caller=symbol_name_at(symbols,func)', analyze)
        self.assertNotIn('let callee=symbol_name_at(symbols,target)', analyze)

    def test_analysis_reply_decode_is_off_main_actor(self):
        self.assertIn("private struct VGAnalysisDecodeResult: Sendable", self.model)
        self.assertGreaterEqual(self.model.count("Task.detached(priority: .userInitiated)"), 2)
        self.assertNotIn("JSONDecoder().decode(AnalysisReply.self, from: Data(try result.get().utf8))", self.model)
        self.assertIn("let replyBytes = text.lengthOfBytes(using: .utf8)", self.client)
        self.assertNotIn("text.utf8.count <= maxReplyBytes", self.client)
        continuity = (ROOT / "App/AnalysisContinuity.swift").read_text()
        self.assertIn("Task.detached(priority: .utility)", continuity)
        self.assertIn("struct VGAnalysisSnapshot: Codable, Hashable, Sendable", continuity)


if __name__ == "__main__":
    unittest.main()
