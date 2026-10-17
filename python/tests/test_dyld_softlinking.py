from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]

def read(path):
    return (ROOT / path).read_text()

class DyldSoftLinkingContract(unittest.TestCase):
    def test_legacy_dyld_opcode_decoder_is_wired(self):
        worker = read('rust/crates/worker/src/preview.rs')
        for token in ['fn dyld_info_ranges(', 'fn dyld_bind_stream(', 'fn dyld_rebase_stream(', 'fn dyld_linking_records(', 'weak-bind', 'lazy-bind']:
            self.assertIn(token, worker)
        messages = read('Shared/WorkbenchMessages.swift')
        self.assertIn('"dyld_linking"', messages)
        self.assertIn('let dyld_linking: String', messages)
        self.assertIn('case "Dyld Linking"', read('App/ContentView.swift'))

    def test_softlinking_decoder_is_evidence_based(self):
        worker = read('rust/crates/worker/src/preview.rs')
        for token in ['fn softlinking_records(', 'weak-load', 'softlinking.framework', '_dlopen', '_dlsym', '_NSClassFromString']:
            self.assertIn(token, worker)
        messages = read('Shared/WorkbenchMessages.swift')
        self.assertIn('"softlinking"', messages)
        self.assertIn('case "Soft Linking"', read('App/ContentView.swift'))

    def test_cache_reader_is_bounded_and_mapping_backed(self):
        host = read('PlatformServices/CoordinatorService/FirmwareRuntimeHost.swift')
        for token in ['case "dyld_read"', 'private func readDyldCache(', '64 * 1024', 'Address is not backed by a primary-cache file mapping']:
            self.assertIn(token, host)
        messages = read('Shared/FirmwareRuntimeMessages.swift')
        self.assertIn('struct DyldCacheMappingRecord', messages)
        self.assertIn('let mappings: [DyldCacheMappingRecord]', messages)

    def test_schema_identity_advanced_for_new_fields(self):
        self.assertIn('arm64-analysis-v71-schema117', read('PlatformServices/CoordinatorService/main.swift'))
        self.assertIn('precondition(AnalysisReport.jsonFields.count == 72)', read('Tests/analysis_protocol_contract.swift'))

if __name__ == '__main__':
    unittest.main()
