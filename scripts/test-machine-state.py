#!/usr/bin/env python3
"""Native / shared-tree interpreter / strict C differential qualification.

Only the CC0 test-owned fixtures are executed natively. Reconstruction sees
captured Mach-O bytes/metadata, never fixture source or native output.
"""
import hashlib
import importlib.util
import json
import platform
from pathlib import Path
import random
import re
import subprocess
import sys
import tempfile

from reconstruction_artifact import (AnalysisScope, ReconstructionArtifact, content_id,
                                     reconstruction_digest)

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("worker_artifact_test", ROOT / "scripts/test-worker-reconstruction.py")
REQUESTS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REQUESTS)

RUNNER = r'''
#include <stdio.h>
#include <inttypes.h>
int main(void) {
    uint64_t x[8];
    while(scanf("%"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64,
       &x[0],&x[1],&x[2],&x[3],&x[4],&x[5],&x[6],&x[7])==8) {
        hs_state s={0};s.sp=HS_MEM_BASE+HS_MEM_SIZE;
        for(unsigned i=0;i<8;i++)s.x[i]=x[i];
        int status=hs_run(&s,4096);
        printf("%d %"PRIu64" %x %"PRIx64" %"PRIx64,status,s.steps,s.nzcv,s.sp,s.pc);
        for(unsigned i=0;i<31;i++)printf(" %"PRIx64,s.x[i]);
        printf(" ");for(unsigned i=0;i<HS_MEM_SIZE;i++)printf("%02x",s.memory[i]);
        printf("\n");
    }return 0;
}
'''


def payload(document, kind):
    found = [n for n in document["nodes"] if n["payload"].get("artifact_kind") == kind]
    assert len(found) == 1, (kind, [n["payload"] for n in document["nodes"] if "artifact_kind" in n["payload"]])
    return found[0]


def execute(command, stdin=None):
    result=subprocess.run(list(map(str, command)), input=stdin, capture_output=True, text=True, timeout=90)
    assert result.returncode==0, (command, result.stderr)
    return result.stdout


def retain_validation(document, generated, evidence, destination):
    """Bind actual observations to this candidate, without promoting review AST."""
    source=document["source_sha256"];function=document["function"]["id"]
    scope=AnalysisScope(**document["scope"])
    node={"kind":"validation-run","epistemic":"fact","producer":"machine-state-qualification-v1",
          "confidence":1,"ranges":[],"payload":evidence}
    node["id"]=content_id("evidence",source,function,node,scope)
    document["nodes"].append(node)
    edge={"source":generated["id"],"target":node["id"],"relation":"validates","producer":"machine-state-qualification-v1"}
    edge["id"]=content_id("edge",source,function,edge,scope)
    document["edges"].append(edge)
    digest=reconstruction_digest(document)
    for kind in ["recompile","differential"]:
        run={"kind":kind,"status":"passed","artifact_sha256":digest,
             "assumptions":[f"Validation targets {generated['payload']['artifact_kind']} metadata, NOT review rendering or original-source fidelity.",
                            "Known fixture uint64_t result ABI; single-threaded EL0; bounded-flat-memory-v1; no precise exception-state claim.",
                            "Compiler matrix and tested inputs are retained in validation-run evidence."],"mismatches":[]}
        run["id"]=content_id("validation",source,function,run,scope)
        document["validation"].append(run)
    checked=ReconstructionArtifact(document)
    assert any(n["kind"]=="bytes" for n in checked.why(node["id"])["nodes"])
    destination.write_text(json.dumps(document,indent=2)+"\n")


def compiled_state(line):
    fields=line.split()
    return {"status":int(fields[0]),"steps":int(fields[1]),"nzcv":int(fields[2],16),
            "sp":hex(int(fields[3],16)),"pc":hex(int(fields[4],16)),
            "registers":[hex(int(v,16)) for v in fields[5:36]],"memory_hex":fields[36]}


