"""Qualify real Apple runtime metadata and compiler declarations on macOS."""

import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / filename)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


NATIVE = module("collect_darwin_evidence", "collect-darwin-evidence.py")
SDK = module("darwin_sdk_catalog", "darwin_sdk_catalog.py")


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


if __name__ == "__main__":
    unittest.main()
