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
    names = ["hs_c_mix", "hs_c_select", "hs_c_stack", "hs_c_signed", "hs_c_narrow", "hs_c_u32", "hs_c_nested", "hs_c_load", "hs_c_load_volatile_u32", "hs_c_load_volatile_twice", "hs_c_load_volatile_branch", "hs_c_volatile_load_then_call", "hs_c_call_then_volatile_load", "hs_c_volatile_store_call_load", "hs_c_loop_volatile_store", "hs_c_loop_volatile_accumulate", "hs_c_loop_two_volatile_pointers", "hs_c_load_signed16", "hs_c_load_signed32", "hs_c_load_signed64", "hs_c_load_unsigned8", "hs_c_load_unsigned16", "hs_c_load_signed_offset", "hs_c_load_word_offset", "hs_c_load_dynamic", "hs_c_load_dynamic_s16", "hs_c_load_dynamic_u32", "hs_c_store", "hs_c_store_volatile", "hs_c_store_volatile_twice", "hs_c_store_u16", "hs_c_store_u32", "hs_c_store_s16", "hs_c_store_s32", "hs_c_store_twice", "hs_c_store_conditional", "hs_c_store_dynamic", "hs_c_store_dynamic_u32", "hs_c_store_dynamic_s16", "hs_c_store_then_load", "hs_c_load_then_store", "hs_c_conditional_store_then_load", "hs_c_alternate_store_then_load", "hs_c_recursive", "hs_c_recursive_pair", "hs_c_recursive_store", "hs_c_call_helper", "hs_c_call_twice", "hs_c_volatile_store_then_call", "hs_c_call_then_volatile_store", "hs_c_store_then_call", "hs_c_store_load_then_call", "hs_c_branch_store_call", "hs_c_call_then_store", "hs_c_call_then_store_branch", "hs_c_conditional_call_store", "hs_c_call_then_load", "hs_c_call_load_store", "hs_m_signed_div", "hs_m_loop", "hs_m_word", "hs_m_rotate", "hs_m_pair", "hs_m_keep", "hs_m_flagwrite", "hs_m_addflags", "hs_m_andflags"]
    catalog=collect_declarations(names+["hs_c_leaf_helper","hs_m_memory"],fixtures/"machine_state.h")
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
                function_stdin=stdin
                if name=="hs_c_recursive_store":
                    function_stdin="".join(" ".join(f"{v:x}" for v in [row[0],row[1]%16,*row[2:]])+"\n" for row in vectors)
                if name in {"hs_c_load_dynamic","hs_c_load_dynamic_s16","hs_c_load_dynamic_u32"}:
                    dynamic_vectors=[[row[0],row[1]%4,*row[2:]] for row in vectors]
                    function_stdin="".join(" ".join(f"{v:x}" for v in values)+"\n" for values in dynamic_vectors)
                if name=="hs_c_store_dynamic":
                    dynamic_vectors=[[row[0],row[1]%16,*row[2:]] for row in vectors]
                    function_stdin="".join(" ".join(f"{v:x}" for v in values)+"\n" for values in dynamic_vectors)
                if name=="hs_c_store_dynamic_u32":
                    dynamic_vectors=[[row[0],row[1]%4,*row[2:]] for row in vectors]
                    function_stdin="".join(" ".join(f"{v:x}" for v in values)+"\n" for values in dynamic_vectors)
                if name=="hs_c_store_dynamic_s16":
                    dynamic_vectors=[[row[0],row[1]%4,*row[2:]] for row in vectors]
                    function_stdin="".join(" ".join(f"{v:x}" for v in values)+"\n" for values in dynamic_vectors)
                try:
                    document=request(worker,binary,"--reconstruct",start,end,prototypes=prototypes)
                except subprocess.CalledProcessError as error:
                    raise AssertionError((name,optimization,stripped,error.stderr.decode(errors="replace"))) from error
                try: ReconstructionArtifact(document)
                except Exception as error: raise AssertionError((name,optimization,str(error))) from error
                class_layouts=[node["payload"] for node in document["nodes"]
                               if node["kind"]=="type" and node["payload"].get("kind")=="objc-class-layout"]
                if name=="objc":
                    assert any(layout["class_name"]=="HSMachineStateFixture" for layout in class_layouts),class_layouts
                else:
                    assert not class_layouts,(name,optimization,class_layouts)
                candidates=[n for n in document["nodes"] if n["payload"].get("artifact_kind")=="recovered-source"]
                assert len(candidates)==1,(name,optimization,[n["payload"] for n in document["nodes"] if n["payload"].get("artifact_kind")=="recovered-source-rejection"])
                generated=candidates[0];source=generated["payload"]["source"];public=generated["payload"]["name"]
                # C03/C07: source memory expressions must cite the same
                # instruction-scoped memory SSA records that represent their
                # reaching read/write versions.
                ssa_by_id={node["id"]:node for node in document["nodes"]}
                memory_record_ids={node["id"] for node in document["nodes"]
                                   if node["kind"] in {"memory-use","memory-definition"}}
                access_asts=[node for node in document["ast"]["nodes"]
                             if node["kind"] in {"source-memory-read","source-memory-store"}]
                for access in access_asts:
                    cited=set(access["evidence"]) & memory_record_ids
                    assert cited,(name,optimization,access["kind"],access["ranges"],access["evidence"])
                    expected="memory-use" if access["kind"]=="source-memory-read" else "memory-definition"
                    assert any(ssa_by_id[record]["kind"]==expected for record in cited),(
                        name,optimization,access["kind"],[(record,ssa_by_id[record]["kind"]) for record in cited])
                store_asts=[node for node in document["ast"]["nodes"] if node["kind"]=="source-memory-store"]
                store_records=[node["payload"] for node in document["nodes"] if node["payload"].get("artifact_kind")=="external-memory-write"]
                if name in {"hs_c_store","hs_c_store_volatile","hs_c_store_volatile_twice","hs_c_store_u16","hs_c_store_u32","hs_c_store_s16","hs_c_store_s32","hs_c_store_twice","hs_c_store_conditional","hs_c_store_dynamic","hs_c_store_dynamic_u32","hs_c_store_dynamic_s16","hs_c_store_then_load","hs_c_load_then_store","hs_c_conditional_store_then_load","hs_c_alternate_store_then_load","hs_c_store_then_call","hs_c_volatile_store_then_call","hs_c_call_then_volatile_store","hs_c_volatile_store_call_load","hs_c_loop_volatile_store","hs_c_loop_volatile_accumulate","hs_c_loop_two_volatile_pointers","hs_c_store_load_then_call","hs_c_branch_store_call","hs_c_call_then_store","hs_c_call_then_store_branch","hs_c_recursive_store","hs_c_conditional_call_store","hs_c_volatile_store_call_load","hs_c_call_load_store"}:
                    expected_stores=2 if name in {"hs_c_store_twice","hs_c_store_volatile_twice","hs_c_loop_two_volatile_pointers","hs_c_call_then_store_branch","hs_c_recursive_store"} else (1,2) if name in {"hs_c_alternate_store_then_load","hs_c_conditional_call_store"} else 1
                    if isinstance(expected_stores,tuple):
                        assert len(store_asts)==len(store_records) and expected_stores[0]<=len(store_asts)<=expected_stores[1],(name,optimization,store_asts,store_records)
                    else:
                        assert len(store_asts)==len(store_records)==expected_stores,(name,optimization,store_asts,store_records)
                    expected_bytes=1 if name=="hs_c_store_dynamic" else 2 if name in {"hs_c_store_u16","hs_c_store_s16","hs_c_store_dynamic_s16"} else 4 if name in {"hs_c_store_u32","hs_c_store_s32","hs_c_store_dynamic_u32"} else 8
                    assert all(node["attributes"]["bytes"]==expected_bytes for node in store_asts)
                    assert (f"hs_vstore_u{expected_bytes}(" if name in {"hs_c_store_volatile","hs_c_store_volatile_twice","hs_c_volatile_store_call_load","hs_c_loop_volatile_accumulate","hs_c_loop_two_volatile_pointers"} else f"hs_store{expected_bytes}(") in source
                else:
                    assert not store_asts and not store_records,(name,optimization,store_asts,store_records)
                load_asts=[node for node in document["ast"]["nodes"] if node["kind"]=="source-memory-load"]
                read_records=[node["payload"] for node in document["nodes"] if node["payload"].get("artifact_kind")=="external-memory-read"]
                expected_load_types={"hs_c_load_volatile_u32":"uint32_t","hs_c_load_volatile_twice":"uint64_t","hs_c_load_volatile_branch":"uint64_t","hs_c_volatile_load_then_call":"uint64_t","hs_c_call_then_volatile_load":"uint64_t","hs_c_volatile_store_call_load":"uint64_t","hs_c_loop_volatile_accumulate":"uint64_t","hs_c_loop_two_volatile_pointers":"uint64_t","hs_c_load_signed16":"int16_t","hs_c_load_signed32":"int32_t","hs_c_load_signed64":"int64_t","hs_c_load_unsigned8":"uint8_t","hs_c_load_unsigned16":"uint16_t","hs_c_load_signed_offset":"int8_t","hs_c_load_dynamic_s16":"int16_t","hs_c_load_dynamic_u32":"uint32_t"}
                if name in expected_load_types:
                    assert len(load_asts)==(2 if name in {"hs_c_load_volatile_twice","hs_c_load_volatile_branch","hs_c_loop_two_volatile_pointers"} else 1),(name,optimization,load_asts)
                    load_type=load_asts[0]["attributes"]["source_type"]
                    expected_signed=expected_load_types[name].startswith("int")
                    assert load_type["spelling"]==expected_load_types[name] and load_type["signed"] is expected_signed,(name,optimization,load_type)
                    if name=="hs_c_load_volatile_u32":assert "hs_vload_u4(" in source,(name,optimization,source)
                    if name in {"hs_c_load_volatile_twice","hs_c_load_volatile_branch"}:assert source.count("hs_vload_u8(")==3,(name,optimization,source)
                    if name in {"hs_c_volatile_load_then_call","hs_c_call_then_volatile_load","hs_c_volatile_store_call_load","hs_c_loop_volatile_accumulate"}:assert source.count("hs_vload_u8(")==2,(name,optimization,source)
                    if name=="hs_c_loop_two_volatile_pointers":assert source.count("hs_vload_u8(")==3,(name,optimization,source)
                    promoted=[node["attributes"]["source_type"] for node in document["ast"]["nodes"] if node["kind"]=="source-memory-value"]
                    if load_type["bits"]<32:
                        assert any(value["spelling"]=="int32_t" and value["signed"] is True for value in promoted),(name,optimization,promoted)
                if name in {"hs_c_store_then_load","hs_c_load_then_store","hs_c_conditional_store_then_load","hs_c_alternate_store_then_load","hs_c_store_load_then_call","hs_c_call_then_load","hs_c_call_load_store","hs_c_load_volatile_twice","hs_c_load_volatile_branch","hs_c_volatile_load_then_call","hs_c_call_then_volatile_load","hs_c_volatile_store_call_load","hs_c_loop_volatile_accumulate","hs_c_loop_two_volatile_pointers"}:
                    assert len(load_asts)>=1 and len(read_records)>=1,(name,optimization,load_asts,read_records)
                    assert all("instruction order" in record["ordering"] and "guard_ast" in record for record in read_records),(name,optimization,read_records)
                else:
                    assert not read_records,(name,optimization,read_records)
                # Parameters come from the existing structured ABI adapter.
                source_root=next(n for n in document["ast"]["nodes"] if n["id"]==generated["payload"]["ast_root"])
                parameters=source_root["attributes"]["parameters"]
                if name in {"hs_c_store_u16","hs_c_store_u32","hs_c_store_s16","hs_c_store_s32","hs_c_store_dynamic_u32","hs_c_store_dynamic_s16"}:
                    ast_by_id={node["id"]:node for node in document["ast"]["nodes"]}
                    expected_signed=name in {"hs_c_store_s16","hs_c_store_s32","hs_c_store_dynamic_s16"}
                    for store_node in store_asts:
                        stored_type=ast_by_id[store_node["children"][1]]["attributes"]["source_type"]
                        assert stored_type["bits"]==store_node["attributes"]["bytes"]*8 and stored_type["signed"] is expected_signed and not stored_type["pointer"],(name,optimization,stored_type)
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
                frame_object_nodes={node["id"] for node in document["nodes"]
                                    if node["payload"].get("artifact_kind")=="frame-object"}
                frame_version_links=[edge for edge in document["edges"]
                                     if edge["relation"]=="backs-frame-object-version"
                                     and edge["target"] in frame_object_nodes]
                assert {edge["target"] for edge in frame_version_links}==frame_object_nodes,(name,optimization,frame_objects,frame_version_links)
                assert all(ssa_by_id[edge["source"]]["kind"]=="memory-definition"
                           for edge in frame_version_links),(name,optimization,frame_version_links)
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
                known={"sum","difference","product","bit_and","bit_or","bit_xor","shifted","argument",
                       "quotient","extended","extracted","negated","compared","selected",
                       "slot","condition","nzcv","narrowed","copy","path","result","value","address","loaded","argument","induction","accumulator","call_result"}
                assert set(roles)<=known,(name,optimization,set(roles)-known)
                assert all(p["name"] in source for p in parameters)
                if name=="hs_c_recursive":
                    assert "hs_c_recursive(" in source,(optimization,source)
                    assert any(node["kind"]=="source-call" for node in document["ast"]["nodes"]),(optimization,document["ast"]["nodes"])
                    coverage=next(node["payload"] for node in document["nodes"] if node["payload"].get("artifact_kind")=="semantic-coverage")
                    assert coverage["strict_machine_state_blocked"] is True,(optimization,coverage)
                if name=="hs_c_recursive_pair":
                    assert source.count("hs_c_recursive_pair(")>=2,(optimization,source)
                    assert sum(node["kind"]=="source-call" for node in document["ast"]["nodes"])>=2,(optimization,document["ast"]["nodes"])
                if name=="hs_m_loop":
                    assert source.count("while (")==2,(optimization,source)
                    assert len([node for node in document["ast"]["nodes"] if node["kind"]=="source-while"])==2
                    assert len([node for node in document["ast"]["nodes"] if node["kind"]=="source-loop-assignment"])==5
                if name in {"hs_c_loop_volatile_store","hs_c_loop_volatile_accumulate"}:
                    assert source.count("while (")==1 and "hs_vstore_u8(" in source
                if name=="hs_c_loop_two_volatile_pointers":
                    assert source.count("while (")==1,(optimization,source)
                    loop_body=source[source.index("while ("):]
                    assert loop_body.count("hs_vload_u8(")==2 and loop_body.count("hs_vstore_u8(")==2,(optimization,source)
                    positions=[loop_body.index("hs_vload_u8("),loop_body.index("hs_vload_u8(",loop_body.index("hs_vload_u8(")+1),loop_body.index("hs_vstore_u8("),loop_body.index("hs_vstore_u8(",loop_body.index("hs_vstore_u8(")+1)]
                    assert positions==sorted(positions),(optimization,source)
                    assert len([node for node in document["ast"]["nodes"] if node["kind"]=="source-while"])==1
                    assert len([node for node in document["ast"]["nodes"] if node["kind"]=="source-memory-store"])==2
                if name=="hs_c_call_helper":
                    assert "hs_c_leaf_helper(" in source,(optimization,source)
                    assert any(node["kind"]=="source-call" for node in document["ast"]["nodes"]),(optimization,document["ast"]["nodes"])
                    call_node=next(node for node in document["ast"]["nodes"] if node["kind"]=="call")
                    summary=call_node["attributes"]["call_effects"]["callee_summary"]
                    assert summary["complete"] and summary["observed_return"] and not summary["calls"] and not summary["observed_memory_read"] and not summary["observed_memory_write"],(optimization,summary)
                    coverage=next(node["payload"] for node in document["nodes"] if node["payload"].get("artifact_kind")=="semantic-coverage")
                    assert coverage["strict_machine_state_blocked"] is True,(optimization,coverage)
                if name in {"hs_c_volatile_store_then_call","hs_c_call_then_volatile_store"}:
                    assert "hs_vstore_u8(" in source and "hs_c_leaf_helper(" in source,(optimization,source)
                    call_pos=source.rfind("hs_c_leaf_helper("); store_pos=source.rfind("hs_vstore_u8(")
                    if name=="hs_c_volatile_store_then_call": assert store_pos < call_pos,(optimization,source)
                    else: assert call_pos < store_pos,(optimization,source)
                    assert any(node["kind"]=="source-call-event" for node in document["ast"]["nodes"])
                if name in {"hs_c_volatile_load_then_call","hs_c_call_then_volatile_load"}:
                    assert "hs_c_leaf_helper(" in source and "hs_vload_u8(" in source,(optimization,source)
                    call_pos=source.rfind("hs_c_leaf_helper("); load_pos=source.rfind("hs_vload_u8(")
                    if name=="hs_c_volatile_load_then_call": assert load_pos < call_pos,(optimization,source)
                    else: assert call_pos < load_pos,(optimization,source)
                    assert any(node["kind"]=="source-call-event" for node in document["ast"]["nodes"])
                if name=="hs_c_volatile_store_call_load":
                    assert "hs_c_leaf_helper(" in source and "hs_vstore_u8(" in source and "hs_vload_u8(" in source,(optimization,source)
                    store_pos=source.rfind("hs_vstore_u8("); call_pos=source.rfind("hs_c_leaf_helper("); load_pos=source.rfind("hs_vload_u8(")
                    assert store_pos < call_pos < load_pos,(optimization,source)
                    assert len([node for node in document["ast"]["nodes"] if node["kind"]=="source-memory-store"])==1
                    assert len([node for node in document["ast"]["nodes"] if node["kind"]=="source-memory-load"])==1
                    assert len([node for node in document["ast"]["nodes"] if node["kind"]=="source-call-event"])==1
                if name=="hs_c_call_twice":
                    assert source.count("hs_c_leaf_helper(")==3,(optimization,source)
                    assert sum(node["kind"]=="source-call" for node in document["ast"]["nodes"])==2
                if name in {"hs_c_store_then_call","hs_c_branch_store_call"}:
                    assert "hs_c_leaf_helper(" in source and "hs_store8(" in source,(optimization,source)
                    assert any(node["kind"]=="source-call" for node in document["ast"]["nodes"]),(optimization,document["ast"]["nodes"])
                    coverage=next(node["payload"] for node in document["nodes"] if node["payload"].get("artifact_kind")=="semantic-coverage")
                    assert coverage["strict_machine_state_blocked"] is True,(optimization,coverage)
                if name=="hs_c_call_then_store":
                    assert "hs_c_leaf_helper(" in source and "hs_store8(" in source,(optimization,source)
                    call_position=source.rfind(" = hs_c_leaf_helper(")
                    store_position=source.rfind("hs_store8(")
                    assert 0<=call_position<store_position,(optimization,source)
                    assert any(node["kind"]=="source-call-event" for node in document["ast"]["nodes"]),(optimization,document["ast"]["nodes"])
                if name=="hs_c_call_then_store_branch":
                    assert source.count(" = hs_c_leaf_helper(")==2 and source.count("    hs_store8(")==2,(optimization,source)
                    assert len([node for node in document["ast"]["nodes"] if node["kind"]=="source-call-event"])==2
                    assert len([node for node in document["ast"]["nodes"] if node["kind"]=="source-memory-store"])==2
                if name=="hs_c_conditional_call_store":
                    call_events=[node for node in document["ast"]["nodes"] if node["kind"]=="source-call-event"]
                    store_events=[node for node in document["ast"]["nodes"] if node["kind"]=="source-memory-store"]
                    assert source.count(" = hs_c_leaf_helper(")==1 and source.count("    hs_store8(")==len(store_events),(optimization,source)
                    assert len(call_events)==1 and len(store_events) in (1,2)
                    ast_by_id={node["id"]:node for node in document["ast"]["nodes"]}
                    assert ast_by_id[call_events[0]["children"][1]]["kind"]=="source-path"
                if name=="hs_c_call_then_load":
                    call_position=source.rfind(" = hs_c_leaf_helper(")
                    load_position=source.rfind(" = hs_load8(")
                    assert 0<=call_position<load_position,(optimization,source)
                    assert any(node["kind"]=="source-call-event" for node in document["ast"]["nodes"])
                    assert len([node for node in document["ast"]["nodes"] if node["kind"]=="source-memory-read"])==1
                if name=="hs_c_call_load_store":
                    call_position=source.rfind(" = hs_c_leaf_helper(")
                    load_position=source.rfind(" = hs_load8(")
                    store_position=source.rfind("    hs_store8(")
                    assert 0<=call_position<load_position<store_position,(optimization,source)
                    assert len([node for node in document["ast"]["nodes"] if node["kind"]=="source-call-event"])==1
                    assert len([node for node in document["ast"]["nodes"] if node["kind"]=="source-memory-read"])==1
                    assert len([node for node in document["ast"]["nodes"] if node["kind"]=="source-memory-store"])==1
                if name=="hs_c_store_load_then_call":
                    assert "hs_c_leaf_helper(" in source and "hs_store8(" in source,(optimization,source)
                    assert any(node["kind"]=="source-call" for node in document["ast"]["nodes"]),(optimization,document["ast"]["nodes"])
                if name in {"hs_c_store_then_load","hs_c_load_then_store","hs_c_conditional_store_then_load","hs_c_alternate_store_then_load","hs_c_store_load_then_call","hs_c_call_load_store"}:
                    arguments=",".join((f"({p['source_spelling']})(&memory_values[0])" if p["abi_gpr"]==0 else f"({p['source_spelling']})((x[0]&1)?&memory_values[0]:&memory_values[1])" if p["abi_gpr"]==1 else f"({p['source_spelling']})(uintptr_t)x[{p['abi_gpr']}]" ) for p in parameters)
                elif name in {"hs_c_recursive","hs_c_recursive_pair"}:
                    arguments=",".join(f"({p['source_spelling']})((x[{p['abi_gpr']}]%16))" for p in parameters)
                    function_stdin="".join(" ".join(f"{v:x}" for v in [row[0]%16,*row[1:]])+"\n" for row in vectors)
                elif name=="hs_c_loop_two_volatile_pointers":
                    arguments=",".join((f"({p['source_spelling']})(&memory_values[0])" if p['abi_gpr']==0 else f"({p['source_spelling']})((x[0]&1)?&memory_values[0]:&memory_values[1])" if p['abi_gpr']==1 else f"({p['source_spelling']})(x[{p['abi_gpr']}]%8)") for p in parameters)
                    function_stdin="".join(" ".join(f"{v:x}" for v in [row[0],row[1]%8,*row[2:]])+"\n" for row in vectors)
                elif name in {"hs_c_loop_volatile_store","hs_c_loop_volatile_accumulate"}:
                    arguments=",".join((f"({p['source_spelling']})(&memory_values[0])" if p['pointer'] else f"({p['source_spelling']})(x[{p['abi_gpr']}]%8)" ) for p in parameters)
                    function_stdin="".join(" ".join(f"{v:x}" for v in [row[0],row[1]%8,*row[2:]])+"\n" for row in vectors)
                elif name=="hs_c_recursive_store":
                    arguments=",".join((f"({p['source_spelling']})(&memory_values[0])" if p["pointer"] else f"({p['source_spelling']})x[{p['abi_gpr']}]" ) for p in parameters)
                elif name=="hs_c_call_then_load":
                    arguments=",".join((f"({p['source_spelling']})(&memory_values[0])" if p["pointer"] else f"({p['source_spelling']})x[{p['abi_gpr']}]" ) for p in parameters)
                elif name in {"hs_c_volatile_load_then_call","hs_c_call_then_volatile_load","hs_c_volatile_store_call_load"}:
                    arguments=",".join((f"({p['source_spelling']})(&memory_values[0])" if p["pointer"] else f"({p['source_spelling']})x[{p['abi_gpr']}]" ) for p in parameters)
                elif name=="hs_c_call_load_store":
                    arguments=",".join((f"({p['source_spelling']})(&memory_values[0])" if p["abi_gpr"]==0 else f"({p['source_spelling']})((x[0]&1)?&memory_values[0]:&memory_values[1])" if p["abi_gpr"]==1 else f"({p['source_spelling']})x[{p['abi_gpr']}]" ) for p in parameters)
                else:
                    arguments=",".join((f"({p['source_spelling']})memory_bytes" if name=="hs_c_store_dynamic" and p["pointer"] else f"({p['source_spelling']})(&memory_words[0])" if name=="hs_c_store_dynamic_u32" and p["pointer"] else f"({p['source_spelling']})(&memory_halves[0])" if name=="hs_c_store_dynamic_s16" and p["pointer"] else f"({p['source_spelling']})(&memory_values[0])" if name.startswith(("hs_c_load","hs_c_store","hs_c_volatile_store_call_load","hs_c_volatile_store_then_call","hs_c_call_then_volatile_store","hs_c_volatile_store_call_load","hs_c_loop_volatile_store","hs_c_loop_volatile_accumulate","hs_c_branch_store_call","hs_c_call_then_store","hs_c_conditional_call_store")) and p["pointer"] else f"({p['source_spelling']})(uintptr_t)x[{p['abi_gpr']}]" ) for p in parameters)
                wrapper=f"""
#include <stdio.h>
#include <inttypes.h>
int main(void) {{
 uint64_t x[8];
 while(scanf("%"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64" %"SCNx64,&x[0],&x[1],&x[2],&x[3],&x[4],&x[5],&x[6],&x[7])==8) {{
   uint64_t memory_values[4]={{x[1],x[2],x[3],x[4]}}; uint32_t memory_words[4]={{(uint32_t)x[1],(uint32_t)x[2],(uint32_t)x[3],(uint32_t)x[4]}}; int16_t memory_halves[4]={{(int16_t)x[1],(int16_t)x[2],(int16_t)x[3],(int16_t)x[4]}}; if ({int(name in {"hs_c_store","hs_c_store_volatile","hs_c_store_volatile_twice","hs_c_store_u16","hs_c_store_u32","hs_c_store_s16","hs_c_store_s32","hs_c_store_twice","hs_c_store_conditional","hs_c_store_then_call","hs_c_volatile_store_then_call","hs_c_call_then_volatile_store","hs_c_volatile_store_call_load","hs_c_loop_volatile_store","hs_c_loop_volatile_accumulate","hs_c_branch_store_call","hs_c_call_then_store","hs_c_call_then_store_branch","hs_c_recursive_store","hs_c_conditional_call_store"})}) memory_values[0]=x[2]; if ({int(name in {"hs_c_store_then_load","hs_c_load_then_store","hs_c_conditional_store_then_load","hs_c_alternate_store_then_load","hs_c_store_load_then_call","hs_c_call_load_store"})}) {{ memory_values[0]=x[3]; memory_values[1]=x[4]; }}
   uint8_t memory_bytes[16]; for (unsigned i=0;i<16;i++) memory_bytes[i]=(uint8_t)(x[3]+i*13); (void)memory_bytes;
   if ({int(name in {"hs_c_load_dynamic","hs_c_load_dynamic_s16","hs_c_load_dynamic_u32"})}) {{ memory_values[0]=x[2]; memory_values[1]=x[3]; memory_values[2]=x[4]; memory_values[3]=x[5]; }} (void)memory_values;
   if ({int(name=="hs_c_loop_two_volatile_pointers")}) {{ memory_values[0]=x[3]; memory_values[1]=x[4]; }}
   uint64_t result=(uint64_t){public}({arguments}); printf("%016"PRIx64"",result);
   if ({int(name in {"hs_c_store","hs_c_store_volatile","hs_c_store_volatile_twice","hs_c_store_u16","hs_c_store_u32","hs_c_store_s16","hs_c_store_s32","hs_c_store_twice","hs_c_store_conditional","hs_c_store_then_call","hs_c_volatile_store_then_call","hs_c_call_then_volatile_store","hs_c_volatile_store_call_load","hs_c_loop_volatile_store","hs_c_loop_volatile_accumulate","hs_c_branch_store_call","hs_c_call_then_store","hs_c_call_then_store_branch","hs_c_recursive_store","hs_c_conditional_call_store"})}) printf(" %016"PRIx64"",memory_values[0]);
   if ({int(name=="hs_c_store_twice")}) printf(" %016"PRIx64"",memory_values[2]);
   if ({int(name=="hs_c_store_dynamic")}) {{ putchar(' '); for(unsigned i=0;i<16;i++)printf("%02x",memory_bytes[i]); }}
   if ({int(name=="hs_c_store_dynamic_u32")}) printf(" %08"PRIx32" %08"PRIx32" %08"PRIx32" %08"PRIx32"",memory_words[0],memory_words[1],memory_words[2],memory_words[3]);
   if ({int(name=="hs_c_store_dynamic_s16")}) printf(" %04"PRIx16" %04"PRIx16" %04"PRIx16" %04"PRIx16"",(uint16_t)memory_halves[0],(uint16_t)memory_halves[1],(uint16_t)memory_halves[2],(uint16_t)memory_halves[3]);
   if ({int(name=="hs_c_loop_two_volatile_pointers")}) printf(" %016"PRIx64" %016"PRIx64"",memory_values[0],memory_values[1]);
   if ({int(name in {"hs_c_store_then_load","hs_c_load_then_store","hs_c_conditional_store_then_load","hs_c_alternate_store_then_load","hs_c_store_load_then_call","hs_c_call_load_store"})}) printf(" %016"PRIx64" %016"PRIx64"",memory_values[0],memory_values[1]); puts("");
 }}
 return 0;
}}
"""
                cfile=root/("recovered.m" if name=="objc" else "recovered.c");executable=root/"recovered"
                helper_definition="uint64_t hs_c_leaf_helper(uint64_t value) { return value + 5; }\n" if name in {"hs_c_call_helper","hs_c_call_twice","hs_c_volatile_store_then_call","hs_c_call_then_volatile_store","hs_c_volatile_store_call_load","hs_c_volatile_load_then_call","hs_c_volatile_store_call_load","hs_c_call_then_volatile_load","hs_c_volatile_store_call_load","hs_c_store_then_call","hs_c_store_load_then_call","hs_c_branch_store_call","hs_c_call_then_store","hs_c_call_then_store_branch","hs_c_conditional_call_store","hs_c_call_then_load","hs_c_call_load_store"} else ""
                cfile.write_text(source+helper_definition+wrapper)
                TEST.execute(["xcrun","clang","-std=c17","-O2","-Wall","-Wextra","-Werror","-fsanitize=undefined","-fno-sanitize-recover=all",cfile,"-o",executable])
                original=TEST.execute([native,binary,name],function_stdin).splitlines()
                recovered=TEST.execute([executable],function_stdin).splitlines()
                mask=(1<<source_root["attributes"]["result_bits"])-1
                assert len(original)==len(recovered)==len(vectors)
                original_rows=[line.split() for line in original];recovered_rows=[line.split() for line in recovered]
                original_results=[int(v[0],16)&mask for v in original_rows];recovered_results=[int(v[0],16)&mask for v in recovered_rows]
                assert original_results==recovered_results,(name,optimization,next((i,a,b) for i,(a,b) in enumerate(zip(original_results,recovered_results)) if a!=b))
                if name in {"hs_c_store","hs_c_store_volatile","hs_c_store_volatile_twice","hs_c_store_u16","hs_c_store_u32","hs_c_store_s16","hs_c_store_s32","hs_c_store_twice","hs_c_store_conditional","hs_c_conditional_store_then_load","hs_c_alternate_store_then_load","hs_c_store_then_call","hs_c_volatile_store_then_call","hs_c_call_then_volatile_store","hs_c_volatile_store_call_load","hs_c_loop_volatile_store","hs_c_loop_volatile_accumulate","hs_c_branch_store_call","hs_c_call_then_store","hs_c_call_then_store_branch","hs_c_conditional_call_store","hs_c_volatile_store_call_load","hs_c_loop_volatile_store","hs_c_loop_volatile_accumulate","hs_c_recursive_store"}:assert [int(v[1],16) for v in original_rows]==[int(v[1],16) for v in recovered_rows],(name,optimization)
                if name in {"hs_c_store_then_load","hs_c_load_then_store","hs_c_conditional_store_then_load","hs_c_alternate_store_then_load","hs_c_store_load_then_call","hs_c_call_load_store","hs_c_loop_two_volatile_pointers"}:
                    assert [v[1:] for v in original_rows]==[v[1:] for v in recovered_rows],(name,optimization,next((i,a,b) for i,(a,b) in enumerate(zip(original_rows,recovered_rows)) if a[1:]!=b[1:]))
                if name=="hs_c_store_twice":assert [int(v[2],16) for v in original_rows]==[int(v[2],16) for v in recovered_rows],(name,optimization)
                if name in {"hs_c_store_dynamic","hs_c_store_dynamic_u32","hs_c_store_dynamic_s16"}:assert [v[1:] for v in original_rows]==[v[1:] for v in recovered_rows],(name,optimization,next((i,a,b) for i,(a,b) in enumerate(zip(original_rows,recovered_rows)) if a[1:]!=b[1:]))
                repeated=request(worker,binary,"--reconstruct",start,end,prototypes=prototypes)
                assert document==repeated,(name,optimization,[key for key in document if document[key]!=repeated.get(key)])
                TEST.retain_validation(document,generated,{"artifact_kind":"recovered-source-qualification","toolchain":TEST.execute(["xcrun","clang","--version"]).splitlines()[0],"original_flags":[optimization,"-fno-stack-protector","-Wl,-no_fixup_chains","-target","arm64-apple-macos14.0"],"strip_flags":["-S","-x"] if stripped else [],"generated_flags":["-std=c17","-O2","-Wall","-Wextra","-Werror","-fsanitize=undefined","-fno-sanitize-recover=all"],"cases":len(vectors),"seed":"0x48535352","inputs_x0_x7":[[hex(v) for v in values] for values in vectors],"prototype_catalog_sha256":hashlib.sha256(prototypes.read_bytes()).hexdigest(),"source_sha256":hashlib.sha256(source.encode()).hexdigest(),"qualification_script_sha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),"comparison":"declared scalar result bits; not undefined high result bits, volatile-register/stack trace equivalence","formal_equivalence":False},evidence_root/f"recovered-source{build_name}-{name}.json")
                qualified.append({"name":name,"optimization":optimization,"stripped":stripped,"cases":len(vectors),"function_id":document["function"]["id"],"scope":document["scope"]})
            for name in ["hs_m_memory"]:
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
