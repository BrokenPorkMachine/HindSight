#!/usr/bin/env python3
"""Independent tests for truthful per-function recovery coverage reporting."""
import hashlib
import importlib.util
import pathlib
import tempfile
import unittest
from unittest.mock import patch

SOURCE = pathlib.Path(__file__).with_name("macho-reconstruction-coverage.py")
spec = importlib.util.spec_from_file_location("hindsight_macho_coverage", SOURCE)
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)


def fixture_manifest(states=("recovered", "review-only", "failed")):
    return {
        "source_sha256": "a" * 64,
        "discovered_in_scope": 4,
        "omitted_by_limit": 1,
        "functions": [
            {"start": hex(0x1000 + i * 0x100), "end": hex(0x1100 + i * 0x100),
             "state": state, "source_blockers": ["unknown alias"] if state != "recovered" else [],
             "unresolved_indirect_calls": int(state != "recovered"), "unresolved_dependencies": []}
            for i, state in enumerate(states)
        ],
    }


class CoverageTests(unittest.TestCase):
    def test_denominator_counts_review_only_and_failed(self):
        manifest = fixture_manifest()
        pairs = [(entry, {}) for entry in manifest["functions"]]
        with patch.object(mod, "read_batch_manifest", return_value=(manifest, b"source-manifest")), \
             patch.object(mod, "verified_outputs", return_value=pairs):
            result = mod.inspect_batch(pathlib.Path("/not/used"))
        self.assertEqual(result["attempted"], 3)
        self.assertEqual(result["states"], {"recovered": 1, "review_only": 1, "failed": 1})
        self.assertEqual(result["recovery_fraction"], 0.3333)
        self.assertEqual(result["omitted_by_limit"], 1)
        self.assertEqual(result["compilation_linkage"], "NOT TESTED")
        self.assertEqual(result["behavioral_equivalence"], "NOT TESTED")
        self.assertEqual(result["functions"][1]["blockers"], ["unknown alias"])

    def test_unrecognized_state_fails_closed(self):
        manifest = fixture_manifest(("impossible",))
        with patch.object(mod, "read_batch_manifest", return_value=(manifest, b"manifest")), \
             patch.object(mod, "verified_outputs", return_value=[(manifest["functions"][0], {})]):
            with self.assertRaises((KeyError, ValueError)):
                mod.inspect_batch(pathlib.Path("/not/used"))

    def test_bounded_64_bit_macho_identity(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = pathlib.Path(tmp) / "input"
            valid = bytes.fromhex("cffaedfe0c000001") + b"fixture-binary"
            p.write_bytes(valid)
            manifest = fixture_manifest(("recovered",))
            manifest["source_sha256"] = hashlib.sha256(valid).hexdigest()
            with patch.object(mod, "read_batch_manifest", return_value=(manifest, b"manifest")), \
                 patch.object(mod, "verified_outputs", return_value=[(manifest["functions"][0], {})]):
                self.assertEqual(mod.inspect_batch(pathlib.Path(tmp), p)["states"]["recovered"], 1)
                p.write_bytes(b"bad bytes")
                with self.assertRaisesRegex(ValueError, "source SHA-256"):
                    mod.inspect_batch(pathlib.Path(tmp), p)

    def test_symlink_source_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = pathlib.Path(tmp) / "original"
            p.write_bytes(bytes.fromhex("cffaedfe0c000001"))
            link = pathlib.Path(tmp) / "linked"
            link.symlink_to(p)
            manifest = fixture_manifest(("recovered",))
            with patch.object(mod, "read_batch_manifest", return_value=(manifest, b"manifest")), \
                 patch.object(mod, "verified_outputs", return_value=[(manifest["functions"][0], {})]):
                with self.assertRaisesRegex(ValueError, "non-symlink"):
                    mod.inspect_batch(pathlib.Path(tmp), link)


if __name__ == "__main__":
    unittest.main()