def run(worker):
    if sys.platform != "darwin" or platform.machine() != "arm64":
        print("Machine-state hardware qualification requires Apple Silicon macOS; skipped")
        return
    fixture = ROOT / "Tests/Fixtures/Decompilation"
    edges = [0, 1, 2, 0x7f, 0x80, 0x7fffffff, 0x80000000, 0xffffffff,
             0x7fffffffffffffff, 0x8000000000000000, 0xffffffffffffffff]
    rng = random.Random(0x48534d53)
    vectors = [[a, b, *[rng.getrandbits(64) for _ in range(6)]] for a in edges for b in edges]
    vectors += [[rng.getrandbits(64) for _ in range(8)] for _ in range(24)]
    qualified = []
    evidence_root=ROOT/"build/qualification"
    evidence_root.mkdir(parents=True,exist_ok=True)
    toolchain=execute(["xcrun","clang","--version"]).splitlines()[0]
    with tempfile.TemporaryDirectory(prefix="hindsight-machine-state-") as directory:
        root = Path(directory)
        native = root / "native"
        execute(["xcrun", "clang", "-target", "arm64-apple-macos14.0", "-O2", fixture / "machine_oracle.c", "-lobjc", "-o", native])
        for optimization in ["-O0", "-O2"]:
            binary = root / ("fixture"+optimization+".dylib")
            execute(["xcrun", "clang", "-target", "arm64-apple-macos14.0", optimization, "-fno-stack-protector", "-dynamiclib",
                     fixture / "machine_state.c", fixture / "machine_state.m", fixture / "machine_state.s", "-lobjc", "-o", binary])
            report = REQUESTS.request(worker, binary, "--analyze")
            assert report.get("kind") == "analysis_result", report
            ranges = {m[3].split("  source=",1)[0]: (m[1], m[2]) for line in report["functions"].splitlines()
                      if (m := re.match(r"^(0x[0-9a-f]+)\s+(0x[0-9a-f]+)\s+\d+\s+bytes\s+(.+)$", line))}
            objc_name = next(name for name in ranges if "HSMachineStateFixture mix:with:" in name)
            names = ["hs_c_mix", "hs_c_select", "hs_c_stack", "hs_m_signed_div", "hs_m_word", "hs_m_rotate", "hs_m_loop", "hs_m_memory", "hs_m_alias", "hs_m_pair", "hs_m_keep", "hs_m_flagwrite", "hs_m_addflags", "hs_m_addcarry", "hs_m_subcarry", "hs_m_nzcv_roundtrip", "hs_m_andflags", "objc"]
            for name in names:
                key=objc_name if name == "objc" else "_"+name
                assert key in ranges, (key, sorted(ranges))
                start, end = ranges[key]
                document = REQUESTS.request(worker, binary, "--reconstruct", start, end)
                validator = ReconstructionArtifact(document)
                assert not any(n["payload"].get("artifact_kind")=="machine-state-c-rejection" for n in document["nodes"]), (name,optimization,
                    [n["payload"] for n in document["nodes"] if n["kind"]=="unsupported-effect"])
                generated = payload(document, "machine-state-c")
                assert any(n["kind"] == "bytes" for n in validator.why(generated["id"])["nodes"])
                source = generated["payload"]["source"]
                cfile, executable = root / "generated.c", root / "generated"
                cfile.write_text(source+RUNNER)
                execute(["xcrun", "clang", "-std=c17", "-O2", "-Wall", "-Wextra", "-Werror",
                         "-fsanitize=undefined", "-fno-sanitize-recover=all", cfile, "-o", executable])
                inputs = [values[:] for values in vectors]
                if name in ["hs_m_memory","hs_m_alias"]:
                    for i, values in enumerate(inputs):
                        values[0] = 0x100000 + i % 4081
                stdin = "".join(" ".join(f"{v:x}" for v in values)+"\n" for values in inputs)
                actual = execute([native, binary, name], stdin).splitlines()
                compiled = execute([executable], stdin).splitlines()
                assert len(actual) == len(compiled) == len(inputs)
                for values, native_line, compiled_line in zip(inputs, actual, compiled):
                    artifact = REQUESTS.request(worker, binary, "--interpret", start, end, ",".join(hex(v) for v in values))
                    assert artifact.get("producer") == "hindsight-reconstruction-v1", artifact
                    # First and last runs also independently validate every ID.
                    if values is inputs[0] or values is inputs[-1]:
                        ReconstructionArtifact(artifact)
                    state = payload(artifact, "machine-state-run")["payload"]["result"]
                    fields = compiled_line.split()
                    assert int(fields[0]) == state["status"] == 0, (name, optimization, values, state)
                    assert int(fields[1]) == state["steps"]
                    assert int(fields[2], 16) == state["nzcv"]
                    assert int(fields[3], 16) == int(state["sp"], 16)
                    assert int(fields[4], 16) == int(state["pc"], 16)
                    assert [int(v, 16) for v in fields[5:36]] == [int(v, 16) for v in state["registers"]]
                    assert fields[36] == state["memory_hex"]
                    native_fields = native_line.split()
                    assert int(native_fields[0], 16) == int(state["registers"][0], 16), (name, optimization, values, native_fields[0], state)
                    if name in ["hs_m_memory","hs_m_alias"]:
                        assert native_fields[1] == state["memory_hex"]
                qualified.append({"function":name,"optimization":optimization,"cases":len(inputs),
                                  "source_sha256":document["source_sha256"],"function_id":document["function"]["id"],
                                  "scope":document["scope"],"generated_sha256":hashlib.sha256(source.encode()).hexdigest()})
                retain_validation(document,generated,{
                    "artifact_kind":"machine-state-qualification","toolchain":toolchain,
                    "platform":platform.platform(),"seed":"0x48534d53","cases":len(inputs),
                    "inputs_sha256":hashlib.sha256(stdin.encode()).hexdigest(),
                    "inputs_x0_x7":[[hex(v) for v in values] for values in inputs],
                    "qualification_script_sha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                    "generated_source_sha256":hashlib.sha256(source.encode()).hexdigest(),
                    "generated_executable_sha256":hashlib.sha256(executable.read_bytes()).hexdigest(),
                    "native_output_sha256":hashlib.sha256("\n".join(actual).encode()).hexdigest(),
                    "compiled_output_sha256":hashlib.sha256("\n".join(compiled).encode()).hexdigest(),
                    "original_flags":[optimization,"-fno-stack-protector","-target","arm64-apple-macos14.0"],
                    "generated_flags":["-std=c17","-O2","-Wall","-Wextra","-Werror","-fsanitize=undefined","-fno-sanitize-recover=all"],
                    "comparison":"native x0 and explicit external buffers; compiled/interpreted all GPRs, SP, PC, NZCV, steps and 4096 memory bytes",
                    "native_exception_testing":False,"formal_equivalence":False,
                },evidence_root/f"machine-state{optimization}-{name}.json")
            for name in ["hs_m_tail", "hs_m_opaque", "hs_m_call", "hs_m_unpredictable"]:
                start,end=ranges["_"+name]
                document=REQUESTS.request(worker,binary,"--reconstruct",start,end)
                payload(document,"machine-state-c-rejection")
                assert REQUESTS.request(worker,binary,"--interpret",start,end,",".join(["0x0"]*8))["kind"]=="error"
            start,end=ranges["_hs_m_spin"]
            document=REQUESTS.request(worker,binary,"--interpret",start,end,",".join(["0x0"]*8))
            result=payload(document,"machine-state-run")["payload"]["result"]
            assert result["status"]==2 and result["steps"]==4096
            cfile.write_text(payload(document,"machine-state-c")["payload"]["source"]+RUNNER)
            execute(["xcrun","clang","-std=c17","-O2","-fsanitize=undefined","-fno-sanitize-recover=all",cfile,"-o",executable])
            assert compiled_state(execute([executable],"0 0 0 0 0 0 0 0\n").strip())==result
            start,end=ranges["_hs_m_memory"]
            document=REQUESTS.request(worker,binary,"--reconstruct",start,end)
            cfile.write_text(payload(document,"machine-state-c")["payload"]["source"]+RUNNER)
            execute(["xcrun","clang","-std=c17","-O2","-fsanitize=undefined","-fno-sanitize-recover=all",cfile,"-o",executable])
            for address in [0,0xffffffffffffffff,0x100ff9]:
                inputs=[address,1,0,0,0,0,0,0]
                document=REQUESTS.request(worker,binary,"--interpret",start,end,",".join(map(hex,inputs)))
                result=payload(document,"machine-state-run")["payload"]["result"]
                assert result["status"]==3 and result["memory_hex"]=="00"*4096
                assert compiled_state(execute([executable]," ".join(f"{v:x}" for v in inputs)+"\n").strip())==result
            assert REQUESTS.request(worker,binary,"--interpret",start,end,"0x0")["kind"]=="error"
        protected=root/"protected.dylib"
        execute(["xcrun","clang","-target","arm64-apple-macos14.0","-O0","-fstack-protector-all","-dynamiclib",fixture/"machine_state.c",fixture/"machine_state.s","-o",protected])
        protected_report=REQUESTS.request(worker,protected,"--analyze")
        row=next(line for line in protected_report["functions"].splitlines() if "_hs_c_stack " in line)
        start,end=row.split()[:2]
        rejected=REQUESTS.request(worker,protected,"--reconstruct",start,end)
        payload(rejected,"machine-state-c-rejection")
        assert any(n["payload"].get("reason")=="unresolved-call-contract" for n in rejected["nodes"])
        report={"contract":"aarch64-gpr-nzcv-flat4096-le-v1","toolchain":toolchain,
                "seed":"0x48534d53","original_flags":["-target","arm64-apple-macos14.0","-fno-stack-protector","-dynamiclib"],
                "generated_flags":["-std=c17","-O2","-Wall","-Wextra","-Werror","-fsanitize=undefined","-fno-sanitize-recover=all"],
                "qualified":qualified,"three_way_comparisons":sum(row["cases"] for row in qualified),
                "qualification_script_sha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                "claims":"bounded tested machine-state behavior; not recovered-source fidelity, precise hardware fault state, or formal equivalence"}
        destination=ROOT/"build/qualification/machine-state.json"
        destination.parent.mkdir(parents=True,exist_ok=True)
        destination.write_text(json.dumps(report,indent=2)+"\n")
        print(json.dumps({"functions":len(qualified),"comparisons":report["three_way_comparisons"],"evidence":str(destination)}))


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv)>1 else ROOT/"rust/target/debug/reworker").resolve())
