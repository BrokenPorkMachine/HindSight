#!/usr/bin/env python3
"""Named source AST -> real C/ObjC compilation -> original result checks."""
import hashlib
import importlib.util
import json
import platform
import itertools
from pathlib import Path
import re
import random
import subprocess
import sys
import tempfile

from darwin_sdk_catalog import collect_declarations, verify_macho_target, worker_prototypes
from reconstruction_artifact import ReconstructionArtifact

ROOT=Path(__file__).resolve().parents[1]
def module(name,path):
    spec=importlib.util.spec_from_file_location(name,path);result=importlib.util.module_from_spec(spec);spec.loader.exec_module(result);return result
TEST=module("machine_test",ROOT/"scripts/test-machine-state.py")
request=TEST.REQUESTS.request

def run(worker):
    if sys.platform!="darwin" or platform.machine()!="arm64":
        print("Recovered source native qualification requires Apple Silicon macOS; skipped");return
    fixtures=ROOT/"Tests/Fixtures/Decompilation"
    names=["hs_c_mix","hs_c_select","hs_c_stack","hs_c_signed","hs_c_narrow","hs_c_u32","hs_c_nested","hs_m_signed_div","hs_m_word","hs_m_rotate","hs_m_pair","hs_m_keep","hs_m_flagwrite","hs_m_addflags","hs_m_andflags"]
    catalog=collect_declarations(names+["hs_m_loop","hs_m_memory"],fixtures/"machine_state.h")
    edge=[0,1,2,0x7f,0x80,0x7fffffff,0x80000000,0xffffffff,0x7fffffffffffffff,0x8000000000000000,0xffffffffffffffff]
    vectors=[[a,b,a^b,a*3&0xffffffffffffffff,b&63,0,0,0] for a in edge for b in edge]
    rng=random.Random(0x48535352)
    vectors += [[rng.getrandbits(64) for _ in range(8)] for _ in range(64)]
    stdin="".join(" ".join(f"{v:x}" for v in values)+"\n" for values in vectors)
    qualified=[];evidence_root=ROOT/"build/qualification";evidence_root.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="hindsight-recovered-source-") as directory:
        root=Path(directory);native=root/"native"
        TEST.execute(["xcrun","clang","-O2",fixtures/"machine_oracle.c","-lobjc","-o",native])
        for optimization,stripped in itertools.product(["-O0","-O1","-O2","-Os"],[False,True]):
            build_name=optimization+("-stripped" if stripped else "")
            binary=root/("original"+build_name+".dylib")
            TEST.execute(["xcrun","clang","-target","arm64-apple-macos14.0",optimization,"-fno-stack-protector","-Wl,-no_fixup_chains","-dynamiclib",fixtures/"machine_state.c",fixtures/"machine_state.m",fixtures/"machine_state.s","-lobjc","-o",binary])
            if stripped:TEST.execute(["xcrun","strip","-S","-x",binary])
            prototypes=root/"prototypes.tsv";prototypes.write_text(worker_prototypes(catalog,verify_macho_target(binary,"arm64-apple-macos14.0")))
            report=request(worker,binary,"--analyze")
            ranges={m[3].split("  source=",1)[0]:(m[1],m[2]) for line in report["functions"].splitlines() if (m:=re.match(r"^(0x[0-9a-f]+)\s+(0x[0-9a-f]+)\s+\d+\s+bytes\s+(.+)$",line))}
            objc=next(name for name in ranges if "HSMachineStateFixture mix:with:" in name)
            for name in names+["objc"]:
                start,end=ranges[objc if name=="objc" else "_"+name]
                document=request(worker,binary,"--reconstruct",start,end,prototypes=prototypes)
                ReconstructionArtifact(document)
                candidates=[n for n in document["nodes"] if n["payload"].get("artifact_kind")=="recovered-source"]
                assert len(candidates)==1,(name,optimization,[n["payload"] for n in document["nodes"] if n["payload"].get("artifact_kind")=="recovered-source-rejection"])
                generated=candidates[0];source=generated["payload"]["source"];public=generated["payload"]["name"]
                # Parameters come from the existing structured ABI adapter.
                source_root=next(n for n in document["ast"]["nodes"] if n["id"]==generated["payload"]["ast_root"])
                parameters=source_root["attributes"]["parameters"]
                # C05: the machine representation and the proven source type
                # are separate. Every AST node states its machine type, and a
                # node with a proven declaration states its semantic type too.
                typed=[n for n in document["ast"]["nodes"] if n["attributes"].get("source_type") not in (None,"unresolved")]
                assert typed,(name,optimization)
                for node in typed:
                    assert node["attributes"]["machine_type"]=="uint64_t",(name,optimization,node)
                    source_type=node["attributes"]["source_type"]
                    assert source_type["spelling"] and source_type["bits"] in (8,16,32,64),(name,optimization,node)
                # The parameter bindings carry the declared parameter type.
                bindings={n["attributes"]["source_type"]["spelling"] for n in typed if n["kind"]=="source-parameter"}
                declared={p["source_spelling"] for p in parameters}
                assert bindings<=declared,(name,optimization,bindings,declared)
                # C03: a recovered function publishes its proven frame
                # objects. The partition is exact, so two objects are the same
                # memory location only when their extents are equal.
                frame_objects=[n["payload"] for n in document["nodes"]
                               if n["payload"].get("artifact_kind")=="frame-object"]
                extents=[(o["offset"],o["bytes"]) for o in frame_objects]
                assert len(extents)==len(set(extents)),(name,optimization,extents)
                for frame_object in frame_objects:
                    assert frame_object["partition"]=="private-frame",(name,optimization,frame_object)
                    assert frame_object["bytes"] in (1,2,4,8,16),(name,optimization,frame_object)
                    assert int(frame_object["version_pc"],16)>=int(start,16),(name,optimization,frame_object)
                # A type never contradicts its declared width: a proven source
                # type states the same width the declaration proves.
                for parameter in parameters:
                    for node in typed:
                        source_type=node["attributes"]["source_type"]
                        if source_type["spelling"]==parameter["source_spelling"]:
                            assert source_type["bits"]==parameter["bits"],(name,optimization,node,parameter)
                            assert source_type["signed"]==parameter["signed"],(name,optimization,node,parameter)
                            assert source_type["pointer"]==parameter["pointer"],(name,optimization,node,parameter)
                assert not re.search(r"\bx(?:[0-9]|[12][0-9]|30)\b|s->|switch\(s->pc",source)
                # C07: a recovered local carries a deterministic role name
                # derived from the decoded operation, not an opaque counter.
                assert not re.search(r"\bhs_value_\d+\b",source),(name,optimization,source)
                roles=re.findall(r"\bhs_([a-z_]+)_\d+\b",source)
                known={"sum","difference","product","bit_and","bit_or","bit_xor","shifted",
                       "quotient","extended","extracted","negated","compared","selected",
                       "slot","condition","nzcv","narrowed","copy","path","result","value"}
                assert set(roles)<=known,(name,optimization,set(roles)-known)
                assert all(p["name"] in source for p in parameters)
                arguments=",".join(f"({p['source_spelling']})(uintptr_t)x[{p['abi_gpr']}]" for p in parameters)
                wrapper=f"""
#include <stdio.h>
#include <inttypes.h>
int main(void) {{
 uint64_t x[8];
 while(scanf("%"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64,&x[0],&x[1],&x[2],&x[3],&x[4],&x[5],&x[6],&x[7])==8)
   printf("%016"PRIx64" ",(uint64_t){public}({arguments})),puts("");
 return 0;
}}
"""
                cfile=root/("recovered.m" if name=="objc" else "recovered.c");executable=root/"recovered"
                cfile.write_text(source+wrapper)
                TEST.execute(["xcrun","clang","-std=c17","-O2","-Wall","-Wextra","-Werror","-fsanitize=undefined","-fno-sanitize-recover=all",cfile,"-o",executable])
                original=TEST.execute([native,binary,name],stdin).splitlines()
                recovered=TEST.execute([executable],stdin).splitlines()
                mask=(1<<source_root["attributes"]["result_bits"])-1
                assert len(original)==len(recovered)==len(vectors)
                assert [int(v.strip(),16)&mask for v in original]==[int(v.strip(),16)&mask for v in recovered],(name,optimization,next((i,a,b) for i,(a,b) in enumerate(zip(original,recovered)) if int(a.strip(),16)&mask!=int(b.strip(),16)&mask))
                repeated=request(worker,binary,"--reconstruct",start,end,prototypes=prototypes)
                assert document==repeated,(name,optimization,[key for key in document if document[key]!=repeated.get(key)])
                TEST.retain_validation(document,generated,{"artifact_kind":"recovered-source-qualification","toolchain":TEST.execute(["xcrun","clang","--version"]).splitlines()[0],"original_flags":[optimization,"-fno-stack-protector","-Wl,-no_fixup_chains","-target","arm64-apple-macos14.0"],"strip_flags":["-S","-x"] if stripped else [],"generated_flags":["-std=c17","-O2","-Wall","-Wextra","-Werror","-fsanitize=undefined","-fno-sanitize-recover=all"],"cases":len(vectors),"seed":"0x48535352","inputs_x0_x7":[[hex(v) for v in values] for values in vectors],"prototype_catalog_sha256":hashlib.sha256(prototypes.read_bytes()).hexdigest(),"source_sha256":hashlib.sha256(source.encode()).hexdigest(),"qualification_script_sha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),"comparison":"declared scalar result bits; not undefined high result bits, volatile-register/stack trace equivalence","formal_equivalence":False},evidence_root/f"recovered-source{build_name}-{name}.json")
                qualified.append({"name":name,"optimization":optimization,"stripped":stripped,"cases":len(vectors),"function_id":document["function"]["id"],"scope":document["scope"]})
            for name in ["hs_m_loop","hs_m_memory"]:
                start,end=ranges["_"+name]
                document=request(worker,binary,"--reconstruct",start,end,prototypes=prototypes)
                assert not any(n["payload"].get("artifact_kind")=="recovered-source" for n in document["nodes"])
                assert any(n["payload"].get("artifact_kind")=="recovered-source-rejection" for n in document["nodes"])
            start,end=ranges["_hs_c_mix"]
            without=request(worker,binary,"--reconstruct",start,end)
            assert not any(n["payload"].get("artifact_kind")=="recovered-source" for n in without["nodes"])
            assert without["scope"]!=request(worker,binary,"--reconstruct",start,end,prototypes=prototypes)["scope"]
            original_catalog=prototypes.read_text()
            prototypes.write_text(original_catalog+"\n_hs_c_mix\tint hs_c_mix(int conflict);\n")
            assert request(worker,binary,"--reconstruct",start,end,prototypes=prototypes)["kind"]=="error"
            prototypes.write_text(original_catalog.replace(hashlib.sha256(binary.read_bytes()).hexdigest(),"0"*64))
            assert request(worker,binary,"--reconstruct",start,end,prototypes=prototypes)["kind"]=="error"
        report={"qualified":qualified,"native_comparisons":sum(v["cases"] for v in qualified),"claims":"bounded scalar-result/ABI qualification; no original-source or formal-equivalence claim"}
        (evidence_root/"recovered-source.json").write_text(json.dumps(report,indent=2)+"\n")
        print(json.dumps({"functions":len(qualified),"native_comparisons":report["native_comparisons"]}))

if __name__=="__main__":run(Path(sys.argv[1] if len(sys.argv)>1 else ROOT/"rust/target/debug/reworker").resolve())
