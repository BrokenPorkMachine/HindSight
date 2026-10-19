import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
NAV = (ROOT / "App/AnalysisNavigation.swift").read_text()
CONTENT = (ROOT / "App/ContentView.swift").read_text()
CATALOG = (ROOT / "App/WorkspaceCatalog.swift").read_text()
TOPOLOGY = (ROOT / "Config/project-topology.json").read_text()


def test_structured_navigation_is_bounded_and_address_aware():
    assert 'limit: Int = 500' in NAV
    assert 'min(max(limit, 1), 1000)' in NAV
    assert 'limit: Int = 180' in NAV
    assert 'min(max(limit, 1), 300)' in NAV
    assert 'maxLineBytes = 16 * 1024' in NAV
    assert r'(?i)\b0x[0-9a-f]{1,16}\b' in NAV
    assert 'address: UInt64?' in NAV
    assert 'symbol: String?' in NAV


def test_focus_queue_is_explicitly_navigation_not_risk_scoring():
    assert 'source-prioritized investigation queue' in NAV
    assert 'not a vulnerability, severity, exploitability, or semantic-importance score' in NAV
    for source in [
        'Analysis Insights', 'Analysis Quality', 'Function Intelligence',
        'Function Neighborhoods', 'Data References', 'Symbol Callers',
        'Indirect Calls', 'Tail-Call Candidates', 'Import/Export Collisions'
    ]:
        assert source in NAV


def test_hits_route_to_source_code_cfg_and_symbol_without_protocol_changes():
    for destination in ['case .source:', 'case .disassembly:', 'case .cfg:', 'case .selectedSymbol:']:
        assert destination in NAV
    assert 'workspace = hit.workspace' in NAV
    assert 'workspace = "Disassembly"' in NAV
    assert 'workspace = "CFG"' in NAV
    assert 'workspace = "Selected Symbol"' in NAV
    assert 'AnalysisReport.jsonFields' not in NAV


def test_navigator_and_inspector_use_structured_evidence_views():
    assert 'case "Analysis Triage"' in CONTENT
    assert 'VGAnalysisTriageView(model: model)' in CONTENT
    assert 'VGAnalysisSearchResults(model: model)' in CONTENT
    assert 'VGRelatedAnalysisEvidenceView(model: model)' in CONTENT
    nav_start = CONTENT.index('case "Navigator":')
    nav_end = CONTENT.index('case "Dominators":', nav_start)
    assert 'monospaced(model.filteredAnalysisLines())' not in CONTENT[nav_start:nav_end]
    assert '"Analysis Triage"' in CATALOG
    assert 'App/AnalysisNavigation.swift' in TOPOLOGY


class CallEvidenceSurfaces(unittest.TestCase):
    """The report's three call-evidence fields must reach the workbench.

    Decoding a field in the transport contract is not the same as surfacing
    it, so each field needs a workspace, a navigable evidence source and a
    pane that states its own completeness honestly.
    """

    surfaces = {
        'typed_call_graph': 'Typed Call Graph',
        'call_effect_summaries': 'Call Effects',
        'prototype_conflicts': 'Prototype Conflicts',
    }

    def test_call_evidence_report_fields_are_surfaced_in_the_workbench(self):
        for field, workspace in self.surfaces.items():
            self.assertIn(f'"{workspace}"', CATALOG, workspace)
            self.assertIn(f'case "{workspace}":', CONTENT, workspace)
            self.assertIn(f'model.currentAnalysis?.{field}', CONTENT, field)
            self.assertIn(f'report.{field}', NAV, field)
            # A navigable hit routes by workspace name, so the catalog must name it.
            self.assertIn(f'workspace: "{workspace}"', NAV, workspace)

    def test_call_evidence_surfaces_stay_bounded_below_the_raw_call_graph(self):
        # The typed and effect views restate call edges the raw call graph
        # already carries. They must rank below it in the focus queue so the
        # bounded queue does not fill with near-duplicate call rows.
        def priority(field):
            match = re.search(r'text: report\.%s, focusPriority: (\d+)' % field, NAV)
            self.assertIsNotNone(match, field)
            return int(match.group(1))

        self.assertGreater(priority('call_graph'), priority('typed_call_graph'))
        self.assertGreater(priority('typed_call_graph'), priority('call_effect_summaries'))
        self.assertGreater(priority('call_effect_summaries'), priority('prototype_conflicts'))

    def test_prototype_conflict_surface_states_the_iteration_and_its_limit(self):
        # The report publishes the bounded caller/callee constraint iteration,
        # so the surface must state that a disagreement keeps both claims and
        # must not read as a clean result.
        start = CONTENT.index('case "Prototype Conflicts":')
        end = CONTENT.index('case "', start + 10)
        pane = CONTENT[start:end]
        self.assertIn('Caller/callee constraint iteration compares', pane)
        self.assertIn('both claims kept', pane)
        self.assertIn('no agreement is claimed', pane)


def test_generic_clickable_evidence_keeps_context_navigation():
    list_start = CONTENT.index('private struct VGBoundedEvidenceList: View')
    list_end = CONTENT.index('@MainActor\nstruct ContentView: View', list_start)
    renderer = CONTENT[list_start:list_end]
    start = CONTENT.index('@ViewBuilder private func clickableEvidence')
    end = CONTENT.index('@ViewBuilder private func pythonWorkspace', start)
    body = CONTENT[start:end]
    assert 'VGBoundedEvidenceList(model: model, text: text)' in body
    assert '.contextMenu' in renderer
    assert 'Open 0x' in renderer
    assert 'in Disassembly' in renderer
    assert 'in CFG' in renderer
