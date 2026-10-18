import contextlib
import hashlib
import importlib.util
import pathlib
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("hindsight_interop", ROOT / "python/hindsight_interop.py")
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
DIGEST = hashlib.sha256(b"captured artifact").hexdigest()


class Adapter:
    producer = "Fixture"

    def __init__(self, base=0x5000):
        self.base = base
        self.names = {base + 0x100: "original", base + 0x200: "same"}
        self.writes = []

    def source_sha256(self):
        return DIGEST

    def image_base(self):
        return self.base

    def functions(self):
        return ((address, name, "fixture assertion") for address, name in self.names.items())

    def name_at(self, address):
        return self.names.get(address)

    def rename(self, address, name):
        self.writes.append((address, name))
        self.names[address] = name

    def transaction(self):
        return contextlib.nullcontext()


class ReversingInterop(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = pathlib.Path(self.directory.name)
        self.path = self.root / "exchange.json"
        self.doc = m.exchange(DIGEST, 0x1000, "HindSight", [
            (0x1100, "renamed", "nlist"), (0x1200, "same", "analyst"), (0x1300, "absent", "candidate")])
        m.save(self.doc, self.path)

    def test_round_trip_rebase_preview_and_conflicts(self):
        adapter = Adapter()
        plan = m.import_adapter(adapter, self.path)
        self.assertEqual([r["status"] for r in plan], ["conflict", "same", "missing"])
        self.assertEqual(adapter.writes, [])
        plan = m.import_adapter(adapter, self.path, replace=True)
        self.assertEqual(plan[0]["address"], "0x5100")
        self.assertEqual(plan[0]["status"], "rename")
        self.assertEqual(adapter.writes, [])
        plan = m.import_adapter(adapter, self.path, replace=True, dry_run=False)
        self.assertEqual(adapter.writes, [(0x5100, "renamed")])
        self.assertEqual(plan[0]["status"], "applied")
        output = self.root / "back.json"
        self.assertEqual(m.export_adapter(adapter, output), 2)
        back = m.bounded_json(output)
        self.assertEqual(back["functions"][0]["offset"], "0x100")
        self.assertEqual(back["functions"][0]["name"], "renamed")

    def test_identity_mismatch_precedes_database_writes(self):
        adapter = Adapter()
        adapter.source_sha256 = lambda: "0" * 64
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            m.import_adapter(adapter, self.path, replace=True, dry_run=False)
        self.assertEqual(adapter.writes, [])

    def test_overflow_precedes_all_writes(self):
        adapter = Adapter(m.U64 - 0x200)
        with self.assertRaisesRegex(ValueError, "overflows"):
            m.import_adapter(adapter, self.path, replace=True, dry_run=False)
        self.assertEqual(adapter.writes, [])

    def test_rejected_tool_names_are_reported(self):
        adapter = Adapter()
        adapter.rename = lambda address, name: False
        plan = m.import_adapter(adapter, self.path, replace=True, dry_run=False)
        self.assertEqual(plan[0]["status"], "rejected")

    def test_invalid_schema_duplicates_controls_and_budgets(self):
        for change in [
            {"schema": "future"}, {"address_space": "runtime"}, {"source_sha256": "bad"},
            {"functions": self.doc["functions"] * 2}, {"functions": [None]},
            {"functions": [{"offset": "0x100", "name": "bad\nname", "provenance": "test"}]},
            {"source_image_base": "0xffffffffffffffff"},
        ]:
            with self.assertRaises(ValueError, msg=str(change)):
                m.validate(dict(self.doc, **change))
        with patch.object(m, "MAX_FUNCTIONS", 1):
            with self.assertRaises(ValueError):
                m.validate(self.doc)
        with patch.object(m, "MAX_BYTES", 10):
            with self.assertRaises(ValueError):
                m.bounded_json(self.path)

    def test_catalog_requires_matching_original_bytes(self):
        artifact = self.root / "binary"
        artifact.write_bytes(b"captured artifact")
        result = m.catalog(self.doc, artifact, 0x8000)
        self.assertIn("0x8100 renamed", result)
        self.assertIn("# source_sha256=" + DIGEST, result)
        self.assertIn("# provenance=nlist", result)
        artifact.write_bytes(b"different")
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            m.catalog(self.doc, artifact, 0x8000)

    def test_structural_and_arm64_report_rows(self):
        for rows in ["0x1100 name=my function source=nlist section=1\n",
                     "0x1100  0x1120  32 bytes  my function  source=nlist confidence=observed\n"]:
            document = m.from_report({"kind": "analysis_result", "source_sha256": DIGEST, "functions": rows}, 0x1000)
            self.assertEqual(document["functions"][0]["name"], "my function")
            self.assertEqual(document["functions"][0]["offset"], "0x100")
        with self.assertRaises(ValueError):
            m.from_report({"kind": "analysis_result", "source_sha256": DIGEST,
                           "functions": "0x1100  0x1120  32 bytes  <discovered>  source=branch confidence=candidate"}, 0x1000)

    def test_cli_and_output_collision(self):
        artifact = self.root / "binary"
        artifact.write_bytes(b"captured artifact")
        output = self.root / "names.txt"
        args = [sys.executable, str(ROOT / "python/hindsight_interop.py"), "to-catalog", str(self.path),
                "--artifact", str(artifact), "--image-base", "0x1000", "--output", str(output)]
        result = subprocess.run(args, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        before = output.read_bytes()
        result = subprocess.run(args, capture_output=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(output.read_bytes(), before)

    def test_binary_ninja_raw_identity_and_platform_ambiguity(self):
        artifact = self.root / "binary"
        artifact.write_bytes(b"captured artifact")
        raw = types.SimpleNamespace(length=17, read=lambda offset, count: b"captured artifact"[offset:offset + count])
        view = types.SimpleNamespace(file=types.SimpleNamespace(raw=raw),
                                     functions=[types.SimpleNamespace(start=0x1100, name="function")],
                                     get_functions_at=lambda address: [object(), object()])
        adapter = m.BinaryNinja(view, artifact, 0x1000)
        self.assertEqual(adapter.source_sha256(), DIGEST)
        with self.assertRaisesRegex(ValueError, "ambiguous"):
            adapter.name_at(0x1100)
        view.functions *= 2
        with self.assertRaisesRegex(ValueError, "multiple platform"):
            list(adapter.functions())
        raw.read = lambda offset, count: b""
        with self.assertRaisesRegex(ValueError, "short read"):
            adapter.source_sha256()

    def test_ida_only_matches_exact_entries_and_reports_name_rejection(self):
        functions = types.SimpleNamespace(get_func=lambda address: types.SimpleNamespace(start_ea=0x1100),
                                          get_func_name=lambda address: "existing")
        modules = {"ida_funcs": functions,
                   "ida_nalt": types.SimpleNamespace(retrieve_input_file_sha256=lambda: bytes.fromhex(DIGEST), get_imagebase=lambda: 0x1000),
                   "ida_name": types.SimpleNamespace(set_name=lambda *args: False, SN_CHECK=0)}
        with patch.dict(sys.modules, modules):
            adapter = m.IDA()
            self.assertEqual(adapter.source_sha256(), DIGEST)
            self.assertEqual(adapter.name_at(0x1100), "existing")
            self.assertIsNone(adapter.name_at(0x1101))
            self.assertFalse(adapter.rename(0x1100, "name"))

    def test_ghidra_high_addresses_and_transaction_rollback(self):
        events = []
        function = types.SimpleNamespace(getName=lambda: "kernel_entry")
        space = types.SimpleNamespace(getAddress=lambda value: int(value, 16))
        base = types.SimpleNamespace(getOffset=lambda: -4096, getAddressSpace=lambda: space)
        manager = types.SimpleNamespace(getFunctionAt=lambda location: function if location == m.U64 - 4095 else None)
        program = types.SimpleNamespace(getImageBase=lambda: base, getFunctionManager=lambda: manager,
                                        getExecutableSHA256=lambda: DIGEST,
                                        startTransaction=lambda label: 7,
                                        endTransaction=lambda identifier, commit: events.append((identifier, commit)))
        adapter = m.Ghidra(program)
        self.assertEqual(adapter.image_base(), m.U64 - 4095)
        self.assertEqual(adapter.name_at(m.U64 - 4095), "kernel_entry")
        with adapter.transaction():
            pass
        self.assertEqual(events, [(7, True)])
        with self.assertRaises(RuntimeError):
            with adapter.transaction():
                raise RuntimeError("rename failed")
        self.assertEqual(events[-1], (7, False))


if __name__ == "__main__":
    unittest.main()
