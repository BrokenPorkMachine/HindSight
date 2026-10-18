from pathlib import Path
import unittest
ROOT=Path(__file__).resolve().parents[2]
def read(p): return (ROOT/p).read_text()
class SymbolicationContract(unittest.TestCase):
 def test_static_symbolication_is_wired(self):
  w=read("rust/crates/worker/src/preview.rs")
  for t in ["fn macho_uuid(","fn symbolication_records(","fn compact_unwind_records(","LC_FUNCTION_STARTS","debug-section","compact-unwind","image_uuid="]:
   self.assertIn(t,w)
  m=read("Shared/WorkbenchMessages.swift")
  self.assertIn('"symbolication"',m); self.assertIn('let symbolication: String',m)
  self.assertIn('case "Symbolication"',read("App/ContentView.swift"))
 def test_apple_symbolication_decoder_is_static(self):
  w=read("rust/crates/worker/src/preview.rs")
  for t in ["fn apple_symbolication_records(","symbolication.framework","cssymbolicator","does not invoke private Symbolication/CoreSymbolication APIs"]:
   self.assertIn(t.lower(),w.lower())
  self.assertIn('case "Apple Symbolication"',read("App/ContentView.swift"))
 def test_unified_symbol_resolver_is_wired(self):
  w=read("rust/crates/worker/src/preview.rs")
  for t in ["fn unified_symbol_records(","Precedence: exact nlist code symbol","name-only symbol=","symbol_resolver"]:
   self.assertIn(t,w)
  m=read("Shared/WorkbenchMessages.swift")
  self.assertIn('"symbol_resolver"',m); self.assertIn('let symbol_resolver: String',m)
  self.assertIn('case "Symbol Resolver"',read("App/ContentView.swift"))
  self.assertIn('workspace: "Symbol Resolver"',read("App/AnalysisNavigation.swift"))
 def test_schema_advanced(self):
  self.assertIn("arm64-analysis-v89-schema122",read("PlatformServices/CoordinatorService/main.swift"))
  self.assertIn("jsonFields.count == 72",read("Tests/analysis_protocol_contract.swift"))
if __name__=="__main__": unittest.main()
