#!/usr/bin/env python3
"""Compile reconstructed void setters and compare native memory effects."""
import hashlib
import ctypes
import importlib.util
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("worker_requests", ROOT / "scripts/test-worker-reconstruction.py")
requests = importlib.util.module_from_spec(spec)
spec.loader.exec_module(requests)


def qualify_interrupted_batches(root, binary, worker, prototypes, reference):
    """SIGKILL a real CLI process at two publication boundaries, then restart."""
    launcher = """
import importlib.util, os, pathlib, signal, sys, types
sys.path.insert(0, sys.argv[1])
spec = importlib.util.spec_from_file_location('interrupted_batch', pathlib.Path(sys.argv[1]) / 'reconstruct-binary.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
publish = module.publish
def interrupted_publish(path, value):
    if path.name == 'checkpoint.json':
        if sys.argv[6] == 'before-second-checkpoint' and len(value['functions']) == 2:
            os.kill(os.getpid(), signal.SIGKILL)
        publish(path, value)
        if sys.argv[6] == 'after-first-checkpoint' and len(value['functions']) == 1:
            os.kill(os.getpid(), signal.SIGKILL)
    else:
        publish(path, value)
module.publish = interrupted_publish
module.run(types.SimpleNamespace(binary=pathlib.Path(sys.argv[2]), output=pathlib.Path(sys.argv[3]),
    worker=pathlib.Path(sys.argv[4]), prototypes=pathlib.Path(sys.argv[5]), resume=False,
    start=None, end=None, include_callees=False, max_functions=3, max_bytes=256*1024*1024))
"""
    for boundary in ["after-first-checkpoint", "before-second-checkpoint"]:
        output = root / boundary
        killed = subprocess.run([sys.executable, "-c", launcher, str(ROOT / "scripts"),
                                 str(binary), str(output), str(worker), str(prototypes), boundary],
                                capture_output=True, timeout=120)
        assert killed.returncode == -signal.SIGKILL, (boundary, killed.stderr)
        checkpoint = json.loads((output / "checkpoint.json").read_bytes())
        assert checkpoint["attempted"] == 1 and len(checkpoint["functions"]) == 1
        assert not (output / "manifest.json").exists()
        retained = {path.name: (path.read_bytes(), path.stat().st_mtime_ns)
                    for path in output.iterdir() if path.name.startswith("function-")}
        expected_files = sum(len(entry["files"]) for entry in reference["functions"][:
                             1 if boundary == "after-first-checkpoint" else 2])
        assert len(retained) == expected_files > 0, (boundary, retained)
        resumed = subprocess.run([sys.executable, str(ROOT / "scripts/reconstruct-binary.py"),
                                  str(binary), str(output), "--worker", str(worker),
                                  "--prototypes", str(prototypes), "--max-functions", "3", "--resume"],
                                 capture_output=True, timeout=120)
        assert resumed.returncode == 0, (boundary, resumed.stderr)
        complete = json.loads((output / "manifest.json").read_bytes())
        assert complete["attempted"] == 3
        assert complete["functions"] == reference["functions"][:3]
        for name, (raw, timestamp) in retained.items():
            assert (output / name).read_bytes() == raw
            assert (output / name).stat().st_mtime_ns == timestamp
    print("CLI SIGKILL/restart passed at checkpoint and pre-checkpoint file boundaries; retained and orphan outputs preserved exactly.")


