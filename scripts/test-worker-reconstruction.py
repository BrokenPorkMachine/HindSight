#!/usr/bin/env python3
"""Validate real worker-produced AST/evidence artifacts, including mutations."""
import copy
import hashlib
import json
from pathlib import Path
import os
import re
import shutil
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
.globl _hs_ast_system_flags
_hs_ast_system_flags:
    mov x0, #0xa0000000
    msr nzcv, x0
    mrs x1, nzcv
    ret
.globl _hs_ast_addcarry
_hs_ast_addcarry:
    adcs x0, x1, x2
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
.globl _hs_ast_barrier_accesses
_hs_ast_barrier_accesses:
    sub sp, sp, #16
    str x0, [sp]
    dmb ish
    ldr x1, [sp]
    add sp, sp, #16
    ret
.globl _hs_ast_barrier_join
_hs_ast_barrier_join:
    sub sp, sp, #16
    str x0, [sp]
    cbz x1, Lbarrier_join
    dmb ish
Lbarrier_join:
    ldr x2, [sp]
    add sp, sp, #16
    ret
.globl _hs_ast_vector_fp
_hs_ast_vector_fp:
    fadd v0.4s, v1.4s, v2.4s
    frecpe v3.2d, v4.2d
    ret
.globl _hs_ast_opaque_flow
_hs_ast_opaque_flow:
    mov x1, #1
    fadd v0.4s, v1.4s, v2.4s
    .long 0x0ee1d820 // reserved frecpe .1d encoding
    fadd v1.4s, v2.4s, v3.4s
    add x0, x1, x2
    csel x5, x6, x7, eq
    ret
