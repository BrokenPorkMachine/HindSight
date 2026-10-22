#!/usr/bin/env python3
"""Qualify supported vector FP lane operations against native Apple Silicon.

Only finite, normal inputs are generated. NaN propagation, signed-zero
selection, FPCR modes, and exception flags are deliberately outside this gate.
"""
from __future__ import annotations

import hashlib
import json
import pathlib
import platform
import struct
import subprocess
import sys
import tempfile
import uuid


OPS = {
    "fadd": "+", "fsub": "-", "fmul": "*", "fdiv": "/",
    "fmax": "fmax", "fmin": "fmin", "fmaxnm": "fmax", "fminnm": "fmin",
}
FORMS = [(bits, lanes, quad) for bits, lanes, quad in
         [(16, 4, False), (16, 8, True), (32, 2, False),
          (32, 4, True), (64, 2, True)]]


def analyze(worker: pathlib.Path, binary: pathlib.Path) -> dict:
    raw = binary.read_bytes()
    result = subprocess.run(
        [str(worker), "--analyze", str(binary), hashlib.sha256(raw).hexdigest(),
         str(len(raw)), str(uuid.uuid4())], check=True, capture_output=True, timeout=120)
    if len(result.stdout) < 4:
        raise RuntimeError("truncated worker frame")
    size = struct.unpack_from(">I", result.stdout)[0]
    if size > 32 * 1024 * 1024 or len(result.stdout) != size + 4:
        raise RuntimeError("invalid worker frame size")
    report = json.loads(result.stdout[4:])
    assert report.get("kind") == "analysis_result", report
    return report


def run(worker: pathlib.Path) -> None:
    if sys.platform != "darwin":
        print("Apple assembler/native vector FP qualification requires macOS; skipped")
        return
    native = platform.machine() == "arm64"
    with tempfile.TemporaryDirectory(prefix="hindsight-vector-fp-") as temp:
        root = pathlib.Path(temp)
        assembly = [".text", ".p2align 2"]
        declarations, entries, expected_lines = [], [], []
        for bits, lanes, quad in FORMS:
            suffix = {16: "h", 32: "s", 64: "d"}[bits]
            arrangement = f"{lanes}{suffix}"
            load, store = ("ldr q", "str q") if quad else ("ldr d", "str d")
            for op in OPS:
                name = f"hs_{op}_{bits}_{lanes}"
                assembly.extend([f".globl _{name}", f"_{name}:",
                                 f"{load}0, [x0]", f"{load}1, [x1]",
                                 f"{op} v0.{arrangement}, v0.{arrangement}, v1.{arrangement}",
                                 f"{store}0, [x2]", "ret"])
                declarations.append(f"extern void {name}(const void *, const void *, void *);")
                entries.append(f"{{{name}, {bits}, {lanes}, {list(OPS).index(op)}}}")
                expected_lines.append(
                    f"v0.{arrangement} = {op}(v0.{arrangement}, v1.{arrangement});")

        source = r"""
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <math.h>
struct test { void (*call)(const void *, const void *, void *); unsigned bits, lanes, op; };
""" + "\n".join(declarations) + "\nstatic struct test tests[] = {" + ",\n".join(entries) + r"""};
static uint64_t next(uint64_t *s) {
    *s ^= *s << 13; *s ^= *s >> 7; *s ^= *s << 17; return *s;
}
int main(void) {
    uint64_t seed = UINT64_C(0x91e10da5c79e7b1d);
    for (unsigned sample = 0; sample < 240; ++sample) {
        _Alignas(16) unsigned char a[16], b[16], out[16], expected[16];
        for (unsigned n = 0; n < sizeof(tests)/sizeof(tests[0]); ++n) {
            struct test t = tests[n]; memset(a, 0, 16); memset(b, 0, 16);
            memset(out, 0, 16); memset(expected, 0, 16);
            for (unsigned lane = 0; lane < t.lanes; ++lane) {
                int32_t ai = (int32_t)(next(&seed) % 20001) - 10000;
                int32_t bi = (int32_t)(next(&seed) % 20001) - 10000;
                if (bi == 0) bi = 1;
                if (t.bits == 16) {
                    __fp16 av = (__fp16)((float)ai / 64.0f), bv = (__fp16)((float)bi / 64.0f);
                    memcpy(a + lane*2, &av, 2); memcpy(b + lane*2, &bv, 2);
                    __fp16 value;
                    switch (t.op) {
                    case 0: value = (__fp16)(av + bv); break; case 1: value = (__fp16)(av - bv); break;
                    case 2: value = (__fp16)(av * bv); break; case 3: value = (__fp16)(av / bv); break;
                    case 4: case 6: value = (__fp16)fmaxf((float)av, (float)bv); break;
                    default: value = (__fp16)fminf((float)av, (float)bv); break;
                    }
                    memcpy(expected + lane*2, &value, 2);
                } else if (t.bits == 32) {
                    float av = (float)ai / 128.0f, bv = (float)bi / 128.0f;
                    memcpy(a + lane*4, &av, 4); memcpy(b + lane*4, &bv, 4);
                    float value;
                    switch (t.op) {
                    case 0: value = av + bv; break; case 1: value = av - bv; break;
                    case 2: value = av * bv; break; case 3: value = av / bv; break;
                    case 4: case 6: value = fmaxf(av, bv); break;
                    default: value = fminf(av, bv); break;
                    }
                    memcpy(expected + lane*4, &value, 4);
                } else {
                    double av = (double)ai / 128.0, bv = (double)bi / 128.0;
                    memcpy(a + lane*8, &av, 8); memcpy(b + lane*8, &bv, 8);
                    double value;
                    switch (t.op) {
                    case 0: value = av + bv; break; case 1: value = av - bv; break;
                    case 2: value = av * bv; break; case 3: value = av / bv; break;
                    case 4: case 6: value = fmax(av, bv); break;
                    default: value = fmin(av, bv); break;
                    }
                    memcpy(expected + lane*8, &value, 8);
                }
            }
            t.call(a, b, out);
            if (memcmp(out, expected, t.lanes * (t.bits/8)) != 0) {
                fprintf(stderr, "vector FP mismatch form=%u sample=%u bits=%u op=%u\n",
                        n, sample, t.bits, t.op); return 1;
            }
        }
    }
    return 0;
}
"""
        asm_path, c_path, binary = root / "vector-fp.s", root / "oracle.c", root / "vector-fp"
        asm_path.write_text("\n".join(assembly) + "\n")
        c_path.write_text(source)
        subprocess.run(["xcrun", "clang", "-target", "arm64-apple-macos14.0", "-O2",
                        "-fno-fast-math", "-Werror", str(asm_path), str(c_path), "-lm", "-o", str(binary)],
                       check=True, capture_output=True, timeout=60)
        if native:
            subprocess.run([str(binary)], check=True, capture_output=True, timeout=30)
        report = analyze(worker, binary)
        for line in set(expected_lines):
            assert line in report["semantic_preview"], (line, report["semantic_preview"])
    print(json.dumps({"forms": len(FORMS) * len(OPS), "native_execution": native,
                      "native_comparisons": len(FORMS) * len(OPS) * 240 if native else 0,
                      "element_widths": [16, 32, 64],
                      "excluded": ["NaN propagation", "signed-zero selection", "FPCR", "exceptions"]}))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {pathlib.Path(sys.argv[0]).name} /absolute/path/to/reworker")
    run(pathlib.Path(sys.argv[1]).expanduser().resolve())
