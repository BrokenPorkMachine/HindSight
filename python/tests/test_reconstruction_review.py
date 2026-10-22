import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


class ReconstructionReviewContracts(unittest.TestCase):
    def test_source_pane_cancels_only_its_active_reconstruction(self):
        model = (ROOT / "App/WorkbenchModel.swift").read_text()
        content = (ROOT / "App/ContentView.swift").read_text()
        cancel = model.split("func cancelReconstruction()", 1)[1].split("func exportReconstruction", 1)[0]
        self.assertIn("reconstructionRequestID == id", cancel)
        self.assertIn("activeRequest == id", cancel)
        self.assertIn("service.cancelInspection(id)", cancel)
        self.assertIn("Cancellation accepted; waiting for the worker to stop.", cancel)
        self.assertIn("The reconstruction had already finished; validating its reply.", cancel)
        self.assertIn("onCancelReconstruction: model.cancelReconstruction", content)

    def test_expression_evidence_keeps_instruction_origin_navigation(self):
        view = (ROOT / "App/BinaryWorkspace.swift").read_text()
        self.assertIn("AST expressions and instruction origins", view)
        self.assertIn("ForEach(expression.ranges", view)
        self.assertIn("selectAddress(span.start)", view)
        self.assertIn("selectAddress(span.start)", view.split('DisclosureGroup("Why:', 1)[1])


if __name__ == "__main__":
    unittest.main()
