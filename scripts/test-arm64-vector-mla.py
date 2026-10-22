#!/usr/bin/env python3
"""Qualify Advanced SIMD MLA/MLS lane arithmetic against native AArch64.

Generated test functions are CC0 and contain no user-controlled code.
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


FORMS = [(bits, lanes, q) for bits, lanes, q in
         [(8, 8, False), (8, 16, True), (16, 4, False), (16, 8, True),
          (32, 2, False), (32, 4, True)]]


def analyze(worker: pathlib.Path, binary: pathlib.Path) -> dict:
    raw = binary.read_bytes()
    result = subprocess.run(
        [str(worker), "--analyze", str(binary), hashlib.sha256(raw).hexdigest(),
         str(len(raw)), str(uuid.uuid4())], check=True, capture_output=True, timeout=120)
    assert len(result.stdout) >= 4, "truncated worker frame"
    size = struct.unpack_from(">I", result.stdout)[0]
    assert size <= 32 * 1024 * 1024 and len(result.stdout) == size + 4
    report = json.loads(result.stdout[4:])
    assert report.get("kind") == "analysis_result", report
    return report


def run(worker: pathlib.Path) -> None:
    if sys.platform != "darwin":
        print("Apple assembler/native MLA qualification requires macOS; skipped")
        return
    native = platform.machine() == "arm64"
    with tempfile.TemporaryDirectory(prefix="hindsight-vector-mla-") as temp:
        root = pathlib.Path(temp)
        assembly = [".text", ".p2align 2"]
        declarations, entries, expected_lines = [], [], []
        for bits, lanes, quad in FORMS:
            suffix = {8: "b", 16: "h", 32: "s"}[bits]
            arrangement = f"{lanes}{suffix}"
            load, store = ("ldr q", "str q") if quad else ("ldr d", "str d")
            for subtract in (False, True):
                name = f"hs_{'mls' if subtract else 'mla'}_{bits}_{lanes}"
                mnemonic = "mls" if subtract else "mla"
                assembly.extend([f".globl _{name}", f"_{name}:",
                                 f"{load}0, [x0]", f"{load}1, [x1]", f"{load}2, [x2]",
                                 f"{mnemonic} v0.{arrangement}, v1.{arrangement}, v2.{arrangement}",
                                 f"{store}0, [x3]", "ret"])
                declarations.append(f"extern void {name}(const unsigned char *, const unsigned char *, const unsigned char *, unsigned char *);")
                entries.append(f"{{{name}, {bits}, {lanes}, {int(subtract)}}}")
                operator = "-" if subtract else "+"
                expected_lines.append(f"v0.{arrangement} = v0.{arrangement} {operator} (v1.{arrangement} * v2.{arrangement});")

        source = """
#include <stdint.h>
#include <stdio.h>
struct test { void (*call)(const unsigned char *, const unsigned char *, const unsigned char *, unsigned char *); unsigned bits, lanes, subtract; };
""" + "\n".join(declarations) + "\nstatic struct test tests[] = {" + ",\n".join(entries) + """};
int main(void) {
    uint64_t seed = UINT64_C(0x9e3779b97f4a7c15);
    for (unsigned sample = 0; sample < 260; ++sample) {
        _Alignas(16) unsigned char a[16], b[16], acc[16], out[16];
        for (unsigned i = 0; i < 16; ++i) {
            seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
            a[i] = sample == 0 ? 255 : sample == 1 ? 0 : (unsigned char)seed;
            b[i] = sample == 0 ? 255 : sample == 2 ? 0 : (unsigned char)(seed >> 8);
            acc[i] = sample == 0 ? 255 : (unsigned char)(seed >> 16);
        }
        for (unsigned n = 0; n < sizeof(tests)/sizeof(tests[0]); ++n) {
            struct test t = tests[n];
            t.call(acc, a, b, out);
            unsigned bytes = t.bits / 8, lane_bytes = bytes;
            uint64_t mask = (UINT64_C(1) << t.bits) - 1;
            for (unsigned lane = 0; lane < t.lanes; ++lane) {
                uint64_t av = 0, bv = 0, cv = 0;
                for (unsigned j = 0; j < lane_bytes; ++j) {
                    unsigned offset = lane * lane_bytes + j;
                    av |= (uint64_t)a[offset] << (8*j);
                    bv |= (uint64_t)b[offset] << (8*j);
                    cv |= (uint64_t)acc[offset] << (8*j);
                }
                uint64_t value = (t.subtract ? cv - av*bv : cv + av*bv) & mask;
                for (unsigned j = 0; j < lane_bytes; ++j) {
                    unsigned offset = lane * lane_bytes + j;
                    unsigned char expected = (unsigned char)(value >> (8*j));
                    if (out[offset] != expected) {
                        fprintf(stderr, "MLA/MLS mismatch form=%u sample=%u lane=%u byte=%u got=%02x expected=%02x\\n",
                                n, sample, lane, j, out[offset], expected);
                        return 1;
                    }
                }
            }
            (void)bytes;
        }
    }
    return 0;
}
"""
        asm_path, c_path, binary = root / "mla.s", root / "oracle.c", root / "mla"
        asm_path.write_text("\n".join(assembly) + "\n")
        c_path.write_text(source)
        subprocess.run(["xcrun", "clang", "-target", "arm64-apple-macos14.0", "-O2",
                        "-Werror", str(asm_path), str(c_path), "-o", str(binary)],
                       check=True, capture_output=True, timeout=60)
        if native:
            subprocess.run([str(binary)], check=True, capture_output=True, timeout=30)
        report = analyze(worker, binary)
        for line in set(expected_lines):
            assert line in report["semantic_preview"], (line, report["semantic_preview"])
        for size in (8, 16):
            span = 8 if size == 8 else 16
            effect = re.compile(
                rf"ssa-vector 0x[0-9a-f]+ writes=\[v0\[0\.\.16\)\] "
                rf"reads=\[v0\[0\.\.{span}\),v1\[0\.\.{span}\),v2\[0\.\.{span}\)\]"
            )
            assert effect.search(report["semantic_preview"]), (effect.pattern, report["semantic_preview"])
    print(json.dumps({"forms": len(FORMS) * 2, "native_execution": native,
                      "native_comparisons": len(FORMS) * 2 * 260 if native else 0,
                      "arrangements": ["8b", "16b", "4h", "8h", "2s", "4s"],
                      "reserved_64_bit_lane_forms": "not emitted"}))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {pathlib.Path(sys.argv[0]).name} /absolute/path/to/reworker")
    run(pathlib.Path(sys.argv[1]).expanduser().resolve())
