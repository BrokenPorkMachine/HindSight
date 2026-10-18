#!/usr/bin/env python3
"""Qualify shared-effect parameter names on Apple-assembled native functions."""
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import struct
import subprocess
import sys
import tempfile
import uuid

from darwin_sdk_catalog import collect_declarations, worker_prototypes

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "Tests/Fixtures/Decompilation"
FUNCTIONS = ["hs_copy_chain", "hs_frame_spill", "hs_pair_spill", "hs_fp_frame", "hs_preindexed", "hs_load_overlap"]


def run(worker):
    if sys.platform != "darwin":
        print("Apple AArch64 parameter name qualification requires macOS; skipped")
        return
    declarations = collect_declarations(FUNCTIONS, FIXTURES / "nameflow_sample.h")
    with tempfile.TemporaryDirectory(prefix="hindsight-nameflow-") as directory:
        root = Path(directory)
        binary, catalog = root / "nameflow", root / "prototypes.tsv"
        subprocess.run(["xcrun", "clang", "-target", "arm64-apple-macos14.0", "-O2", "-Werror",
                        str(FIXTURES / "nameflow_sample.s"), str(FIXTURES / "nameflow_oracle.c"), "-o", str(binary)],
                       check=True, capture_output=True, timeout=60)
        raw = binary.read_bytes()
        digest = hashlib.sha256(raw).hexdigest()
        catalog.write_text(worker_prototypes(declarations, digest))
        environment = dict(os.environ, HINDSIGHT_PROTOTYPE_DB=str(catalog))
        response = subprocess.run([str(worker), "--analyze", str(binary), digest, str(len(raw)), str(uuid.uuid4())],
                                  check=True, capture_output=True, env=environment, timeout=120).stdout
        assert len(response) >= 4
        length = struct.unpack_from(">I", response)[0]
        assert length <= 32 * 1024 * 1024 and len(response) == length + 4
        report = json.loads(response[4:])
        assert report["kind"] == "analysis_result", report
        pseudo = report["semantic_preview"].split("Reconstructed pseudocode\n========================\n", 1)[1]

        def body(name):
            marker = f"// unsigned long {name}(" if name != "hs_fp_frame" else f"// double {name}("
            assert marker in pseudo, (name, pseudo)
            return pseudo.split(marker, 1)[1].split("\n}\n", 1)[0]

        chain = body("hs_copy_chain")
        assert "original_copy_x5 = xzr | original_copy_x4;" in chain, chain
        assert "return original;" in chain, chain
        assert "_copy_x4_copy_" not in chain, chain
        spilled = body("hs_frame_spill")
        assert re.search(r"original_copy_x2 = \*\(sp \+ 0x8\);", spilled), spilled
        assert "return original;" in spilled, spilled
        pair = body("hs_pair_spill")
        assert "x0 = first_copy_x2 + second_copy_x3;" in pair, pair
        floating = body("hs_fp_frame")
        assert "original_copy_d1 = *(double *)(sp + 0x8);" in floating, floating
        assert "return original;" in floating, floating
        pre = body("hs_preindexed")
        assert "x0 = owner + 0x8; x1 = *(x0);" in pre, pre
        assert "x1 = *(owner" not in pre and "x1 = *(x0 + 0x8)" not in pre, pre
        overlap = body("hs_load_overlap")
        assert re.search(r"pair_addr_[0-9a-f]+ = owner; x0 = \*\(pair_addr_[0-9a-f]+\); x1 = \*\(pair_addr_[0-9a-f]+ \+ 0x8\);", overlap), overlap
        native = platform.machine() == "arm64"
        if native:
            subprocess.run([str(binary)], check=True, capture_output=True, timeout=30)
        print(json.dumps({"functions": len(FUNCTIONS), "native_execution": native,
                          "native_result_checks": len(FUNCTIONS)*260 if native else 0,
                          "scope": "parameter copies, frame/pair/FP spills, pre-index and overlapping-base loads; not source equivalence"}))


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "build/rust/debug/reworker").resolve())