.globl _hs_ast_opaque_memory
_hs_ast_opaque_memory:
    sub sp, sp, #16
    str x0, [sp]
    .long 0x0ee1d820 // reserved frecpe .1d encoding
    ldr x3, [sp]
    add sp, sp, #16
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
        for name in ["identity", "sum", "join", "flags", "system_flags", "addcarry", "call", "indirect", "opaque", "large", "barriers", "barrier_accesses", "barrier_join", "vector_fp", "opaque_flow", "opaque_memory"]:
            start, end = ranges["_hs_ast_"+name]
            document = request(worker, binary, "--reconstruct", start, end)
            assert document.get("producer") == "hindsight-reconstruction-v1", document
            validated = ReconstructionArtifact(document)
            assert document["function"]["start"] == hex(int(start, 16))
            assert document["function"]["end"] == hex(int(end, 16))
            assert not document["ast"]["compilable"] and not document["validation"]
            coverage = [node["payload"] for node in document["nodes"]
                        if node["payload"].get("artifact_kind") == "semantic-coverage"]
            decoded = [node["payload"] for node in document["nodes"] if node["kind"] == "decode"]
            operations = [node["payload"] for node in document["nodes"] if node["kind"] == "ir-operation"]
            blockers = [node for node in document["nodes"] if node["kind"] == "unsupported-effect"]
            assert len(coverage) == 1, (name, coverage)
            summary = coverage[0]
            assert summary["instruction_count"] == len(decoded) == len(operations), (name, summary)
            assert summary["shared_ir_count"] == sum(item["shared_ir_available"] for item in decoded), (name, summary)
            assert summary["modeled_tree_count"] == sum(item["tree"] is not None for item in operations), (name, summary)
            assert summary["data_in_code_count"] == sum(item["data_in_code"] for item in decoded), (name, summary)
            assert summary["unsupported_effect_reason_count"] == len(blockers), (name, summary)
            assert summary["strict_machine_state_blocked"] == bool(blockers or document["truncated"] or document["budgets"]["exhausted"]), (name, summary)
            assert "do not certify complete" in summary["interpretation"], (name, summary)
            settings = [node["payload"]["analysis_settings"] for node in document["nodes"] if "analysis_settings" in node["payload"]]
            assert len(settings) == 1 and settings_digest(settings[0]) == document["scope"]["settings_digest"]
            repeated = request(worker, binary, "--reconstruct", start, end)
            assert document == repeated, "request identity/randomness changed reconstruction content"
            mirrors = [node for node in document["nodes"] if node["kind"] == "ast-node" and node["payload"]["ast_id"] != document["ast"]["root"]]
            assert mirrors and validated.why(mirrors[0]["id"])["nodes"]
            assert any(node["kind"] == "bytes" for node in validated.why(mirrors[0]["id"])["nodes"])
            artifacts[name] = document
        swiftc = shutil.which("swiftc")
        if swiftc:
            contract = root / "reconstruction-artifact-contract"
            fixture = root / "identity-artifact.json"
            module_cache = root / "swift-module-cache"
            module_cache.mkdir()
            swift_environment = dict(os.environ, CLANG_MODULE_CACHE_PATH=str(module_cache))
            fixture.write_text(json.dumps(artifacts["identity"], separators=(",", ":")))
            subprocess.run([swiftc, str(ROOT / "Shared/ReconstructionArtifact.swift"),
                            str(ROOT / "Tests/reconstruction_artifact_contract.swift"), "-o", str(contract)],
                           check=True, capture_output=True, timeout=120, env=swift_environment)
            function = artifacts["identity"]["function"]
            subprocess.run([str(contract), str(fixture), artifacts["identity"]["source_sha256"],
                            function["start"][2:], function["end"][2:]], check=True, timeout=30,
                           env=swift_environment)
        assert artifacts["identity"]["eligibility"]["state"] == "review-only"
        assert artifacts["sum"]["eligibility"]["state"] == "review-only"
        system_flags = artifacts["system_flags"]
        system_operations = [node["payload"] for node in system_flags["nodes"] if node["kind"] == "ir-operation"]
        system_trees = [item["tree"]["kind"] for item in system_operations]
        assert "system-register-write" in system_trees and "system-register-read" in system_trees, system_trees
        assert all(node["payload"]["shared_ir_available"] for node in system_flags["nodes"] if node["kind"] == "decode")
        write = next(item for item in system_operations if item["tree"]["kind"] == "system-register-write")
        read = next(item for item in system_operations if item["tree"]["kind"] == "system-register-read")
        assert write["gpr_uses"] == [0] and write["gpr_definitions"] == [], write
        assert read["gpr_uses"] == [] and read["gpr_definitions"] == [1], read
        flag_uses = [node for node in system_flags["nodes"] if node["kind"] == "file-use"
                     and node["payload"].get("location", "").startswith("nzcv.")]
        flag_definitions = [node for node in system_flags["nodes"] if node["kind"] == "file-definition"
                            and node["payload"].get("origin_kind") == "instruction"
                            and node["payload"].get("location", "").startswith("nzcv.")]
        assert {node["payload"]["location"] for node in flag_uses} == {"nzcv.n", "nzcv.z", "nzcv.c", "nzcv.v"}, flag_uses
        assert {node["payload"]["location"] for node in flag_definitions} == {"nzcv.n", "nzcv.z", "nzcv.c", "nzcv.v"}, flag_definitions
        system_start = int(system_flags["function"]["start"], 16)
        assert {int(node["ranges"][0]["start"], 16) for node in flag_uses} == {system_start + 8}, flag_uses
        assert {int(node["ranges"][0]["start"], 16) for node in flag_definitions} == {system_start + 4}, flag_definitions
        assert not any(node["kind"] == "unsupported-effect" and "NZCV system-state" in node["payload"].get("reason", "")
                       for node in system_flags["nodes"]), system_flags["nodes"]
        assert any(node["payload"].get("artifact_kind") == "machine-state-c"
                   for node in system_flags["nodes"]), system_flags["nodes"]
        addcarry = artifacts["addcarry"]
        carry_pc = int(addcarry["function"]["start"], 16)
        carry_operation = next(node["payload"] for node in addcarry["nodes"]
                               if node["kind"] == "ir-operation"
                               and int(node["ranges"][0]["start"], 16) == carry_pc)
        carry_decode = next(node["payload"] for node in addcarry["nodes"]
                            if node["kind"] == "decode"
                            and int(node["ranges"][0]["start"], 16) == carry_pc)
        assert carry_decode["shared_ir_available"] and carry_operation["gpr_uses"] == [1, 2]
        assert carry_operation["gpr_definitions"] == [0] and carry_operation["tree"]["kind"] == "assignment"
        carry_uses = [node for node in addcarry["nodes"] if node["kind"] == "file-use"
                      and node["payload"].get("location") == "nzcv.c"]
        carry_definitions = [node for node in addcarry["nodes"] if node["kind"] == "file-definition"
                             and node["payload"].get("origin_kind") == "instruction"
                             and node["payload"].get("location", "").startswith("nzcv.")]
        assert len(carry_uses) == 1 and int(carry_uses[0]["ranges"][0]["start"], 16) == carry_pc
        assert {node["payload"]["location"] for node in carry_definitions} == {"nzcv.n", "nzcv.z", "nzcv.c", "nzcv.v"}
        assert all(int(node["ranges"][0]["start"], 16) == carry_pc for node in carry_definitions)
        assert addcarry["ast"]["compilable"] is False
        assert any(node["payload"].get("artifact_kind") == "machine-state-c"
                   for node in addcarry["nodes"]), addcarry["nodes"]
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
        ordered_fixture = artifacts["barrier_accesses"]
        ordered_event = next(node for node in ordered_fixture["nodes"]
                             if node["kind"] == "memory-effect" and node["payload"].get("mnemonic") == "dmb")
        ordered_edges = {edge["relation"]: edge for edge in ordered_fixture["edges"]
                         if edge["target"] == ordered_event["id"] or edge["source"] == ordered_event["id"]}
        assert "program-order-before-event" in ordered_edges and "program-order-after-event" in ordered_edges, (ordered_fixture["function"], [(node["kind"], node["payload"], node["ranges"]) for node in ordered_fixture["nodes"] if node["kind"] in {"memory-access", "memory-effect"}], ordered_edges)
        before_edge = ordered_edges["program-order-before-event"]
        after_edge = ordered_edges["program-order-after-event"]
        node_by_id = {node["id"]: node for node in ordered_fixture["nodes"]}
        assert node_by_id[before_edge["source"]]["kind"] == "memory-access"
        assert node_by_id[after_edge["target"]]["kind"] == "memory-access"
        assert node_by_id[before_edge["source"]]["ranges"][0]["start"] < ordered_event["ranges"][0]["start"]
        assert node_by_id[after_edge["target"]]["ranges"][0]["start"] > ordered_event["ranges"][0]["start"]
        join_fixture = artifacts["barrier_join"]
        join_event = next(node for node in join_fixture["nodes"]
                          if node["kind"] == "memory-effect" and node["payload"].get("mnemonic") == "dmb")
        assert not any(edge["relation"] in {"program-order-before-event", "program-order-after-event"}
                       and (edge["source"] == join_event["id"] or edge["target"] == join_event["id"])
                       for edge in join_fixture["edges"]), join_fixture["edges"]
        vector_fp_start = int(artifacts["vector_fp"]["function"]["start"], 16)
        vector_uses = [node for node in artifacts["vector_fp"]["nodes"] if node["kind"] == "file-use"]
        expected_uses = {
            ("v1", vector_fp_start, ((0, 16),)),
            ("v2", vector_fp_start, ((0, 16),)),
            ("v4", vector_fp_start + 4, ((0, 16),)),
        }
        actual_uses = {
            (node["payload"]["location"], int(node["ranges"][0]["start"], 16),
             tuple((item["offset"], item["bytes"]) for item in node["payload"]["byte_ranges"]))
            for node in vector_uses
        }
        assert expected_uses <= actual_uses, actual_uses
        vector_definitions = [node for node in artifacts["vector_fp"]["nodes"]
                              if node["kind"] == "file-definition" and node["payload"].get("origin_kind") == "instruction"]
        actual_definitions = {
            (node["payload"]["location"], int(node["ranges"][0]["start"], 16),
             tuple((item["offset"], item["bytes"]) for item in node["payload"]["byte_ranges"]))
            for node in vector_definitions
        }
        assert ("v0", vector_fp_start, ((0, 16),)) in actual_definitions, actual_definitions
        assert ("v3", vector_fp_start + 4, ((0, 16),)) in actual_definitions, actual_definitions
        opaque_start = int(artifacts["opaque_flow"]["function"]["start"], 16)
        opaque_pc = opaque_start + 8
        opaque_nodes = artifacts["opaque_flow"]["nodes"]
        opaque_edges = artifacts["opaque_flow"]["edges"]
        opaque_gpr_defs = [node for node in opaque_nodes if node["kind"] == "definition"
                           and node["payload"].get("origin_kind") == "unknown-barrier"
                           and int(node["ranges"][0]["start"], 16) == opaque_pc]
        assert {node["payload"]["register"] for node in opaque_gpr_defs} == set(range(32)), opaque_gpr_defs
        opaque_file_defs = [node for node in opaque_nodes if node["kind"] == "file-definition"
                            and node["payload"].get("origin_kind") == "unknown-barrier"
                            and int(node["ranges"][0]["start"], 16) == opaque_pc]
        assert {node["payload"]["location"] for node in opaque_file_defs} == {
            *(f"v{register}" for register in range(32)), "nzcv.n", "nzcv.z", "nzcv.c", "nzcv.v"
        }, opaque_file_defs
        post_barrier_uses = [node for node in opaque_nodes
                             if node["kind"] in {"use", "file-use"}
                             and int(node["ranges"][0]["start"], 16) in {opaque_pc + 4, opaque_pc + 8, opaque_pc + 12}]
        gpr_use_nodes = [node for node in post_barrier_uses if node["kind"] == "use"
                         and node["payload"].get("origin_kind") == "unknown-barrier"]
        gpr_uses = {(node["payload"]["register"], int(node["ranges"][0]["start"], 16))
                    for node in gpr_use_nodes}
        expected_gpr_uses = {(2, opaque_pc + 8), (1, opaque_pc + 8),
                             (6, opaque_pc + 12), (7, opaque_pc + 12)}
        assert expected_gpr_uses <= gpr_uses, gpr_uses
        file_uses = [node for node in post_barrier_uses if node["kind"] == "file-use"]
        vector_uses = [node for node in file_uses if node["payload"]["location"] in {"v2", "v3"}
                       and int(node["ranges"][0]["start"], 16) == opaque_pc + 4]
        assert len(vector_uses) == 2 and all(f"unknown({hex(opaque_pc)})" in node["payload"]["value"]
                                              for node in vector_uses), vector_uses
        flag_uses = [node for node in file_uses if node["payload"]["location"] == "nzcv.z"
                     and int(node["ranges"][0]["start"], 16) == opaque_pc + 12]
        assert len(flag_uses) == 1 and f"unknown({hex(opaque_pc)})" in flag_uses[0]["payload"]["value"], flag_uses
        opaque_file_def_ids = {node["id"] for node in opaque_file_defs}
        assert all(any(edge["source"] in opaque_file_def_ids and edge["target"] == use["id"]
                       and edge["relation"] == "flows-to" for edge in opaque_edges)
                   for use in vector_uses + flag_uses), opaque_edges
        gpr_definition_ids = {node["payload"]["register"]: node["id"] for node in opaque_gpr_defs}
        assert all(any(edge["source"] == gpr_definition_ids[use["payload"]["register"]]
                       and edge["target"] == use["id"] and edge["relation"] == "flows-to"
                       for edge in opaque_edges) for use in gpr_use_nodes), opaque_edges
        opaque_memory_start = int(artifacts["opaque_memory"]["function"]["start"], 16)
        memory_barrier_pc = opaque_memory_start + 8
        memory_load_pc = opaque_memory_start + 12
        memory_nodes = artifacts["opaque_memory"]["nodes"]
        memory_edges = artifacts["opaque_memory"]["edges"]
        frame_barrier_defs = [node for node in memory_nodes if node["kind"] == "memory-definition"
                              and node["payload"].get("partition") == "frame"
                              and node["payload"].get("origin") == "instruction"
                              and int(node["ranges"][0]["start"], 16) == memory_barrier_pc]
        frame_load_uses = [node for node in memory_nodes if node["kind"] == "memory-use"
                           and node["payload"].get("partition") == "frame"
                           and int(node["ranges"][0]["start"], 16) == memory_load_pc]
        assert len(frame_barrier_defs) == len(frame_load_uses) == 1, (frame_barrier_defs, frame_load_uses)
        assert frame_barrier_defs[0]["payload"]["version"] == frame_load_uses[0]["payload"]["version"], (frame_barrier_defs, frame_load_uses)
        assert any(edge["source"] == frame_barrier_defs[0]["id"] and edge["target"] == frame_load_uses[0]["id"]
                   and edge["relation"] == "flows-to" for edge in memory_edges), memory_edges
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
            review = [node["payload"] for node in artifacts[name]["nodes"] if node["payload"].get("artifact_kind") == "best-effort-source"]
            assert len(review) == 1 and "loc_" in review[0]["source"] and "BEST-EFFORT" in review[0]["source"]
            assert review[0]["status"] == "review-only-uncompiled"
        assert artifacts["large"]["truncated"]
        assert any("truncated: true" in node["payload"].get("source", "") for node in artifacts["large"]["nodes"])
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