def run(worker):
    if sys.platform != "darwin":
        raise SystemExit("This qualification requires the Apple Mach-O toolchain.")
    from reconstruction_artifact import ReconstructionArtifact
    with tempfile.TemporaryDirectory(prefix="hindsight-void-") as directory:
        root = Path(directory)
        fixture = root / "fixture.c"
        fixture.write_text("""
#include <stdint.h>
#include <stdbool.h>
bool bool_identity(bool first, bool second) { (void)second; return first; }
bool bool_not(bool first, bool second) { (void)second; return !first; }
bool bool_and(bool first, bool second) { return first & second; }
bool bool_compare(uint64_t first, uint64_t second) { return first != second; }
bool bool_load(const bool *value) { return *value; }
void bool_store(bool *destination, bool value) { *destination = value; }
bool bool_exchange(bool *destination, bool value) { bool previous = *destination; *destination = value; return previous; }
bool bool_volatile_load(const volatile bool *value) { return *value; }
void bool_volatile_store(volatile bool *destination, bool value) { *destination = value; }
bool bool_index_load(const volatile bool *value, uint64_t index) { return value[index]; }
void bool_index_store(volatile bool *destination, uint64_t index, bool value) { destination[index] = value; }
bool bool_index_exchange(volatile bool *destination, uint64_t index, bool value) { bool previous = destination[index]; destination[index] = value; return previous; }
void empty(void) {}
void setter(uint64_t *destination, uint64_t value) { *destination = value; }
void conditional(uint64_t *destination, uint64_t value) { if (value) *destination = value; }
""")
        assembly = root / "discard.s"
        assembly.write_text("""
.text
.p2align 2
.globl _discard
_discard:
stp x29, x30, [sp, #-16]!
bl _leaf
ldp x29, x30, [sp], #16
ret
.globl _bool_store_bad
_bool_store_bad:
mov w1, #2
strb w1, [x0]
ret
.globl _blocked_header
_blocked_header:
brk #0
ret
.globl _discard_scalar
_discard_scalar:
stp x29, x30, [sp, #-16]!
bl _leaf
mov x0, #19
ldp x29, x30, [sp], #16
ret
.globl _reuse_scalar
_reuse_scalar:
stp x29, x30, [sp, #-16]!
bl _leaf
add x0, x0, x0
ldp x29, x30, [sp], #16
ret
.globl _leaf
_leaf:
mov x0, #7
ret
.globl _bool_loop
_bool_loop:
cbz x0, 2f
1:
eor w1, w1, #1
sub x0, x0, #1
cbnz x0, 1b
2:
mov w0, w1
ret
.globl _bool_loop_bad
_bool_loop_bad:
cbz x0, 2f
1:
add w1, w1, #2
sub x0, x0, #1
cbnz x0, 1b
2:
mov w0, w1
ret
.globl _void_call
_void_call:
stp x29, x30, [sp, #-16]!
bl _empty
ldp x29, x30, [sp], #16
ret
.globl _bool_leaf
_bool_leaf:
eor w0, w0, #1
ret
.globl _bool_bad_leaf
_bool_bad_leaf:
mov w0, #2
ret
.globl _bool_call
_bool_call:
stp x29, x30, [sp, #-16]!
bl _bool_leaf
ldp x29, x30, [sp], #16
ret
.globl _bool_call_bad
_bool_call_bad:
stp x29, x30, [sp, #-16]!
bl _bool_bad_leaf
ldp x29, x30, [sp], #16
ret
.globl _void_call_bad_result
_void_call_bad_result:
stp x29, x30, [sp, #-16]!
bl _empty
ldp x29, x30, [sp], #16
ret
.globl _guarded_void
_guarded_void:
stp x29, x30, [sp, #-16]!
cbz x0, 1f
bl _empty
1:
ldp x29, x30, [sp], #16
ret
.globl _void_then_store
_void_then_store:
stp x19, x30, [sp, #-16]!
mov x19, x0
bl _empty
mov x1, #9
str x1, [x19]
ldp x19, x30, [sp], #16
ret
""")
        comparisons = 0
        for optimization in ["-O0", "-O1", "-O2", "-Os"]:
            binary = root / "original.dylib"
            subprocess.run(["xcrun", "clang", optimization, "-fno-stack-protector",
                            "-dynamiclib", str(fixture), str(assembly), "-o", str(binary)], check=True)
            digest = hashlib.sha256(binary.read_bytes()).hexdigest()
            prototypes = root / "prototypes.tsv"
            prototypes.write_text(f"# source_sha256={digest}\n_empty\tvoid empty(void);\n"
                                  "_setter\tvoid setter(uint64_t *destination, uint64_t value);\n"
                                  "_conditional\tvoid conditional(uint64_t *destination, uint64_t value);\n"
                                  "_discard\tvoid discard(void);\n_leaf\tuint64_t leaf(void);\n"
                                  "_discard_scalar\tuint64_t discard_scalar(void);\n"
                                  "_reuse_scalar\tuint64_t reuse_scalar(void);\n"
                                  "_void_call\tvoid void_call(void);\n"
                                  "_void_call_bad_result\tuint64_t void_call_bad_result(void);\n"
                                  "_guarded_void\tvoid guarded_void(uint64_t value);\n"
                                  "_void_then_store\tvoid void_then_store(uint64_t *destination, uint64_t value);\n"
                                  "_bool_identity\t_Bool bool_identity(_Bool first, _Bool second);\n"
                                  "_bool_not\tbool bool_not(bool first, bool second);\n"
                                  "_bool_and\tbool bool_and(bool first, bool second);\n"
                                  "_bool_compare\tbool bool_compare(uint64_t first, uint64_t second);\n"
                                  "_bool_loop\tbool bool_loop(uint64_t count, bool value);\n"
                                  "_bool_loop_bad\tbool bool_loop_bad(uint64_t count, bool value);\n"
                                  "_bool_leaf\tbool bool_leaf(bool first, bool second);\n"
                                  "_bool_bad_leaf\tbool bool_bad_leaf(bool first, bool second);\n"
                                  "_bool_call\tbool bool_call(bool first, bool second);\n"
                                  "_bool_call_bad\tbool bool_call_bad(bool first, bool second);\n"
                                  "_bool_load\tbool bool_load(const bool *value);\n"
                                  "_bool_store\tvoid bool_store(bool *destination, bool value);\n"
                                  "_bool_exchange\tbool bool_exchange(bool *destination, bool value);\n"
                                  "_bool_volatile_load\tbool bool_volatile_load(const volatile bool *value);\n"
                                  "_bool_volatile_store\tvoid bool_volatile_store(volatile bool *destination, bool value);\n"
                                  "_bool_index_load\tbool bool_index_load(const volatile bool *value, uint64_t index);\n"
                                  "_bool_index_store\tvoid bool_index_store(volatile bool *destination, uint64_t index, bool value);\n"
                                  "_bool_index_exchange\tbool bool_index_exchange(volatile bool *destination, uint64_t index, bool value);\n"
                                  "_blocked_header\tvoid blocked_header(void);\n"
                                  "_bool_store_bad\tvoid bool_store_bad(bool *destination);\n")
            report = requests.request(worker, binary, "--analyze")
            ranges = {}
            for line in report["functions"].splitlines():
                match = re.match(r"^(0x[0-9a-f]+)\s+(0x[0-9a-f]+)\s+\d+\s+bytes\s+(\S+)", line)
                if match:
                    ranges[match[3]] = (match[1], match[2])
            if optimization == "-Os":
                output = root / "batch"
                subprocess.run([sys.executable, str(ROOT / "scripts/reconstruct-binary.py"), str(binary),
                                str(output), "--worker", str(worker), "--prototypes", str(prototypes)],
                               check=True, timeout=120)
                manifest = json.loads((output / "manifest.json").read_text())
                assert manifest["source_sha256"] == digest and manifest["omitted_by_limit"] == 0
                assert manifest["attempted"] == len(manifest["functions"]) > 1
                assert manifest["status"] == "completed" and manifest["failed_functions"] == 0
                assert manifest["recovered_functions"] + manifest["review_only_functions"] == manifest["attempted"]
                failed_output = root / "failed-budget"
                failed_command = [sys.executable, str(ROOT / "scripts/reconstruct-binary.py"), str(binary),
                                  str(failed_output), "--worker", str(worker), "--prototypes", str(prototypes),
                                  "--max-functions", "1", "--max-bytes", "1"]
                failed = subprocess.run(failed_command, capture_output=True, timeout=120)
                assert failed.returncode == 1, failed.stderr
                failed_manifest = json.loads((failed_output / "manifest.json").read_bytes())
                assert failed_manifest["status"] == "completed-with-failures"
                assert failed_manifest["failed_functions"] == failed_manifest["attempted"] == 1
                assert failed_manifest["recovered_functions"] == failed_manifest["review_only_functions"] == 0
                assert failed_manifest["functions"][0]["state"] == "failed" and not failed_manifest["functions"][0]["files"]
                failed_resumed = subprocess.run(failed_command + ["--resume"], capture_output=True, timeout=120)
                assert failed_resumed.returncode == 1, failed_resumed.stderr
                assert json.loads((failed_output / "manifest.json").read_bytes())["functions"] == failed_manifest["functions"]
                qualify_interrupted_batches(root, binary, worker, prototypes, manifest)
                for entry in manifest["functions"]:
                    assert entry["state"] != "failed", entry
                    for exported in entry["files"]:
                        data = (output / exported["path"]).read_bytes()
                        assert len(data) == exported["bytes"]
                        assert hashlib.sha256(data).hexdigest() == exported["sha256"]
                recovered = [entry for entry in manifest["functions"] if entry["state"] == "recovered"]
                assert len(recovered) >= 9
                assert all(any(file["path"].endswith(".h") for file in entry["files"]) for entry in recovered)
                subprocess.run([sys.executable, str(ROOT / "scripts/qualify-reconstruction-batch.py"),
                                str(output)], check=True, timeout=120)
                built = json.loads((output / "build-qualification/report.json").read_text())
                assert built["status"] == "passed" and len(built["compiled"]) == len(recovered)
                assert len(built["headers"]) == len(manifest["functions"])
                assert len(built["unrecovered"]) == 6
                batch_manifest_bytes = (output / "manifest.json").read_bytes()
                dump_tool = str(ROOT / "scripts/export-reconstruction-batch.py")
                for format_name, input_suffix, suffix in [("c", ".c", ".c"), ("objc", ".c", ".m"),
                                                          ("header", ".h", ".h"), ("native", ".machine.c", ".n")]:
                    dump = root / ("dump-" + format_name)
                    dump_command = [sys.executable, dump_tool, str(output), str(dump), "--format", format_name]
                    subprocess.run(dump_command, check=True, timeout=120)
                    dump_manifest_bytes = (dump / "export-manifest.json").read_bytes()
                    dumped = json.loads(dump_manifest_bytes)
                    expected_count = sum(any(file["path"] == f"function-{int(entry['start'],16):016x}{input_suffix}"
                                             for file in entry["files"]) for entry in manifest["functions"])
                    if format_name == "native":
                        expected_count = sum(any(file["path"].endswith(".machine.c") for file in entry["files"]) or
                                             entry["state"] == "review-only" for entry in manifest["functions"])
                    assert dumped["available"] == expected_count > 0
                    assert dumped["available"] + dumped["unavailable"] == len(manifest["functions"])
                    assert dumped["source_batch_manifest_sha256"] == hashlib.sha256(batch_manifest_bytes).hexdigest()
                    for identity_key in ["worker_sha256", "prototypes_sha256", "source_bytes"]:
                        assert dumped[identity_key] == manifest[identity_key]
                    assert dumped["batch_scope"] == manifest["scope"]
                    for entry in dumped["functions"]:
                        assert entry["state"] == entry["recovery_state"]
                        prefix = f"function-{int(entry['start'],16):016x}"
                        assert entry["available"] == any(file["path"] == prefix + suffix for file in entry["files"])
                        for file in entry["files"]:
                            raw = (dump / file["path"]).read_bytes()
                            assert len(raw) == file["bytes"] and hashlib.sha256(raw).hexdigest() == file["sha256"]
                            selected_suffix = ".c" if entry.get("representation") == "best-effort-source" else input_suffix
                            if entry.get("representation") == "machine-state-c":
                                selected_suffix = ".machine.c"
                            original = prefix + (".reconstruction.json" if file["path"].endswith(".json") else selected_suffix)
                            assert raw == (output / original).read_bytes()
                            if format_name == "native" and entry.get("representation") == "machine-state-c" and file["path"].endswith(".n"):
                                subprocess.run(["xcrun", "clang", "-fsyntax-only", "-x", "c", "-std=c17",
                                                "-Wall", "-Wextra", "-Werror", str(dump / file["path"])], check=True)
                    duplicate = subprocess.run(dump_command, capture_output=True, timeout=120)
                    assert duplicate.returncode != 0 and (dump / "export-manifest.json").read_bytes() == dump_manifest_bytes
                first_entry = recovered[0]
                for scoped, arguments in [("address", ["--address", first_entry["start"]]),
                                          ("range", ["--start", first_entry["start"], "--end", first_entry["end"]])]:
                    scoped_output = root / ("dump-" + scoped)
                    subprocess.run([sys.executable, dump_tool, str(output), str(scoped_output), "--format", "c", *arguments],
                                   check=True, timeout=120)
                    scoped_manifest = json.loads((scoped_output / "export-manifest.json").read_text())
                    assert len(scoped_manifest["functions"]) == 1 and scoped_manifest["functions"][0]["start"] == first_entry["start"]
                for label, arguments in [("budget", ["--max-bytes", "1"]), ("empty", ["--address", "0xffffffffffffffff"])]:
                    refused_output = root / ("dump-refused-" + label)
                    refused = subprocess.run([sys.executable, dump_tool, str(output), str(refused_output), "--format", "c", *arguments],
                                             capture_output=True, timeout=120)
                    assert refused.returncode != 0 and not refused_output.exists()
                assert (output / "manifest.json").read_bytes() == batch_manifest_bytes
                # Rehashing a changed export must not detach it from the
                # independently validated worker artifact that produced it.
                for tamper in ["body", "header", "state", "missing-output", "source-identity", "bounds", "graph"]:
                    tampered_output = root / ("qualification-tamper-" + tamper)
                    tampered_output.mkdir()
                    altered_manifest = json.loads(json.dumps(manifest))
                    entry = json.loads(json.dumps(recovered[0]))
                    altered_manifest["functions"] = [entry]
                    altered_manifest["attempted"] = 1
                    for exported in entry["files"]:
                        (tampered_output / exported["path"]).write_bytes((output / exported["path"]).read_bytes())
                    if tamper in {"body", "header", "graph"}:
                        suffix = {"body": ".c", "header": ".h", "graph": ".reconstruction.json"}[tamper]
                        exported = next(file for file in entry["files"] if file["path"].endswith(suffix) and not file["path"].endswith(".machine.c"))
                        path = tampered_output / exported["path"]
                        if tamper == "graph":
                            changed_artifact = json.loads(path.read_bytes())
                            changed_artifact["nodes"][0]["payload"]["tampered"] = True
                            path.write_text(json.dumps(changed_artifact))
                        else:
                            path.write_bytes(path.read_bytes() + b"\n/* changed after recovery */\n")
                        exported["bytes"] = path.stat().st_size
                        exported["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
                    elif tamper == "state":
                        entry["state"] = "review-only"
                    elif tamper == "missing-output":
                        entry["files"] = [file for file in entry["files"] if not file["path"].endswith(".h")]
                    elif tamper == "source-identity":
                        altered_manifest["source_sha256"] = "0" * 64
                    elif tamper == "bounds":
                        entry["end"] = hex(int(entry["end"], 16) + 4)
                    (tampered_output / "manifest.json").write_text(json.dumps(altered_manifest))
                    refused = subprocess.run([sys.executable, str(ROOT / "scripts/qualify-reconstruction-batch.py"),
                                              str(tampered_output)], capture_output=True, timeout=120)
                    assert refused.returncode != 0, tamper
                    refusal = json.loads((tampered_output / "build-qualification/report.json").read_text())
                    assert refusal["status"] == "failed" and not refusal["compiled"] and not refusal["headers"], (tamper, refusal)
                    assert not (tampered_output / "build-qualification/recovered.dylib").exists(), tamper
                    refused_dump = root / ("dump-refused-" + tamper)
                    refused = subprocess.run([sys.executable, dump_tool, str(tampered_output), str(refused_dump), "--format", "header"],
                                             capture_output=True, timeout=120)
                    assert refused.returncode != 0 and not refused_dump.exists(), tamper
                    checkpoint = tampered_output / "checkpoint.json"
                    checkpoint.write_text(json.dumps(altered_manifest))
                    checkpoint_before = checkpoint.read_bytes()
                    manifest_before = (tampered_output / "manifest.json").read_bytes()
                    refused = subprocess.run([sys.executable, str(ROOT / "scripts/reconstruct-binary.py"),
                                              str(binary), str(tampered_output), "--worker", str(worker),
                                              "--prototypes", str(prototypes), "--max-functions", "1", "--resume"],
                                             capture_output=True, timeout=120)
                    assert refused.returncode != 0, (tamper, refused.stdout, refused.stderr)
                    assert checkpoint.read_bytes() == checkpoint_before, tamper
                    assert (tampered_output / "manifest.json").read_bytes() == manifest_before, tamper
                print("Batch qualification, dump and resume rejected seven rehashed evidence/state/identity tamper cases without replacing retained metadata.")
                race_output = root / "qualification-snapshot-race"
                race_output.mkdir()
                race_manifest = json.loads(json.dumps(manifest))
                race_entry = json.loads(json.dumps(recovered[0]))
                race_manifest["functions"] = [race_entry]
                race_manifest["attempted"] = 1
                frozen = {}
                for exported in race_entry["files"]:
                    raw = (output / exported["path"]).read_bytes()
                    frozen[exported["path"]] = raw
                    (race_output / exported["path"]).write_bytes(raw)
                (race_output / "manifest.json").write_text(json.dumps(race_manifest))
                qualifier_spec = importlib.util.spec_from_file_location("qualification_snapshot_test", ROOT / "scripts/qualify-reconstruction-batch.py")
                qualifier = importlib.util.module_from_spec(qualifier_spec)
                qualifier_spec.loader.exec_module(qualifier)
                original_execute = qualifier.execute
                race_mutated = []

                def snapshot_execute(arguments):
                    if "-fsyntax-only" in arguments or "-c" in arguments:
                        source_path = Path(arguments[arguments.index("-c") + 1] if "-c" in arguments else arguments[-1])
                        if not race_mutated:
                            body = next(name for name in frozen if name.endswith(".c") and not name.endswith(".machine.c"))
                            (race_output / body).write_bytes(b"changed after validation: must never be compiled")
                            race_mutated.append(True)
                        assert source_path.parent != race_output
                        assert source_path.read_bytes() == frozen[source_path.name]
                    return original_execute(arguments)

                with patch.object(qualifier, "execute", side_effect=snapshot_execute):
                    assert qualifier.run(race_output)
                assert race_mutated
                print("Batch compilation used the verified snapshot despite a later mutation of the input body.")
                implementation_report_bytes = (output / "build-qualification/report.json").read_bytes()
                subprocess.run([sys.executable, str(ROOT / "scripts/qualify-reconstruction-batch.py"),
                                str(output), "--headers-only"], check=True, timeout=120)
                mixed_headers = json.loads((output / "header-qualification/report.json").read_text())
                assert mixed_headers["status"] == "passed" and len(mixed_headers["headers"]) == len(manifest["functions"])
                assert not mixed_headers["compiled"] and "library_sha256" not in mixed_headers
                assert not (output / "header-qualification/recovered.dylib").exists()
                assert (output / "build-qualification/report.json").read_bytes() == implementation_report_bytes
                for entry in manifest["functions"]:
                    if entry["state"] == "review-only":
                        assert any(exported["path"].endswith(".h") for exported in entry["files"]), entry
                declaration_output = root / "declarations-only"
                start, end = ranges["_blocked_header"]
                declaration_command = [sys.executable, str(ROOT / "scripts/reconstruct-binary.py"), str(binary),
                                       str(declaration_output), "--worker", str(worker), "--prototypes", str(prototypes),
                                       "--start", start, "--end", end]
                subprocess.run(declaration_command, check=True, timeout=120)
                declaration_manifest = (declaration_output / "manifest.json").read_bytes()
                qualify_declarations = [sys.executable, str(ROOT / "scripts/qualify-reconstruction-batch.py"), str(declaration_output)]
                subprocess.run(qualify_declarations + ["--headers-only"], check=True, timeout=120)
                declaration_report_path = declaration_output / "header-qualification/report.json"
                declaration_report_bytes = declaration_report_path.read_bytes()
                declaration_report = json.loads(declaration_report_bytes)
                assert declaration_report["status"] == "passed" and declaration_report["mode"] == "headers-only"
                assert len(declaration_report["headers"]) == len(declaration_report["unrecovered"]) == 1
                assert not declaration_report["compiled"] and "library_sha256" not in declaration_report
                assert not (declaration_output / "header-qualification/recovered.dylib").exists()
                implementation_rejected = subprocess.run(qualify_declarations, capture_output=True, timeout=120)
                assert implementation_rejected.returncode != 0
                assert json.loads((declaration_output / "build-qualification/report.json").read_text())["status"] == "failed"
                duplicate_rejected = subprocess.run(qualify_declarations + ["--headers-only"], capture_output=True, timeout=120)
                assert duplicate_rejected.returncode != 0
                assert declaration_report_path.read_bytes() == declaration_report_bytes
                assert (declaration_output / "manifest.json").read_bytes() == declaration_manifest
                # The same trapped body without declaration evidence must not
                # turn an empty syntax-check run into a qualification pass.
                absent_output = root / "no-declarations"
                subprocess.run([sys.executable, str(ROOT / "scripts/reconstruct-binary.py"), str(binary), str(absent_output),
                                "--worker", str(worker), "--start", start, "--end", end], check=True, timeout=120)
                absent = subprocess.run([sys.executable, str(ROOT / "scripts/qualify-reconstruction-batch.py"),
                                         str(absent_output), "--headers-only"], capture_output=True, timeout=120)
                assert absent.returncode != 0
                assert json.loads((absent_output / "header-qualification/report.json").read_text())["status"] == "failed"
                # Only the test-owned generated fixture library is executed.
                library = ctypes.CDLL(str(output / "build-qualification/recovered.dylib"))
                library.setter.argtypes = [ctypes.POINTER(ctypes.c_uint64), ctypes.c_uint64]
                library.setter.restype = None
                for value in [0, 1, 2, 2**64 - 1]:
                    memory = (ctypes.c_uint64 * 3)(17, 23, 31)
                    library.setter(ctypes.cast(ctypes.byref(memory, 8), ctypes.POINTER(ctypes.c_uint64)), value)
                    assert list(memory) == [17, value, 31]
                library.reuse_scalar.restype = ctypes.c_uint64
                assert library.reuse_scalar() == 14
                ranged_output = root / "range"
                start, end = ranges["_setter"]
                subprocess.run([sys.executable, str(ROOT / "scripts/reconstruct-binary.py"), str(binary),
                                str(ranged_output), "--worker", str(worker), "--prototypes", str(prototypes),
                                "--start", start, "--end", end], check=True, timeout=120)
                ranged = json.loads((ranged_output / "manifest.json").read_text())
                assert ranged["attempted"] == ranged["discovered_in_scope"] == 1
                assert int(ranged["functions"][0]["start"], 16) == int(start, 16)
                missing_output = root / "missing-dependency"
                start, end = ranges["_discard_scalar"]
                subprocess.run([sys.executable, str(ROOT / "scripts/reconstruct-binary.py"), str(binary),
                                str(missing_output), "--worker", str(worker), "--prototypes", str(prototypes),
                                "--start", start, "--end", end], check=True, timeout=120)
                missing = subprocess.run([sys.executable, str(ROOT / "scripts/qualify-reconstruction-batch.py"),
                                          str(missing_output)], capture_output=True, timeout=120)
                assert missing.returncode != 0
                failure = json.loads((missing_output / "build-qualification/report.json").read_text())
                assert failure["status"] == "failed" and "leaf" in failure["error"]
                closure_output = root / "dependency-closure"
                closure_command = [sys.executable, str(ROOT / "scripts/reconstruct-binary.py"), str(binary),
                                   str(closure_output), "--worker", str(worker),
                                   "--prototypes", str(prototypes), "--start", start, "--end", end,
                                   "--include-callees"]
                subprocess.run(closure_command, check=True, timeout=120)
                closure = json.loads((closure_output / "manifest.json").read_text())
                assert closure["attempted"] == 2 and closure["discovered_in_scope"] == 1
                assert all(not entry["unresolved_dependencies"] for entry in closure["functions"])
                subprocess.run([sys.executable, str(ROOT / "scripts/qualify-reconstruction-batch.py"),
                                str(closure_output)], check=True, timeout=120)
                subprocess.run(closure_command + ["--resume"], check=True, timeout=120)
                resumed_output = root / "resumed"
                command = [sys.executable, str(ROOT / "scripts/reconstruct-binary.py"), str(binary),
                           str(resumed_output), "--worker", str(worker), "--prototypes", str(prototypes)]
                subprocess.run(command + ["--max-functions", "1"], check=True, timeout=120)
                first = json.loads((resumed_output / "checkpoint.json").read_text())["functions"][0]
                preserved = resumed_output / first["files"][0]["path"]
                timestamp = preserved.stat().st_mtime_ns
                resumed = subprocess.run(command + ["--resume"], check=True, capture_output=True,
                                         text=True, timeout=120)
                assert "retained" in resumed.stdout
                assert preserved.stat().st_mtime_ns == timestamp
                complete = json.loads((resumed_output / "manifest.json").read_text())
                assert complete["attempted"] == manifest["attempted"]
                # Tampered completed outputs must be refused without replacement.
                preserved.write_bytes(preserved.read_bytes() + b"\nchanged")
                changed = preserved.read_bytes()
                rejected = subprocess.run(command + ["--resume"], capture_output=True, timeout=120)
                assert rejected.returncode != 0 and preserved.read_bytes() == changed
            for rejected_name in ["_void_call_bad_result", "_bool_loop_bad", "_bool_bad_leaf", "_bool_call_bad", "_bool_store_bad", "_blocked_header"]:
                bad = requests.request(worker, binary, "--reconstruct", *ranges[rejected_name],
                                       prototypes=prototypes)
                ReconstructionArtifact(bad)
                assert not any(n["payload"].get("artifact_kind") == "recovered-source" for n in bad["nodes"]), rejected_name
                headers = [node["payload"] for node in bad["nodes"]
                           if node["payload"].get("artifact_kind") == "recovered-header"]
                assert len(headers) == 1 and headers[0]["status"] == "declaration-only-uncompiled", rejected_name
                assert "ast_root" not in headers[0], "a declaration-only header must not claim a recovered body"
                rejected_header = root / "review-only.h"
                rejected_header.write_text(headers[0]["source"])
                for language in ["c", "c++"]:
                    subprocess.run(["xcrun", "clang", "-fsyntax-only", "-x", language + "-header",
                                    "-std=c++17" if language == "c++" else "-std=c17",
                                    "-Wall", "-Wextra", "-Werror", str(rejected_header)], check=True)
            indexed_names = {"bool_index_load", "bool_index_store", "bool_index_exchange"}
            memory_names = {"bool_load", "bool_store", "bool_exchange", "bool_volatile_load", "bool_volatile_store"} | indexed_names
            for name in ["empty", "setter", "conditional", "discard", "discard_scalar", "reuse_scalar", "void_call",
                         "guarded_void", "void_then_store", "bool_identity", "bool_not", "bool_and", "bool_compare", "bool_loop", "bool_call",
                         "bool_load", "bool_store", "bool_exchange", "bool_volatile_load", "bool_volatile_store",
                         "bool_index_load", "bool_index_store", "bool_index_exchange"]:
                artifact = requests.request(worker, binary, "--reconstruct", *ranges["_" + name],
                                            prototypes=prototypes)
                ReconstructionArtifact(artifact)
                if optimization == "-Os" and name == "bool_loop":
                    swift_contract = root / "swift-batch-contract"
                    module_cache = root / "swift-module-cache"
                    module_cache.mkdir()
                    swift_env = dict(os.environ, CLANG_MODULE_CACHE_PATH=str(module_cache))
                    subprocess.run(["xcrun", "swiftc", str(ROOT / "Shared/ReconstructionArtifact.swift"),
                                    str(ROOT / "Tests/reconstruction_artifact_contract.swift"),
                                    "-o", str(swift_contract)], check=True, env=swift_env, timeout=120)
                    artifact_file = root / "swift-artifact.json"
                    artifact_file.write_text(json.dumps(artifact))
                    subprocess.run([str(swift_contract), str(artifact_file), digest,
                                    artifact["function"]["start"][2:], artifact["function"]["end"][2:]],
                                   check=True, env=swift_env, timeout=30)
                    header_only = requests.request(worker, binary, "--reconstruct", *ranges["_blocked_header"],
                                                   prototypes=prototypes)
                    artifact_file.write_text(json.dumps(header_only))
                    subprocess.run([str(swift_contract), str(artifact_file), digest,
                                    header_only["function"]["start"][2:], header_only["function"]["end"][2:]],
                                   check=True, env=swift_env, timeout=30)
                sources = [n["payload"]["source"] for n in artifact["nodes"]
                           if n["payload"].get("artifact_kind") == "recovered-source"]
                assert len(sources) == 1, (name, optimization, [n["payload"] for n in artifact["nodes"]
                    if n["payload"].get("artifact_kind") == "recovered-source-rejection"])
                source = sources[0]
                if name == "bool_call":
                    proofs = [node["payload"] for node in artifact["nodes"]
                              if node["payload"].get("artifact_kind") == "callee-boolean-result"]
                    assert proofs and all(proof["canonical_boolean_proven"] for proof in proofs)
                    if optimization == "-Os":
                        changed_prototypes = root / "changed-prototypes.tsv"
                        original = prototypes.read_text()
                        changed = original.replace("_bool_leaf\tbool bool_leaf(",
                                                   "_bool_leaf\tuint8_t bool_leaf(")
                        assert changed != original
                        changed_prototypes.write_text(changed)
                        altered = requests.request(worker, binary, "--reconstruct", *ranges["_bool_call"],
                                                   prototypes=changed_prototypes)
                        ReconstructionArtifact(altered)
                        assert altered["scope"]["settings_digest"] != artifact["scope"]["settings_digest"]
                        assert not any(node["payload"].get("artifact_kind") == "recovered-source"
                                       for node in altered["nodes"])
                assert "sizeof(void)" not in source
                if name == "bool_loop":
                    assert sum(node["kind"] == "source-while" for node in artifact["ast"]["nodes"]) == 1
                if name.startswith("bool_") and name not in {"bool_store", "bool_volatile_store", "bool_index_store"}:
                    returns = [node for node in artifact["ast"]["nodes"] if node["kind"] == "source-return"]
                    assert returns and all(node["attributes"]["canonical_boolean_proven"] for node in returns)
                if name in {"bool_load", "bool_exchange", "bool_volatile_load", "bool_index_load", "bool_index_exchange"}:
                    assert any(node["payload"].get("artifact_kind") == "boolean-memory-precondition"
                               and node["epistemic"] == "assumption" for node in artifact["nodes"])
                if name not in {"discard_scalar", "reuse_scalar"} and not name.startswith("bool_"):
                    assert "return;" in source
                if name in {"discard", "discard_scalar", "reuse_scalar"}:
                    events = [node for node in artifact["ast"]["nodes"] if node["kind"] == "source-call-event"]
                    assert len(events) == 1, "discarded call must survive as an executable AST event"
                    assert source.count("= leaf();") == 1, source
                if name in {"void_call", "guarded_void", "void_then_store"}:
                    assert "\n    empty();\n" in source and "void hs_call_result_" not in source, source
                    assert sum(node["kind"] == "source-call-event" for node in artifact["ast"]["nodes"]) == 1
                    call = next(node for node in artifact["ast"]["nodes"] if node["kind"] == "source-call")
                    assert call["type"] == call["attributes"]["machine_type"] == "void"
                if name == "void_then_store":
                    assert source.rindex("empty();") < source.rindex("hs_store8("), source
                # Link the original and generated functions into one executable.
                headers = [n["payload"]["source"] for n in artifact["nodes"]
                           if n["payload"].get("artifact_kind") == "recovered-header"]
                assert len(headers) == 1, "supported function must retain its exact header"
                header = root / "recovered.h"
                header.write_text(headers[0])
                for dialect in ["c-header", "c++-header"]:
                    subprocess.run(["xcrun", "clang", "-x", dialect, "-fsyntax-only",
                                    "-Wall", "-Wextra", "-Werror", str(header)], check=True)
                generated = root / "generated.c"
                generated.write_text("#define " + name + " recovered_" + name
                                     + '\n#include "recovered.h"\n' + source)
                runner = root / "runner.c"
                if name in indexed_names:
                    is_load = name == "bool_index_load"
                    is_store = name == "bool_index_store"
                    result_type = "void" if is_store else "bool"
                    parameters = ("const " if is_load else "") + "volatile bool *,uint64_t" + ("" if is_load else ",bool")
                    arguments = "a.values,index" + ("" if is_load else ",next")
                    recovered_arguments = "b.values,index" + ("" if is_load else ",next")
                    call = (f"{name}({arguments}); recovered_{name}({recovered_arguments});" if is_store else
                            f"if({name}({arguments})!=recovered_{name}({recovered_arguments}))return 1;")
                    runner.write_text("#include <stdint.h>\n#include <stdbool.h>\n#include <string.h>\n"
                        f"{result_type} {name}({parameters}); {result_type} recovered_{name}({parameters});\n"
                        "typedef struct { uint8_t before; bool values[5]; uint8_t after; } Cell;\n"
                        "int main(void) { for(unsigned pattern=0;pattern<32;pattern++)"
                        "for(unsigned index=0;index<5;index++)for(unsigned next=0;next<2;next++){"
                        "Cell a={0xa5,{0},0x5a};for(unsigned j=0;j<5;j++)a.values[j]=(pattern>>j)&1;Cell b=a;"
                        + call + "if(memcmp(&a,&b,sizeof(a)))return 1;}return 0;}\n")
                    assert any(expression["kind"] == "source-pointer-offset" for expression in artifact["ast"]["nodes"]), name
                    # Each helper has one definition and exactly one access;
                    # the exchange must read before it writes, not merely
                    # converge to the same final memory in this fixture.
                    assert source.count("hs_vload_u1(") == (0 if is_store else 2), name
                    assert source.count("hs_vstore_u1(") == (0 if is_load else 2), name
                    if name == "bool_index_exchange":
                        assert source.rfind("hs_vload_u1(") < source.rfind("hs_vstore_u1("), name
                    accesses = [node["payload"] for node in artifact["nodes"]
                                if node["payload"].get("artifact_kind") in {"external-memory-read", "external-memory-write"}]
                    # A lone read stays inline; explicit access-event records
                    # are emitted only for stores or a mixed access schedule.
                    expected_accesses = 2 if name == "bool_index_exchange" else 0 if is_load else 1
                    assert len(accesses) == expected_accesses, (name, optimization, accesses)
                    assert all("volatile pointer" in access["partition"] and "instruction order" in access["ordering"]
                               for access in accesses), name
                elif name in memory_names:
                    is_load = name in {"bool_load", "bool_volatile_load"}
                    is_store = name in {"bool_store", "bool_volatile_store"}
                    result_type = "void" if is_store else "bool"
                    pointer = ("const " if is_load else "") + ("volatile " if "volatile" in name else "") + "bool *"
                    parameters = pointer + ("" if is_load else ",bool")
                    arguments = "&a.value" + ("" if is_load else ",next")
                    recovered_arguments = "&b.value" + ("" if is_load else ",next")
                    call = (f"{name}({arguments}); recovered_{name}({recovered_arguments});" if is_store else
                            f"if({name}({arguments})!=recovered_{name}({recovered_arguments}))return 1;")
                    runner.write_text("#include <stdint.h>\n#include <stdbool.h>\n#include <string.h>\n"
                        f"{result_type} {name}({parameters}); {result_type} recovered_{name}({parameters});\n"
                        "typedef struct { uint8_t before; bool value; uint8_t after; } Cell;\n"
                        "int main(void) { for(unsigned previous=0;previous<2;previous++)"
                        "for(unsigned next=0;next<2;next++){Cell a={0xa5,previous,0x5a},b=a;"
                        + call + "if(memcmp(&a,&b,sizeof(a)))return 1;}return 0;}\n")
                elif name == "bool_loop":
                    runner.write_text("#include <stdint.h>\n#include <stdbool.h>\n"
                        "bool bool_loop(uint64_t,bool); bool recovered_bool_loop(uint64_t,bool);\n"
                        "int main(void) { uint64_t counts[]={0,1,2,9,31};"
                        "for(unsigned i=0;i<5;i++)for(unsigned j=0;j<2;j++)"
                        "if(bool_loop(counts[i],j)!=recovered_bool_loop(counts[i],j))return 1;"
                        "return 0; }\n")
                elif name.startswith("bool_"):
                    parameter = "uint64_t" if name == "bool_compare" else "bool"
                    runner.write_text("#include <stdint.h>\n#include <stdbool.h>\n"
                        f"bool {name}({parameter},{parameter}); bool recovered_{name}({parameter},{parameter});\n"
                        "int main(void) { uint64_t values[]={0,1,UINT64_MAX,UINT64_C(0x8000000000000000)};\n"
                        f"for(unsigned i=0;i<{'4' if name == 'bool_compare' else '2'};i++)"
                        f"for(unsigned j=0;j<{'4' if name == 'bool_compare' else '2'};j++)"
                        f"if({name}(values[i],values[j])!=recovered_{name}(values[i],values[j]))return 1;"
                        "return 0; }\n")
                elif name in {"discard_scalar", "reuse_scalar"}:
                    runner.write_text("#include <stdint.h>\n"
                        f"uint64_t {name}(void); uint64_t recovered_{name}(void);\n"
                        f"int main(void) {{ return {name}() != recovered_{name}(); }}\n")
                elif name == "guarded_void":
                    runner.write_text("#include <stdint.h>\nvoid guarded_void(uint64_t); void recovered_guarded_void(uint64_t);\n"
                        "int main(void) { guarded_void(0); recovered_guarded_void(0);"
                        "guarded_void(1); recovered_guarded_void(1); return 0; }\n")
                elif name in {"empty", "discard", "void_call"}:
                    runner.write_text(f"void {name}(void); void recovered_{name}(void);\n"
                                      f"int main(void) {{ {name}(); recovered_{name}(); return 0; }}\n")
                else:
                    runner.write_text("#include <stdint.h>\n#include <string.h>\n"
                        f"void {name}(uint64_t *,uint64_t); void recovered_{name}(uint64_t *,uint64_t);\n"
                        "int main(void) { uint64_t values[]={0,1,2,UINT64_MAX,UINT64_C(0x8000000000000000)};\n"
                        "for(unsigned i=0;i<5;i++){ uint64_t a[]={17,23,31},b[]={17,23,31};\n"
                        f"{name}(&a[1],values[i]); recovered_{name}(&b[1],values[i]);\n"
                        "if(memcmp(a,b,sizeof(a)))return 1; } return 0; }\n")
                executable = root / "compare"
                subprocess.run(["xcrun", "clang", "-std=c17", "-Wall", "-Wextra", "-Werror",
                                "-fsanitize=undefined", "-fno-sanitize-recover=all",
                                str(generated), str(fixture), str(assembly), str(runner), "-o", str(executable)], check=True)
                subprocess.run([str(executable)], check=True, timeout=15)
                comparisons += 320 if name in indexed_names else 4 if name in memory_names else 10 if name == "bool_loop" else (16 if name == "bool_compare" else 4) if name.startswith("bool_") else 5 if name in {"setter", "conditional", "void_then_store"} else 2 if name == "guarded_void" else 1
        print(f"Void/call/Boolean reconstruction passed: 92 function/configuration pairs; {comparisons} native comparisons; 24 body-recovery rejections with retained C/C++ headers.")


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else ROOT / "build/rust/debug/reworker").resolve())
