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


def analyze(worker, binary):
    data = binary.read_bytes()
    reply = subprocess.run([str(worker), "--analyze", str(binary), hashlib.sha256(data).hexdigest(),
                            str(len(data)), str(uuid.uuid4())], capture_output=True, check=True, timeout=120)
    length = struct.unpack(">I", reply.stdout[:4])[0]
    assert length <= 32 * 1024 * 1024 and len(reply.stdout) == length + 4
    report = json.loads(reply.stdout[4:])
    assert report["kind"] == "analysis_result", report
    return report


def file_offset(data, address):
    # The fixture compiler emits a thin, little-endian 64-bit Mach-O.
    assert data[:4] == bytes.fromhex("cffaedfe")
    count = struct.unpack_from("<I", data, 16)[0]
    at = 32
    for _ in range(count):
        command, size = struct.unpack_from("<II", data, at)
        assert size >= 8 and at + size <= len(data)
        if command == 0x19:
            sections = struct.unpack_from("<I", data, at + 64)[0]
            for index in range(sections):
                section = at + 72 + index * 80
                va, extent, offset = struct.unpack_from("<QQI", data, section + 32)
                if va <= address < va + extent:
                    return offset + address - va
        at += size
    raise AssertionError("protocol VA has no fixture section mapping")


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
            report = analyze(worker, binary)
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
            assert "protocol=ABIParent membership=required-instance selector=parentValue" in evidence, evidence[-12000:]
            assert "protocol=ABIRecovered adopts=ABIParent inheritance=validated membership=required-instance selector=castValue:" in evidence, evidence[-12000:]
            assert "protocol=ABIRecovered adopts=ABIParent inheritance=validated" in evidence, evidence[-12000:]
            assert "@protocol ABIRecovered <ABIParent>" in evidence
            assert "class=ABISample conforms=ABIRecovered source=validated-class-ro-protocol-list" in evidence
            assert "@interface ABISample <ABIRecovered>" in evidence
            assert "implementation-coverage=unproven dispatch-targets=unresolved" in evidence
            # Runtime `c` encodes char/signed-char without a source typedef name.
            assert "- (int)castValue:(char)arg0;" in evidence
            assert "protocol=ABIRecovered adopts=ABIParent inheritance=validated membership=optional-class selector=makeOptional" in evidence
            assert "@optional\n+ (id)makeOptional;" in evidence
            assert "protocol=ABIRecovered adopts=ABIParent inheritance=validated membership=optional-instance selector=optionalRatio:" in evidence
            assert "- (double)optionalRatio:(float)arg0;" in evidence
            assert "implementation=unproven" in evidence
            assert "source-cast=signed<int>; low-bits=32; raw-register=0xffffffff" in pseudocode, pseudocode[-8000:]
            assert "(int)-1" in pseudocode and "(size_t)0x8" in pseudocode
            for declaration, returned in [("- (void *)pointer", "return x0;"), ("- (double)scalar", "return d0;"), ("- (float)single", "return s0;")]:
                body = pseudocode.split(declaration + " {", 1)[1].split("\n}", 1)[0]
                assert returned in body, body
            print(f"Mach-O prototype/ABI metadata and pseudocode checks passed ({optimization})")
            if optimization == "-O2":
                match = re.search(r"protocol (0x[0-9a-f]+) name=ABIRecovered(?: adopts=[^ ]+)? confidence=validated", report["objc_protocols"])
                assert match, report["objc_protocols"]
                data = binary.read_bytes()
                offset = file_offset(data, int(match[1], 16))
                for bad_size in [0, 71, 0xffffffff]:
                    malformed = bytearray(data)
                    struct.pack_into("<I", malformed, offset + 64, bad_size)
                    binary.write_bytes(malformed)
                    rejected = analyze(worker, binary)
                    assert "protocol=ABIRecovered " not in rejected["call_values"]
                    assert "protocol=ABIParent " in rejected["call_values"]
                print("Malformed protocol extents do not produce prototype claims")


if __name__ == "__main__":
    run(pathlib.Path(sys.argv[1]).resolve())
