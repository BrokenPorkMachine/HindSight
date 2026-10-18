#!/usr/bin/env python3
"""Qualify all UMOV/SMOV lane forms against Apple Clang and native AArch64.

Generated test inputs are CC0. No selected/user binary is loaded or executed.
The native harness tests bit results, not privileged trap/streaming-mode models.
"""
from __future__ import annotations

import hashlib
import json
import pathlib
import platform
import re
import struct
import subprocess
import sys
import tempfile
import uuid


def forms():
    for wide, signed, sizes in [(False, False, [8, 16, 32]), (True, False, [64]),
                                (False, True, [8, 16]), (True, True, [8, 16, 32])]:
        for bits in sizes:
            for lane in range(128 // bits):
                yield wide, signed, bits, lane


def analyze(worker, binary):
    raw = binary.read_bytes()
    result = subprocess.run([str(worker), "--analyze", str(binary), hashlib.sha256(raw).hexdigest(),
                             str(len(raw)), str(uuid.uuid4())], check=True, capture_output=True, timeout=120)
    assert len(result.stdout) >= 4, "truncated worker frame"
    length = struct.unpack_from(">I", result.stdout)[0]
    assert length <= 32 * 1024 * 1024 and len(result.stdout) == length + 4
    report = json.loads(result.stdout[4:])
    assert report["kind"] == "analysis_result", report
    return report


def run(worker):
    if sys.platform != "darwin":
        print("Apple assembler/native vector extraction qualification requires macOS; skipped")
        return
    native = platform.machine() == "arm64"
    cases = list(forms())
    assert len(cases) == 82
    with tempfile.TemporaryDirectory(prefix="hindsight-vector-extract-") as directory:
        root = pathlib.Path(directory)
        for batch in range(0, len(cases), 8):
            assembly = [".text", ".p2align 2"]
            declarations = []
            entries = []
            for index, (wide, signed, bits, lane) in enumerate(cases[batch:batch + 8]):
                register = "x0" if wide else "w0"
                instruction = f"{'smov' if signed else 'umov'} {register}, v0.{ {8:'b',16:'h',32:'s',64:'d'}[bits]}[{lane}]"
                for constant in [False, True]:
                    name = f"hs_lane_{index}{'_constant' if constant else ''}"
                    assembly.extend([f".globl _{name}", f"_{name}:",
                                     "movi v0.2d, #0xff" if constant else "ldr q0, [x0]",
                                     instruction, "ret"])
                    declarations.append(f"extern uint64_t {name}(const unsigned char *);")
                    entries.append(f"{{{name}, {bits}, {lane}, {int(signed)}, {int(wide)}, {int(constant)}}}")
            source = """
#include <stdint.h>
#include <stdio.h>
struct test { uint64_t (*call)(const unsigned char *); unsigned bits, lane, sign, wide, constant; };
""" + "\n".join(declarations) + "\nstatic struct test tests[] = {" + ",\n".join(entries) + """};
int main(void) {
    uint64_t seed = UINT64_C(0x36e28a519bf074cd);
    for (unsigned sample = 0; sample < 260; ++sample) {
        _Alignas(16) unsigned char input[16];
        for (unsigned byte = 0; byte < 16; ++byte) {
            seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
            input[byte] = sample == 0 ? 0 : sample == 1 ? 255 : sample == 2 ? 128 : sample == 3 ? byte : seed;
        }
        for (unsigned item = 0; item < sizeof(tests)/sizeof(tests[0]); ++item) {
            struct test t = tests[item];
            uint64_t expected = 0;
            for (unsigned byte = 0; byte < t.bits/8; ++byte) {
                unsigned offset = t.lane * (t.bits/8) + byte;
                unsigned value = t.constant ? (offset % 8 == 0 ? 255 : 0) : input[offset];
                expected |= (uint64_t)value << (byte*8);
            }
            if (t.sign && (expected & (UINT64_C(1) << (t.bits - 1)))) expected |= UINT64_MAX << t.bits;
            if (!t.wide) expected &= UINT32_MAX;
            uint64_t actual = t.call(input);
            if (actual != expected) {
                fprintf(stderr, "lane extraction mismatch item=%u sample=%u actual=%llx expected=%llx\\n",
                        item, sample, (unsigned long long)actual, (unsigned long long)expected);
                return 1;
            }
        }
    }
    return 0;
}
"""
            asm_path, c_path, binary = root / "lanes.s", root / "oracle.c", root / "lanes"
            asm_path.write_text("\n".join(assembly) + "\n")
            c_path.write_text(source)
            subprocess.run(["xcrun", "clang", "-target", "arm64-apple-macos14.0", "-O2",
                            "-Werror", str(asm_path), str(c_path), "-o", str(binary)],
                           check=True, capture_output=True, timeout=60)
            if native:
                subprocess.run([str(binary)], check=True, capture_output=True, timeout=30)
            report = analyze(worker, binary)
            # Check the actual assembler-produced words through the worker,
            # including exact constant folding, not just presence of functions.
            for wide, signed, bits, lane in cases[batch:batch + 8]:
                register = "x0" if wide else "w0"
                expression = f"{register} = {'sext' if signed else 'zext'}(vector_lane(v0, {lane}, {bits}), {bits});"
                assert expression in report["semantic_preview"], (expression, report["semantic_preview"])
                pattern = (0xff << 64) | 0xff
                expected = (pattern >> (lane * bits)) & ((1 << bits) - 1)
                if signed and expected & (1 << (bits - 1)):
                    expected -= 1 << bits
                expected &= (1 << (64 if wide else 32)) - 1
                assert re.search(re.escape(expression) + rf" /\* constant=0x{expected:x}\b[^\n]*proven_by=", report["semantic_preview"]), expression
    print(json.dumps({"forms": len(cases), "native_execution": native,
                      "native_comparisons": len(cases) * 2 * 260 if native else 0,
                      "assembler_and_worker_constant_checks": len(cases),
                      "scope": "EL0 non-streaming integer lane results; no trap/FPCR/SME qualification"}))


if __name__ == "__main__":
    run(pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "build/rust/debug/reworker").resolve())
