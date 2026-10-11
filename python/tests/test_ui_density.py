import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class UIDensityContracts(unittest.TestCase):
    def text(self, relative: str) -> str:
        return (ROOT / relative).read_text()

    def test_shared_responsive_primitives_exist(self):
        chrome = self.text("App/WorkbenchChrome.swift")
        for token in [
            "struct VGFlowLayout: Layout",
            "struct VGKeyValueField: View",
            "struct VGTagPill: View",
            "struct VGAdaptiveThreePane",
            "struct VGAdaptiveTwoPane",
            "struct VGHorizontalOverflowBar",
        ]:
            self.assertIn(token, chrome)

    def test_three_pane_layout_has_testable_wide_medium_compact_modes(self):
        chrome = self.text("App/WorkbenchChrome.swift")
        for token in [
            "enum VGAdaptiveLayoutMode",
            "case wide",
            "case medium",
            "case compact",
            "static func resolve(width: CGFloat",
            "switch VGAdaptiveLayoutMode.resolve",
            "wideLayout",
            "mediumLayout",
            "compactLayout",
        ]:
            self.assertIn(token, chrome)

    def test_targets_surface_has_no_legacy_fixed_table(self):
        content = self.text("App/ContentView.swift")
        start = content.index("@ViewBuilder private func targetsWorkspace()")
        end = content.index("private func targetIcon", start)
        targets = content[start:end]
        for token in [
            "VGAdaptiveTwoPane(breakpoint: 980)",
            "VGFlowLayout",
            "LazyVGrid(columns: [GridItem(.adaptive(minimum: 220)",
            "ViewThatFits(in: .horizontal)",
            "VGKeyValueField(label: \"Evidence\"",
        ]:
            self.assertIn(token, targets)
        for legacy in [
            ".frame(width: 170",
            ".frame(width: 150",
            ".frame(width: 140",
            "Text(row.space).font(.system(.caption, design: .monospaced)).frame",
        ]:
            self.assertNotIn(legacy, targets)

    def test_dense_workspaces_use_wrapping_controls(self):
        expected = {
            "App/ContentView.swift": [
                "VGFlowLayout",
                "Apple SoC Virtualization Kit",
                "VectorEmu • deterministic execution microscope",
                "IPSW / SoC Catalog",
                "Hypervisor-assisted Reverse Engineering",
                "Extension Manager • Trust & Annotations",
            ],
            "App/BinaryWorkspace.swift": ["VGFlowLayout"],
            "App/GraphWorkspace.swift": ["VGFlowLayout"],
            "App/TraceExplorerWorkspace.swift": ["VGFlowLayout"],
            "App/RuntimeCorrelationWorkspace.swift": ["VGFlowLayout"],
            "App/EvidenceGraphWorkspace.swift": ["VGFlowLayout"],
            "App/InvestigationLedgerWorkspace.swift": ["VGFlowLayout"],
            "App/BinaryDiffWorkspace.swift": ["VGHorizontalOverflowBar", "ViewThatFits(in: .horizontal)"],
        }
        for path, tokens in expected.items():
            source = self.text(path)
            for token in tokens:
                self.assertIn(token, source, f"{path}: missing {token}")


    def test_evidence_surfaces_incrementally_reveal_rows(self):
        content = self.text("App/ContentView.swift")
        self.assertIn("private struct VGBoundedEvidenceList: View", content)
        self.assertIn("batchSize: Int = 200", content)
        self.assertIn(r'Button("Show next \(min(batchSize, rows.count - visibleCount))")', content)
        self.assertIn("rows.prefix(visibleCount)", content)
        self.assertNotIn(r'prefix(4000).enumerated()), id: \.offset', content)

    def test_content_view_does_not_restore_wide_fixed_control_columns(self):
        content = self.text("App/ContentView.swift")
        widths = [int(value) for value in re.findall(r"\.frame\(width:\s*(\d+)", content)]
        # Fixed widths in ContentView are reserved for icons, tiny progress/status
        # elements, and the compact VectorEmu port field. Wider evidence columns
        # belong in scroll-owning specialist views, not the general workbench.
        self.assertTrue(all(width <= 120 for width in widths), widths)

    def test_known_overlap_patterns_are_gone(self):
        content = self.text("App/ContentView.swift")
        forbidden = [
            'HStack { Button("Start", action: model.startVirtualMachine)',
            'HStack { TextField("SoC / platform filter (a12, t8020, a14…)',
            'HStack { Picker("Device",selection:$model.firmwareDeviceID)',
            'HStack { Picker("Build",selection:$model.firmwareBuildID)',
            'HStack { Button("Set Breakpoint"){model.vectorEmuDebug',
            'GroupBox("Context inspector") { monospaced(model.contextInspector()).frame(width: 280)',
        ]
        for token in forbidden:
            self.assertNotIn(token, content)


if __name__ == "__main__":
    unittest.main()
