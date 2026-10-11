from pathlib import Path
import unittest
ROOT = Path(__file__).resolve().parents[1]
class StartupGovernorContract(unittest.TestCase):
    def test_no_forced_connect_under_pressure(self):
        s=(ROOT/'App/WorkbenchModel.swift').read_text()
        self.assertIn('while self.shouldDeferBackgroundStartup',s)
        self.assertNotIn('for _ in 0..<20',s)
        self.assertIn('guard !Task.isCancelled else { return }',s)
    def test_manual_override(self):
        s=(ROOT/'App/WorkbenchModel.swift').read_text()
        self.assertIn('func connect(force: Bool = false)',s)
        self.assertIn('startupTask?.cancel()',s)
if __name__=='__main__': unittest.main()
