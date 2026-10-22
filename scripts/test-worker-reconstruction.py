#!/usr/bin/env python3
"""Validate real worker-produced AST/evidence artifacts, including mutations."""
import copy
import hashlib
import json
from pathlib import Path
import os
import re
import struct
import subprocess
import sys
import tempfile
import uuid

from reconstruction_artifact import (AnalysisScope, ArtifactError, ReconstructionArtifact,
                                     content_id, settings_digest)

ROOT = Path(__file__).resolve().parents[1]


def request(worker, binary, command, *arguments, digest=None, prototypes=None):
    raw = binary.read_bytes()
    environment = dict(os.environ)
    environment.pop("HINDSIGHT_PROTOTYPE_DB", None)
    if prototypes:
        environment["HINDSIGHT_PROTOTYPE_DB"]=str(prototypes)
    result = subprocess.run([str(worker), command, str(binary), digest or hashlib.sha256(raw).hexdigest(),
                             str(len(raw)), *arguments, str(uuid.uuid4())],
                            check=True, capture_output=True, env=environment, timeout=120)
    assert len(result.stdout) >= 4
    length = struct.unpack_from(">I", result.stdout)[0]
    assert length <= 32 * 1024 * 1024 and len(result.stdout) == length+4
    return json.loads(result.stdout[4:])


def invalid(document, message):
    try:
        ReconstructionArtifact(document)
    except ArtifactError:
        return
    raise AssertionError(message)


