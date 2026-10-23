"""Qualify real Apple runtime metadata and compiler declarations on macOS."""

import importlib.util
from pathlib import Path
import sys
import struct
import json
import subprocess
import io
from contextlib import redirect_stdout
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / filename)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


NATIVE = module("collect_darwin_evidence", "collect-darwin-evidence.py")
SDK = module("darwin_sdk_catalog", "darwin_sdk_catalog.py")


class SDKImportSelectionTests(unittest.TestCase):
    def test_redeclaration_comparison_ignores_only_top_level_restrict(self):
        self.assertEqual(SDK._parameter_abi_key({"qualType": "const void *restrict"}), "const void *")
        self.assertNotEqual(SDK._parameter_abi_key({"qualType": "const void *restrict"}), "void *")
        self.assertEqual(SDK._parameter_abi_key({"qualType": "size_t", "desugaredQualType": "unsigned long"}), "unsigned long")

    def test_direct_worker_lookup_validates_frame_and_clears_inherited_prototypes(self):
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "capture"
            binary.write_bytes(b"fixture")
            worker = Path(directory) / "worker"
            worker.write_bytes(b"fixture")
            raw = json.dumps({"kind": "analysis_result", "source_sha256": "a" * 64}).encode()
            frame = struct.pack(">I", len(raw)) + raw
            command = ["darwin_sdk_catalog.py", str(binary), "--worker", str(worker)]
            with patch.object(sys, "argv", command), patch.object(SDK, "verify_macho_target", return_value="a" * 64), patch.object(SDK.subprocess, "run", return_value=SimpleNamespace(stdout=frame)) as run, patch.object(SDK, "collect_import_declarations", return_value={"verified": True}) as collect, patch.dict(SDK.os.environ, {"HINDSIGHT_PROTOTYPE_DB": "/unrelated/catalog"}), redirect_stdout(io.StringIO()) as output:
                SDK.main()
                self.assertEqual(json.loads(output.getvalue()), {"verified": True})
                self.assertNotIn("HINDSIGHT_PROTOTYPE_DB", run.call_args.kwargs["env"])
                self.assertEqual(run.call_args.args[0][1], "--analyze")
                self.assertEqual(collect.call_args.args[1]["source_sha256"], "a" * 64)
                run.return_value.stdout = struct.pack(">I", len(raw)+1) + raw
                with self.assertRaisesRegex(ValueError, "analysis frame"):
                    SDK.main()
                self.assertEqual(collect.call_count, 1)

    def test_import_collector_binds_source_and_retains_provider(self):
        report = {"source_sha256": "a" * 64, "linkage_records": "symbol=_malloc import[0] dylib=/usr/lib/libSystem.B.dylib addend=0 weak=no stub=0x1000\n"}
        with patch.object(SDK, "verify_macho_target", return_value="a" * 64), patch.object(SDK, "collect_declarations", return_value={"functions": [{"name": "malloc", "symbol": "_malloc"}]}) as collect:
            catalog = SDK.collect_import_declarations(Path("capture"), report)
            self.assertEqual(collect.call_args.args[0], ["malloc"])
            self.assertEqual(catalog["source_sha256"], "a" * 64)
            self.assertEqual(catalog["functions"][0]["captured_provider"], "/usr/lib/libSystem.B.dylib")
            with self.assertRaisesRegex(ValueError, "source differs"):
                SDK.collect_import_declarations(Path("capture"), {**report, "source_sha256": "b" * 64})
            self.assertEqual(collect.call_count, 1)

    def test_public_imports_retain_provider_and_reject_ambiguous_or_private_matches(self):
        def row(symbol, provider, addend=0):
            return f"symbol={symbol} import[0] dylib={provider} addend={addend} weak=no stub=0x1000\n"
        records = row("_malloc", "/usr/lib/libSystem.B.dylib")
        records += row("_CFRelease", "/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation")
        records += row("_private", "/System/Library/PrivateFrameworks/Hidden.framework/Hidden")
        records += row("_special", "<special>")
        records += row("_offset", "/usr/lib/libSystem.B.dylib", 8)
        records += row("_collision", "/usr/lib/libSystem.B.dylib") + row("_collision", "/tmp/custom.dylib")
        selected = SDK.sdk_import_candidates(records)
        self.assertEqual([item["name"] for item in selected], ["CFRelease", "malloc"])
        self.assertEqual(selected[1]["provider"], "/usr/lib/libSystem.B.dylib")


