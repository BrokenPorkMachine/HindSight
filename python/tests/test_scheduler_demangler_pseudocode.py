import pathlib
import subprocess
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]

class SchedulerDemanglePseudocodeContracts(unittest.TestCase):
    def test_priority_queue_has_cancellation_and_lease_checks(self):
        code = (ROOT/'rust/crates/scheduler/src/lib.rs').read_text()
        for token in ('pub struct PriorityScheduler', 'cancel_older_than', 'can_publish', 'dispatch_next', 'QueueError::Full', 'mod priority_tests'):
            self.assertIn(token, code)

    def test_coordinator_lane_has_priority_and_generation(self):
        code = (ROOT/'PlatformServices/CoordinatorService/main.swift').read_text()
        self.assertIn('enum Priority: Int', code)
        self.assertIn('func cancelQueued()', code)
        self.assertIn('generation == job.generation', code)
        self.assertIn('scriptingLane.submit(priority: .initiated', code)
        self.assertIn('firmwareLane.submit(priority: .initiated', code)

    def test_analysis_and_page_requests_reach_priority_worker_queue(self):
        source = (ROOT/'PlatformServices/CoordinatorService/main.swift').read_text()
        page = source.split('func instructionPage(', 1)[1].split('func analyzeRecord(', 1)[0]
        analysis = source.split('func analyzeRecord(', 1)[1].split('func cancelInspection(', 1)[0]
        self.assertNotIn('guard !supervisor.isBusy', page)
        self.assertNotIn('guard !supervisor.isBusy', analysis)
        self.assertIn('supervisor.page(', page)
        self.assertIn('supervisor.analyze(', analysis)

    def test_ir_preview_is_bounded_and_explicitly_structural(self):
        code = (ROOT/'App/ContentView.swift').read_text()
        self.assertIn('vgStructuralPseudocode', code)
        self.assertIn('prefix(120)', code)
        self.assertIn('Structural pseudocode preview', code)

    def test_cli_references_swift_demangler_without_shell(self):
        code = (ROOT/'scripts/vg-symbol-tools.swift').read_text()
        self.assertIn('swift-demangle', code)
        self.assertIn('task.arguments', code)
        self.assertIn('pseudocode(_ ir:', code)
        self.assertNotIn('/bin/sh', code)

if __name__ == '__main__':
    unittest.main()
