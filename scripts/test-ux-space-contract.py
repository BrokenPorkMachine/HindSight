#!/usr/bin/env python3
"""Static regression checks for HindSight's space-allocation policy.

These checks do not replace Xcode compilation or visual/VoiceOver qualification.
"""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
INTEGRATED = ("Binary Diff", "Investigation Ledger", "Evidence Graph",
              "Trace Explorer", "Runtime Correlation")


class UXSpaceContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.chrome = (ROOT / "App/WorkbenchChrome.swift").read_text()
        cls.content = (ROOT / "App/ContentView.swift").read_text()
        cls.binary = (ROOT / "App/BinaryWorkspace.swift").read_text()

    def test_compact_dock_is_on_demand_not_permanent(self):
        first = self.chrome.index("private func compactLayout") if "private func compactLayout" in self.chrome else self.chrome.index("private var compactLayout")
        end = self.chrome.index("private var auxiliaryDock", first)
        compact = self.chrome[first:end]
        self.assertIn("compactDockPresented.toggle()", compact)
        self.assertIn(".popover(isPresented: $compactDockPresented", compact)
        self.assertNotIn("VSplitView", compact)
        self.assertIn("center", compact)

    def test_integrated_workspaces_start_without_duplicate_global_inspector(self):
        self.assertIn("private var hasIntegratedInspector", self.content)
        self.assertIn("inspectorOverrides[model.workspace] ?? !hasIntegratedInspector", self.content)
        self.assertIn("if inspectorIsVisible {", self.content)
        self.assertIn("Button(action: toggleWorkspaceInspector)", self.content)
        for name in ["CFG", "Binary Workspace", "Runtime Correlation",
                     "Trace Explorer", "Evidence Graph", "Investigation Ledger"]:
            self.assertIn('"' + name + '"', self.content)

    def test_focus_mode_does_not_erase_inspector_preferences(self):
        start = self.content.index("private func toggleFocusMode()")
        end = self.content.index("private var hasNextPage:", start)
        section = self.content[start:end]
        self.assertIn("focusMode.toggle()", section)
        self.assertNotIn("inspectorOverrides =", section)

    def test_binary_view_is_code_first_with_explicit_split(self):
        self.assertIn("@State private var centerMode: VGBinaryCenterMode = .code", self.binary)
        self.assertIn("case .split:", self.binary)
        self.assertIn("case .lifted:", self.binary)

    def test_integrated_viewports_not_forced_to_650_points(self):
        for name in INTEGRATED:
            start = self.content.index('            case "' + name + '":')
            end = self.content.find('\n            case "', start + 16)
            if end == -1:
                end = len(self.content)
            section = self.content[start:end]
            self.assertNotIn(".frame(minHeight: 650)", section, name)

    def test_main_window_minimum_is_still_supported(self):
        self.assertIn(".frame(minWidth: 900, minHeight: 680)", self.content)


if __name__ == "__main__":
    unittest.main()