@unittest.skipUnless(sys.platform == "darwin", "native Darwin catalog requires macOS")
class DarwinCatalogTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory(prefix="hindsight-native-catalog-test-")
        cls.addClassCleanup(cls.directory.cleanup)
        cls.helper = Path(cls.directory.name) / "native-catalog"
        NATIVE.compile_helper(cls.helper, [ROOT / "Tests/Fixtures/Decompilation/darwin_catalog_sample.m"])
        cls.catalog = NATIVE.collect(cls.helper, ["HSNativeCatalogFixture", "HSNativeMissingClass"], ["malloc", "HSNativeMissingSymbol"])
        cls.header = ROOT / "Tests/Fixtures/Decompilation/darwin_catalog_sample.h"
        cls.declarations = SDK.collect_declarations(["HSNativeCatalogCount", "HSNativeCatalogCallback"], cls.header)

    def test_loaded_image_identity_and_symbol_ownership(self):
        record = self.catalog["symbols"][0]
        self.assertEqual(record["lookup_name"], "malloc")
        image = record["image"]
        self.assertRegex(image["uuid"], r"^[0-9a-f-]{36}$")
        self.assertTrue(image["install_name"].endswith(".dylib"))
        self.assertGreater(int(record["runtime_address"], 16), 0)
        self.assertEqual(int(image["preferred_address"], 16) - int(image["image_offset"], 16),
                         int(image["preferred_address"], 16) - (int(record["runtime_address"], 16) - int(image["runtime_base"], 16)))
        self.assertEqual(self.catalog["missing_classes"], ["HSNativeMissingClass"])
        self.assertEqual(self.catalog["missing_symbols"], ["HSNativeMissingSymbol"])

    def test_class_names_selectors_and_method_abi_types(self):
        cls = self.catalog["classes"][0]
        self.assertEqual(cls["name"], "HSNativeCatalogFixture")
        self.assertEqual(cls["superclass"], "NSObject")
        self.assertEqual(cls["metaclass_name"], cls["name"])
        methods = {method["selector"]: method for method in cls["instance_methods"]}
        score = methods["score:count:"]
        self.assertEqual(score["name"], "-[HSNativeCatalogFixture score:count:]")
        self.assertEqual([argument["encoding"] for argument in score["arguments"]], ["@", ":", "d", "Q"])
        self.assertEqual(score["return_type"]["size"], 8)
        self.assertRegex(score["implementation_image"]["uuid"], r"^[0-9a-f-]{36}$")
        self.assertEqual(score["implementation_image"]["uuid"], cls["image"]["uuid"])
        self.assertEqual(methods["pair"]["return_type"]["size"], 16)
        self.assertEqual(methods["pair"]["return_type"]["alignment"], 8)
        self.assertIn("rangeForCount:", {method["selector"] for method in cls["class_methods"]})

    def test_properties_and_ivars_retain_native_attributes_and_offsets(self):
        cls = self.catalog["classes"][0]
        properties = {record["name"]: record for record in cls["properties"]}
        label = properties["label"]["fields"]
        self.assertEqual(label["T"], '@"NSString"')
        self.assertEqual(label["G"], "displayLabel")
        self.assertEqual(label["S"], "setDisplayLabel:")
        self.assertEqual(label["V"], "_label")
        self.assertIn("C", label)
        ivars = {record["name"]: record for record in cls["ivars"]}
        for name in ["_count", "_weight", "_label"]:
            record = ivars[name]
            self.assertGreaterEqual(record["offset"], 0)
            self.assertLessEqual(record["offset"] + record["type"]["size"], cls["instance_size"])
        self.assertEqual(ivars["_weight"]["type"]["encoding"], "d")

    def test_sdk_struct_fields_agree_with_lp64_layout(self):
        types = {record["name"]: record for record in self.catalog["types"]}
        self.assertEqual(types["NSRange"]["size"], 16)
        self.assertEqual(types["NSRange"]["fields"], [{"name": "location", "offset": 0, "size": 8}, {"name": "length", "offset": 8, "size": 8}])
        self.assertEqual(types["CGRect"]["size"], 32)
        self.assertEqual(types["CGRect"]["fields"][1]["offset"], 16)
        self.assertEqual(types["mach_msg_header_t"]["size"], 24)
        self.assertFalse(self.catalog["truncated"])

    def test_clang_declaration_flows_to_source_bound_worker_prototype(self):
        record = self.declarations["functions"][0]
        self.assertEqual(record["symbol"], "_HSNativeCatalogCount")
        self.assertEqual(record["declaration"], "unsigned long HSNativeCatalogCount(unsigned long value);")
        self.assertEqual(record["parameters"][0]["name"], "value")
        catalog = {**self.declarations, "functions": [record]}
        emitted = SDK.worker_prototypes(catalog, "1" * 64)
        self.assertIn("# source_sha256=" + "1" * 64, emitted)
        self.assertIn("_HSNativeCatalogCount\tunsigned long HSNativeCatalogCount(unsigned long value);", emitted)

    def test_import_selected_public_sdk_declaration_is_compiler_derived(self):
        target = self.catalog["architecture"] + "-apple-macos14.0"
        digest = SDK.verify_macho_target(self.helper, target)
        report = {"source_sha256": digest, "linkage_records": "symbol=_malloc import[0] dylib=/usr/lib/libSystem.B.dylib addend=0 weak=no stub=0x1000\n"}
        catalog = SDK.collect_import_declarations(self.helper, report, target=target)
        self.assertEqual(catalog["missing_functions"], [])
        record = catalog["functions"][0]
        self.assertEqual(record["name"], "malloc")
        self.assertEqual(record["captured_provider"], "/usr/lib/libSystem.B.dylib")
        self.assertIn("void *", record["abi_declaration"])
        self.assertIn("_malloc\t", SDK.worker_prototypes(catalog, digest))
        analysis = Path(self.directory.name) / "sdk-import-analysis.json"
        analysis.write_text(json.dumps({**report, "kind": "analysis_result"}))
        command = [sys.executable, str(ROOT / "scripts/darwin_sdk_catalog.py"), str(self.helper), str(analysis), "--target", target]
        evidence = json.loads(subprocess.run(command, check=True, capture_output=True, timeout=60).stdout)
        self.assertEqual(evidence["source_sha256"], digest)
        self.assertEqual(evidence["functions"][0]["captured_provider"], record["captured_provider"])
        prototypes = subprocess.run([*command, "--worker-prototypes"], check=True, capture_output=True, text=True, timeout=60).stdout
        self.assertIn("# source_sha256=" + digest, prototypes)
        self.assertIn("_malloc\t", prototypes)

    def test_direct_lookup_joins_real_worker_imports_to_sdk(self):
        worker = ROOT / "rust/target/debug/reworker"
        if not worker.is_file() or self.catalog["architecture"] != "arm64":
            self.skipTest("requires the built AArch64 worker")
        command = [sys.executable, str(ROOT / "scripts/darwin_sdk_catalog.py"), str(self.helper),
                   "--worker", str(worker), "--target", "arm64-apple-macos14.0"]
        result = subprocess.run(command, capture_output=True, timeout=180)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
        catalog = json.loads(result.stdout)
        self.assertTrue(catalog["import_candidates"])
        self.assertTrue(catalog["functions"])
        names = {item["name"] for item in catalog["import_candidates"]}
        self.assertTrue(all(record["name"] in names and record["captured_provider"] for record in catalog["functions"]))
        self.assertEqual(catalog["source_sha256"], SDK.verify_macho_target(self.helper, "arm64-apple-macos14.0"))

    def test_callback_declarators_remain_evidence_without_false_scalar_prototype(self):
        record = self.declarations["functions"][1]
        self.assertFalse(record["worker_eligible"])
        self.assertIn("(*)(int)", record["parameters"][0]["type"]["qualType"])
        with self.assertRaisesRegex(ValueError, "unsupported prototype"):
            SDK.worker_prototypes(self.declarations, "1" * 64)

    def test_request_bounds_and_target_sdk_mismatches_are_rejected(self):
        with self.assertRaises(ValueError):
            NATIVE.collect(self.helper, ["NSObject"] * 33, [])
        with self.assertRaises(ValueError):
            SDK.collect_declarations(["bad;identifier"], self.header)
        with self.assertRaisesRegex(ValueError, "disagree"):
            SDK.collect_declarations(["HSNativeCatalogCount"], self.header, sdk="macosx", target="arm64-apple-ios16.0")

    def test_source_binding_checks_actual_macho_architecture_and_platform(self):
        host_target = self.catalog["architecture"] + "-apple-macos14.0"
        SDK.verify_macho_target(self.helper, host_target)
        with self.assertRaisesRegex(ValueError, "platform differs"):
            SDK.verify_macho_target(self.helper, self.catalog["architecture"] + "-apple-ios16.0")
        other = "x86_64" if self.catalog["architecture"] == "arm64" else "arm64"
        with self.assertRaisesRegex(ValueError, "architecture differs"):
            SDK.verify_macho_target(self.helper, other + "-apple-macos14.0")
        malformed = Path(self.directory.name) / "malformed.macho"
        raw = bytearray(self.helper.read_bytes())
        struct.pack_into("<I", raw, 20, 0xffffffff)
        malformed.write_bytes(raw)
        with self.assertRaisesRegex(ValueError, "budget exceeded"):
            SDK.verify_macho_target(malformed, host_target)


if __name__ == "__main__":
    unittest.main()