def run(worker):
    if sys.platform != "darwin":
        print("Apple Mach-O reconstruction integration requires macOS; skipped")
        return
    with tempfile.TemporaryDirectory(prefix="hindsight-worker-reconstruction-") as directory:
        root = Path(directory)
        assembly, main, binary = root / "reconstruction.s", root / "main.c", root / "reconstruction"
        assembly.write_text("""// CC0 / public-domain HindSight qualification fixture.
.text
.p2align 2
.globl _hs_ast_identity
_hs_ast_identity:
    mov x0, x1
    ret
.globl _hs_ast_sum
_hs_ast_sum:
    add w0, w0, w1
    ret
.globl _hs_ast_join
_hs_ast_join:
    cbz x0, Lelse
    mov x2, x1
    b Ljoin
Lelse:
    mov x2, x3
Ljoin:
    add x0, x2, #1
    ret
.globl _hs_ast_flags
_hs_ast_flags:
    adds x0, x0, #1
    ret
.globl _hs_ast_call
_hs_ast_call:
    bl _hs_ast_callee
    ret
.globl _hs_ast_indirect
_hs_ast_indirect:
    blr x8
    ret
.globl _hs_ast_callee
_hs_ast_callee:
    ret
.globl _hs_ast_opaque
_hs_ast_opaque:
    .long 0xffffffff
    ret
.globl _hs_ast_large
_hs_ast_large:
    .rept 130
    nop
    .endr
    ret
.globl _hs_ast_barriers
_hs_ast_barriers:
    dmb ish
    dsb sy
    isb
    clrex
    ret
""")
        main.write_text("// CC0 / public-domain HindSight qualification fixture.\nint main(void){return 0;}\n")
        subprocess.run(["xcrun", "clang", "-target", "arm64-apple-macos14.0", "-O2", str(assembly), str(main), "-o", str(binary)],
                       check=True, capture_output=True, timeout=60)
        report = request(worker, binary, "--analyze")
        assert report["kind"] == "analysis_result", report
        ranges = {match[3]: (match[1], match[2]) for line in report["functions"].splitlines()
                  if (match := re.match(r"^(0x[0-9a-f]+)\s+(0x[0-9a-f]+)\s+\d+\s+bytes\s+(\S+)", line))}
        artifacts = {}
        for name in ["identity", "sum", "join", "flags", "call", "indirect", "opaque", "large", "barriers"]:
            start, end = ranges["_hs_ast_"+name]
            document = request(worker, binary, "--reconstruct", start, end)
            assert document.get("producer") == "hindsight-reconstruction-v1", document
            validated = ReconstructionArtifact(document)
            assert document["function"]["start"] == hex(int(start, 16))
            assert document["function"]["end"] == hex(int(end, 16))
            assert not document["ast"]["compilable"] and not document["validation"]
            settings = [node["payload"]["analysis_settings"] for node in document["nodes"] if "analysis_settings" in node["payload"]]
            assert len(settings) == 1 and settings_digest(settings[0]) == document["scope"]["settings_digest"]
            repeated = request(worker, binary, "--reconstruct", start, end)
            assert document == repeated, "request identity/randomness changed reconstruction content"
            mirrors = [node for node in document["nodes"] if node["kind"] == "ast-node" and node["payload"]["ast_id"] != document["ast"]["root"]]
            assert mirrors and validated.why(mirrors[0]["id"])["nodes"]
            assert any(node["kind"] == "bytes" for node in validated.why(mirrors[0]["id"])["nodes"])
            artifacts[name] = document
        assert artifacts["identity"]["eligibility"]["state"] == "review-only"
        assert artifacts["sum"]["eligibility"]["state"] == "review-only"
        # Static Mach-O facts must not be mislabeled as observations from a
        # live dyld/libobjc process. The exact nlist symbol remains target-local.
        native_symbols = [node["payload"] for node in artifacts["identity"]["nodes"]
                          if node["kind"] == "symbol" and node["payload"].get("name") == "_hs_ast_identity"]
        assert native_symbols and any(item["source"] == "macho-nlist" for item in native_symbols), native_symbols
        assert all(item["address"] == native_symbols[0]["address"] for item in native_symbols), native_symbols
        assert not any(node["payload"].get("source") == "libobjc-runtime"
                       for node in artifacts["identity"]["nodes"]), artifacts["identity"]["nodes"]
        assert not any(node["payload"].get("kind") in {"objc-class-graph", "export-trie"}
                       for node in artifacts["identity"]["nodes"]), artifacts["identity"]["nodes"]
        for evidence in artifacts["identity"]["nodes"]:
            if evidence["payload"].get("kind") in {"export-function-start", "unwind-function-start", "lc-function-starts"}:
                assert evidence["ranges"] == [], evidence
                assert int(evidence["payload"]["address"], 0) == int(artifacts["identity"]["function"]["start"], 16), evidence
        assert any(node["kind"] == "phi" for node in artifacts["join"]["nodes"])
        assert artifacts["flags"]["eligibility"]["state"] == "review-only"
        assert any(node["kind"]=="flags-arithmetic" for node in artifacts["flags"]["ast"]["nodes"])
        barrier_events = [node for node in artifacts["barriers"]["nodes"]
                          if node["kind"] == "memory-effect" and "effect" in node["payload"]]
        barrier_events.sort(key=lambda node: int(node["ranges"][0]["start"], 16))
        assert [(node["payload"]["mnemonic"], node["payload"]["domain"]) for node in barrier_events] == [
            ("dmb", "ish"), ("dsb", "sy"), ("isb", None), ("clrex", None)], barrier_events
        assert all(node["ranges"] == [{"start": f"0x{int(artifacts['barriers']['function']['start'], 16) + index * 4:x}",
                                      "end": f"0x{int(artifacts['barriers']['function']['start'], 16) + index * 4 + 4:x}"}]
                   for index, node in enumerate(barrier_events)), barrier_events
        # C04: a direct call publishes the caller-visible AAPCS64 contract and
        # keeps its prototype-derived argument and result locations explicitly
        # unresolved until a callee prototype is proven.
        call_effects = [node["attributes"]["call_effects"] for node in artifacts["call"]["ast"]["nodes"]
                        if node["kind"] == "call" and "call_effects" in node["attributes"]]
        assert call_effects, artifacts["call"]["ast"]["nodes"]
        effect = call_effects[0]
        assert effect["abi"]["name"] == "apple-aapcs64", effect
        assert effect["abi"]["caller_saved_gpr"] == "x0-x18", effect
        assert effect["abi"]["callee_saved_gpr"] == "x19-x30", effect
        assert effect["abi"]["flags_clobbered"] is True, effect
        assert effect["call_kind"] == "direct", effect
        assert isinstance(effect["target"], int), effect
        assert effect["arguments"] == "unresolved: no proven prototype", effect
        assert effect["result"] == "unresolved: no proven prototype", effect
        # An unproven callee keeps the memory claim unknown; a proven one
        # refines it below. The summary never suppresses an unobserved write.
        assert effect["callee_summary"] is not None, effect
        # A direct call into the captured image admits a whole-function
        # observation of the callee. The callee here is a leaf `ret`, so the
        # complete walk proves it writes and reads no memory. The claim names
        # its scope, so it is never read as a claim about an unobserved callee.
        callee = effect["callee_summary"]
        assert callee["kind"] == "callee-effect-summary", callee
        assert callee["instructions"] >= 1, callee
        assert callee["observed_return"] is True, callee
        assert callee["complete"] is True, callee
        assert "whole reachable callee control flow" in callee["scope"], callee
        assert callee["memory_effect"]["writes"].startswith("none"), callee
        assert effect["memory_effect"]["writes"].startswith("none"), effect
        # An indirect call never claims a target.
        indirect_effects = [node["attributes"]["call_effects"] for node in artifacts["indirect"]["ast"]["nodes"]
                            if node["kind"] == "indirect-call" and "call_effects" in node["attributes"]]
        assert indirect_effects, artifacts["indirect"]["ast"]["nodes"]
        # The non-returning claim is stated as evidence, never as a bare
        # boolean that a consumer could read as "the callee returns".
        assert effect["non_returning"].startswith(("proven", "unresolved")), effect
        indirect = indirect_effects[0]
        assert indirect["call_kind"] == "indirect", indirect
        assert indirect["target"] is None, indirect
        # An indirect call has no callee summary, so its memory effect stays
        # explicitly unknown rather than claiming the callee writes nothing.
        assert indirect["memory_effect"]["writes"].startswith("unknown"), indirect
        assert indirect["dispatch_register"], indirect
        for name in ["opaque"]:
            assert artifacts[name]["eligibility"]["state"] == "blocked"
            assert any(node["kind"] == "unsupported-effect" for node in artifacts[name]["nodes"])
        assert artifacts["large"]["truncated"]
        assert artifacts["large"]["budgets"]["exhausted"] == ["instructions"]
        assert artifacts["large"]["eligibility"]["state"] == "blocked"

        document = artifacts["identity"]
        mutated = copy.deepcopy(document)
        mutated["nodes"][0]["payload"]["tampered"] = True
        invalid(mutated, "mutated worker evidence was accepted")
        mutated = copy.deepcopy(document)
        mutated["ast"]["nodes"][0]["ranges"][0]["end"] = "0xffffffffffffffff"
        scope = AnalysisScope(**mutated["scope"])
        node = mutated["ast"]["nodes"][0]
        node["id"] = content_id("ast", mutated["source_sha256"], mutated["function"]["id"], node, scope)
        invalid(mutated, "out-of-function AST source range was accepted")
        mutated = copy.deepcopy(document)
        mutated["eligibility"]["state"] = "equivalent"
        invalid(mutated, "unvalidated worker source acquired equivalence")
        start, end = ranges["_hs_ast_identity"]
        assert request(worker, binary, "--reconstruct", start, end, digest="0"*64)["kind"] == "error"
        assert request(worker, binary, "--reconstruct", hex(int(start, 16)+4), end)["kind"] == "error"
        assert request(worker, binary, "--reconstruct", start, hex(int(end, 16)-4))["kind"] == "error"
        link = root / "source-link"; link.symlink_to(binary)
        assert request(worker, link, "--reconstruct", start, end)["kind"] == "error"
        print(json.dumps({"worker_artifacts": len(artifacts), "deterministic": True,
                          "schema_content_ids_graph_ast_and_blockers": "passed",
                          "source_export_or_equivalence": "not claimed"}))


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv)>1 else "build/rust/debug/reworker").resolve())
