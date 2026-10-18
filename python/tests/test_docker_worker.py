"""Docker launch contract tests; no daemon or imported code execution needed."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("docker_worker", ROOT / "scripts/docker-worker.py")
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class DockerWorkerTests(unittest.TestCase):
    def args(self, path=None, **overrides):
        values = dict(operation="analyze", path=path, image=worker.IMAGE,
                      format="auto", architecture="auto", load_address=0,
                      file_offset=0, start_word=0, start=None, end=None, inputs=[0]*8)
        values.update(overrides)
        return argparse.Namespace(**values)

    def test_health_has_no_host_mount_or_network(self):
        argv, digest = worker.command(self.args(operation="health"), "job")
        self.assertIsNone(digest)
        self.assertNotIn("--mount", argv)
        for flag in ["--network=none", "--read-only", "--cap-drop=ALL",
                     "--security-opt=no-new-privileges=true", "--memory=1g",
                     "--memory-swap=1g", "--cpus=2", "--pids-limit=64", "--pull=never"]:
            self.assertIn(flag, argv)
        self.assertFalse(any(flag.startswith("--user=0:") for flag in argv))
        self.assertNotIn("--privileged", argv)
        self.assertEqual(argv[-2:], [worker.IMAGE, "--self-test"])

    def test_single_readonly_input_and_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input with spaces;$(literal)"
            path.write_bytes(b"abc")
            argv, digest = worker.command(self.args(path), "job")
            self.assertEqual(digest, hashlib.sha256(b"abc").hexdigest())
            self.assertEqual(argv.count("--mount"), 1)
            self.assertIn(f"type=bind,source={path.resolve()},target=/input.bin,readonly", argv)
            offset = argv.index("--analyze-image")
            self.assertEqual(argv[offset+1:offset+4], ["/input.bin", digest, "3"])
            self.assertNotIn("/var/run/docker.sock", " ".join(argv))

    def test_nonregular_symlink_oversize_and_mount_injection_reject(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "input"
            path.write_bytes(b"abc")
            link = root / "link"
            link.symlink_to(path)
            fifo = root / "fifo"
            os.mkfifo(fifo)
            comma = root / "input,readonly=false"
            comma.write_bytes(b"abc")
            for invalid in [root, link, fifo, comma]:
                with self.subTest(path=invalid), self.assertRaises(ValueError):
                    worker.source_identity(invalid)
            with patch.object(worker, "MAX_SOURCE", 2), self.assertRaises(ValueError):
                worker.source_identity(path)

    def test_invalid_image_does_not_reach_docker(self):
        for image in ["--privileged", "image;sh", "image\nother", "/input", ""]:
            with self.subTest(image=image), self.assertRaises(ValueError):
                worker.command(self.args(image=image, operation="health"), "job")

    def test_uint64(self):
        self.assertEqual(worker.unsigned("0xffffffffffffffff"), 2**64-1)
        for text in ["-1", "+1", "18446744073709551616", "0x10000000000000000", "1.0", "1_0"]:
            with self.subTest(text=text), self.assertRaises(argparse.ArgumentTypeError):
                worker.unsigned(text)

    def test_reconstruction_and_interpreter_arguments(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input"
            path.write_bytes(b"abc")
            args = self.args(path, operation="interpret", start=0x1000, end=0x1008)
            argv, _ = worker.command(args, "job")
            self.assertIn("--interpret", argv)
            self.assertEqual(argv[-4:-1], ["0x1000", "0x1008", ",".join(["0x0"]*8)])
            for changed in [dict(start=None), dict(end=0), dict(format="elf"), dict(inputs=[0])]:
                invalid = vars(args).copy()
                invalid.update(changed)
                with self.subTest(changed=changed), self.assertRaises(ValueError):
                    worker.command(argparse.Namespace(**invalid), "job")

    def test_frame_integrity_and_source_binding(self):
        payload = json.dumps({"source_sha256": "a"*64, "kind": "analysis_result"}).encode()
        frame = struct.pack(">I", len(payload)) + payload
        self.assertEqual(worker.decode_frame(frame, "a"*64)["kind"], "analysis_result")
        for invalid in [b"", frame[:-1], frame+b"x", b"\xff"*4]:
            with self.subTest(raw=invalid), self.assertRaises(ValueError):
                worker.decode_frame(invalid)
        with self.assertRaises(ValueError):
            worker.decode_frame(frame, "b"*64)

    def test_process_output_budget_and_cleanup(self):
        with patch.object(worker, "MAX_FRAME", 64), patch.object(worker.subprocess, "run") as cleanup:
            cleanup.return_value.returncode = 0
            with self.assertRaisesRegex(ValueError, "output budget"):
                worker.execute([sys.executable, "-c", "import os;os.write(1,b'x'*1024)"], "test-owned-job", 5)
            self.assertEqual(cleanup.call_args.args[0], ["docker", "rm", "--force", "test-owned-job"])

    def test_process_timeout_and_cleanup(self):
        with patch.object(worker.subprocess, "run") as cleanup:
            cleanup.return_value.returncode = 0
            with self.assertRaises(TimeoutError):
                worker.execute([sys.executable, "-c", "import time;time.sleep(5)"], "test-owned-job", 0.05)
            cleanup.assert_called_once()

    def test_successful_process(self):
        self.assertEqual(worker.execute([sys.executable, "-c", "print('ok')"], "unused", 5), b"ok\n")


if __name__ == "__main__":
    unittest.main()
