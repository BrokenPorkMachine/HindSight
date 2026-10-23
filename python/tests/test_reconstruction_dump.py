"""Exercise the actual CLI exporter with independently valid review evidence."""
import hashlib
import importlib.util
import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

from test_reconstruction_schema import fixture, MODULE

ROOT = Path(__file__).resolve().parents[2]


class ReconstructionDumpTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="hindsight-dump-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.batch = self.root / "batch"
        self.batch.mkdir()
        self.artifact = json.dumps(fixture()).encode()
        self.name = "function-0000000000001000.reconstruction.json"
        (self.batch / self.name).write_bytes(self.artifact)
        self.manifest = {
            "schema": 1, "source_sha256": "1" * 64, "worker_sha256": "2" * 64,
            "prototypes_sha256": "3" * 64, "source_bytes": 8192,
            "scope": {"start": 4096, "end": 4104},
            "functions": [{"start": "0x1000", "end": "0x1008", "state": "review-only",
                           "files": [{"path": self.name, "bytes": len(self.artifact),
                                      "sha256": hashlib.sha256(self.artifact).hexdigest()}]}]}
        self.publish()

    def publish(self):
        self.raw_manifest = json.dumps(self.manifest).encode()
        (self.batch / "manifest.json").write_bytes(self.raw_manifest)

    def dump(self, destination, *arguments):
        return subprocess.run([sys.executable, str(ROOT / "scripts/export-reconstruction-batch.py"),
                               str(self.batch), str(destination), "--format", "c", *arguments],
                              capture_output=True, timeout=30)

    def test_review_only_retains_graph_and_producer_identity_without_body(self):
        output = self.root / "dump"
        result = self.dump(output)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads((output / "export-manifest.json").read_bytes())
        for key in ["worker_sha256", "prototypes_sha256", "source_bytes"]:
            self.assertEqual(report[key], self.manifest[key])
        self.assertEqual(report["batch_scope"], self.manifest["scope"])
        self.assertEqual(report["source_batch_manifest_sha256"], hashlib.sha256(self.raw_manifest).hexdigest())
        self.assertEqual((report["available"], report["unavailable"]), (0, 1))
        entry = report["functions"][0]
        self.assertEqual(entry["state"], entry["recovery_state"])
        self.assertEqual(entry["state"], "review-only")
        self.assertEqual((output / self.name).read_bytes(), self.artifact)
        self.assertEqual(sorted(path.name for path in output.iterdir()),
                         sorted([self.name, "export-manifest.json"]))

    def test_failed_function_is_unavailable_not_an_empty_source(self):
        self.manifest["functions"][0].update(state="failed", files=[])
        self.publish()
        output = self.root / "failed"
        result = self.dump(output)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads((output / "export-manifest.json").read_bytes())
        self.assertEqual(report["functions"][0]["files"], [])
        self.assertEqual((report["available"], report["unavailable"]), (0, 1))

    def test_refusals_do_not_publish_destination(self):
        for name, arguments in [("budget", ["--max-bytes", "1"]),
                                ("empty", ["--address", "0x2000"]),
                                ("mixed", ["--address", "0x1000", "--start", "0x1000", "--end", "0x1008"])]:
            with self.subTest(name=name):
                output = self.root / name
                self.assertNotEqual(self.dump(output, *arguments).returncode, 0)
                self.assertFalse(output.exists())

    def test_rehashed_graph_cannot_change_source_identity(self):
        # A fully valid graph from a different binary is still not this batch.
        raw = json.dumps(fixture(source="4" * 64)).encode()
        (self.batch / self.name).write_bytes(raw)
        self.manifest["functions"][0]["files"][0].update(
            bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest())
        self.publish()
        output = self.root / "foreign"
        self.assertNotEqual(self.dump(output).returncode, 0)
        self.assertFalse(output.exists())

    def test_existing_destination_is_preserved(self):
        output = self.root / "existing"
        output.mkdir()
        marker = output / "analyst-note"
        marker.write_bytes(b"keep")
        self.assertNotEqual(self.dump(output).returncode, 0)
        self.assertEqual(marker.read_bytes(), b"keep")
        self.assertEqual(list(output.iterdir()), [marker])

    def test_rehashed_header_language_contract_is_validated_before_publication(self):
        for index, languages in enumerate([[], ["swift"], ["c", "c"], "c", [42]]):
            with self.subTest(languages=languages):
                artifact = fixture()
                scope = MODULE.AnalysisScope(artifact["scope"]["pass_version"], artifact["scope"]["settings_digest"])
                node = {"kind": "metadata", "epistemic": "derived", "producer": "fixture-v1",
                        "confidence": 1, "ranges": [], "payload": {
                            "artifact_kind": "recovered-header", "source": "#pragma once\n",
                            "status": "declaration-only-uncompiled", "languages": languages}}
                node["id"] = MODULE.content_id("evidence", artifact["source_sha256"], artifact["function"]["id"], node, scope)
                artifact["nodes"].append(node)
                raw = json.dumps(artifact).encode()
                MODULE.ReconstructionArtifact(artifact)
                (self.batch / self.name).write_bytes(raw)
                self.manifest["functions"][0]["files"][0].update(bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest())
                self.publish()
                output = self.root / ("invalid-language-" + str(index))
                result = self.dump(output, "--format", "header")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(b"invalid header language contract", result.stderr)
                self.assertFalse(output.exists())

    def test_qualifier_schedules_declared_languages_and_labels_legacy_default(self):
        # This portable test checks scheduling and provenance, not C syntax;
        # the macOS native header gate runs the actual compiler.
        spec = importlib.util.spec_from_file_location("header_qualification_test", ROOT / "scripts/qualify-reconstruction-batch.py")
        qualifier = importlib.util.module_from_spec(spec)
        with patch.object(sys, "path", [str(ROOT / "scripts"), *sys.path]):
            spec.loader.exec_module(qualifier)
        for index, languages in enumerate([None, ["objective-c", "objective-c++"],
                                            ["c", "c++", "objective-c", "objective-c++"]]):
            with self.subTest(languages=languages):
                directory = self.root / ("qualify-" + str(index))
                directory.mkdir()
                artifact = fixture()
                scope = MODULE.AnalysisScope(artifact["scope"]["pass_version"], artifact["scope"]["settings_digest"])
                text = b"#pragma once\n"
                payload = {"artifact_kind": "recovered-header", "source": text.decode(), "status": "declaration-only-uncompiled"}
                if languages is not None:
                    payload["languages"] = languages
                node = {"kind": "metadata", "epistemic": "derived", "producer": "fixture-v1",
                        "confidence": 1, "ranges": [], "payload": payload}
                node["id"] = MODULE.content_id("evidence", artifact["source_sha256"], artifact["function"]["id"], node, scope)
                artifact["nodes"].append(node)
                raw = json.dumps(artifact).encode()
                header_name = "function-0000000000001000.h"
                (directory / self.name).write_bytes(raw)
                (directory / header_name).write_bytes(text)
                manifest = json.loads(json.dumps(self.manifest))
                manifest["functions"][0]["files"] = [
                    {"path": self.name, "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()},
                    {"path": header_name, "bytes": len(text), "sha256": hashlib.sha256(text).hexdigest()}]
                (directory / "manifest.json").write_text(json.dumps(manifest))
                commands = []
                def execute(command):
                    commands.append(command)
                    return "fixture-compiler"
                with patch.object(qualifier, "execute", execute):
                    self.assertTrue(qualifier.run(directory, headers_only=True))
                report = json.loads((directory / "header-qualification/report.json").read_bytes())
                expected = languages if languages is not None else ["objective-c", "objective-c++"]
                self.assertEqual([check["language"] for check in report["header_checks"]], expected)
                self.assertEqual([command[command.index("-x") + 1] for command in commands if "-x" in command],
                                 [language + "-header" for language in expected])
                for check in report["header_checks"]:
                    self.assertEqual(check["sha256"], hashlib.sha256(text).hexdigest())
                    self.assertEqual(check["language_origin"], "artifact" if languages is not None else "legacy-qualification-default")
                self.assertFalse(report["compiled"])

    def test_batch_outcomes_separate_review_only_from_execution_failure(self):
        spec = importlib.util.spec_from_file_location("batch_outcome_test", ROOT / "scripts/reconstruct-binary.py")
        batch = importlib.util.module_from_spec(spec)
        with patch.object(sys, "path", [str(ROOT / "scripts"), *sys.path]):
            spec.loader.exec_module(batch)
        binary = self.root / "captured-input"
        binary.write_bytes(b"test-owned source identity")
        worker = self.root / "worker-identity"
        worker.write_bytes(b"test-owned worker identity")
        digest = hashlib.sha256(binary.read_bytes()).hexdigest()
        artifact = fixture(source=digest)
        def request(worker, binary, digest, size, command, arguments, prototypes):
            if command == "--analyze":
                return {"source_sha256": digest, "functions": "0x1000 0x1008 8 bytes identity"}
            return artifact
        for budget, failed in [(1024 * 1024, False), (1, True)]:
            args = types.SimpleNamespace(binary=binary, worker=worker, output=self.root / ("outcome-" + str(budget)),
                                         prototypes=None, resume=False, start=None, end=None, include_callees=False,
                                         max_functions=1, max_bytes=budget)
            with patch.object(batch, "request", request), contextlib.redirect_stdout(io.StringIO()):
                manifest = batch.run(args)
            self.assertEqual(manifest["failed_functions"], int(failed))
            self.assertEqual(manifest["review_only_functions"], int(not failed))
            self.assertEqual(manifest["recovered_functions"], 0)
            self.assertEqual(manifest["status"], "completed-with-failures" if failed else "completed")
            self.assertEqual(json.loads((args.output / "manifest.json").read_bytes()), json.loads(json.dumps(manifest)))
            if failed:
                args.resume = True
                with patch.object(batch, "request", side_effect=request) as requests, contextlib.redirect_stdout(io.StringIO()):
                    resumed = batch.run(args)
                self.assertEqual(requests.call_count, 1, "retained failures must not be silently retried")
                self.assertEqual(resumed["functions"], manifest["functions"])
                self.assertEqual(resumed["failed_functions"], 1)


if __name__ == "__main__":
    unittest.main()
