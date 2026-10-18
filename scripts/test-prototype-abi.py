#!/usr/bin/env python3
"""Compile real Mach-O metadata and check recovered Apple arm64 ABI evidence."""
import hashlib
import json
import pathlib
import re
import struct
import subprocess
import sys
import tempfile
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]


def run(worker):
    if sys.platform != "darwin":
        print("Apple prototype ABI fixture requires macOS; skipped")
        return
    with tempfile.TemporaryDirectory(prefix="hindsight-prototype-abi-") as directory:
        for optimization in ["-O0", "-O2"]:
            binary = pathlib.Path(directory) / "abi-sample"
            subprocess.run(["xcrun", "clang", "-target", "arm64-apple-macos14.0", optimization,
                            "-fobjc-arc", "-Wl,-no_fixup_chains", "-framework", "Foundation", "-framework", "CoreGraphics",
                            str(ROOT / "Tests/Fixtures/Decompilation/abi_sample.m"), "-o", str(binary)],
                           capture_output=True, check=True, timeout=60)
            data = binary.read_bytes()
            reply = subprocess.run([str(worker), "--analyze", str(binary), hashlib.sha256(data).hexdigest(),
                                    str(len(data)), str(uuid.uuid4())], capture_output=True, check=True, timeout=120)
            length = struct.unpack(">I", reply.stdout[:4])[0]
            assert length <= 32 * 1024 * 1024 and len(reply.stdout) == length + 4
            report = json.loads(reply.stdout[4:])
            assert report["kind"] == "analysis_result", report
            evidence = report["call_values"]
            pseudocode = report["semantic_preview"]

            def abi(method):
                rows = evidence.splitlines()
                for index, row in enumerate(rows[:-1]):
                    if "-[ABISample " + method + "] language=" in row:
                        return rows[index + 1]
                raise AssertionError("missing method " + method + "\n" + evidence[:6000])

            assert "result=return x0;" in abi("pointer"), abi("pointer")
            assert "result=return d0;" in abi("scalar"), abi("scalar")
            assert "result=return s0;" in abi("single"), abi("single")
            assert "arg0=aggregate<NSRange>(x2, x3)" in abi("range:")
            assert "result=return aggregate<NSRange>(x0, x1);" in abi("range:")
            assert "arg0=aggregate<CGRect>(d0, d1, d2, d3)" in abi("rectangle:")
            assert "result=return aggregate<CGRect>(d0, d1, d2, d3);" in abi("rectangle:")
            many = abi("manyFP:b:c:d:e:f:g:h:i:tail:")
            assert "arg7=d7" in many and "arg8=stack argument (slot 0, sp+0x0, 8 bytes" in many, many
            assert "arg9=x2" in many, many
            packed = abi("packed:b:c:d:e:f:byte:half:word:pointer:")
            for index, offset, size in [(6, 0, 1), (7, 2, 2), (8, 4, 4), (9, 8, 8)]:
                assert re.search(rf"arg{index}=stack argument \(slot \d+, sp\+0x{offset:x}, {size} bytes", packed), packed
            for declaration, returned in [("- (void *)pointer", "return x0;"), ("- (double)scalar", "return d0;"), ("- (float)single", "return s0;")]:
                body = pseudocode.split(declaration + " {", 1)[1].split("\n}", 1)[0]
                assert returned in body, body
            print(f"Mach-O prototype/ABI metadata and pseudocode checks passed ({optimization})")


if __name__ == "__main__":
    run(pathlib.Path(sys.argv[1]).resolve())
