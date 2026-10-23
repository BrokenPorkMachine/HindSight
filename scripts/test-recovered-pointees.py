#!/usr/bin/env python3
"""Qualify native Apple LP64 pointer types against test-owned C fixtures."""
import hashlib
import importlib.util
import json
from pathlib import Path
import platform
import re
import subprocess
import sys
import tempfile

from reconstruction_artifact import ReconstructionArtifact

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("pointee_requests", ROOT / "scripts/test-worker-reconstruction.py")
requests = importlib.util.module_from_spec(spec)
spec.loader.exec_module(requests)
TYPES = [("char", 8, True), ("signed char", 8, True), ("unsigned char", 8, False),
         ("short", 16, True), ("unsigned short", 16, False), ("int", 32, True),
         ("unsigned int", 32, False), ("long", 64, True), ("unsigned long", 64, False),
         ("long long", 64, True), ("unsigned long long", 64, False), ("size_t", 64, False),
         ("signed short int", 16, True), ("signed long", 64, True), ("signed long int", 64, True),
         ("signed long long", 64, True), ("signed long long int", 64, True),
         ("NSInteger", 64, True), ("NSUInteger", 64, False), ("CFIndex", 64, True),
         ("CFTypeID", 64, False), ("CFOptionFlags", 64, False), ("CFStringEncoding", 32, False),
         ("CFNumberType", 64, True), ("CFURLPathStyle", 64, True), ("CFPropertyListFormat", 64, True),
         ("dispatch_once_t", 64, True)]
OPERATIONS = ["load", "store", "exchange", "zero", "ones", "truncate"]


def command(arguments):
    subprocess.run(arguments, check=True, timeout=180)


def run(worker):
    if sys.platform != "darwin" or platform.machine() != "arm64":
        raise SystemExit("Native pointee qualification requires Apple Silicon macOS.")
    configurations = comparisons = 0
    with tempfile.TemporaryDirectory(prefix="hindsight-pointees-") as temporary:
        root = Path(temporary)
        preamble = "#include <stdint.h>\n#include <stddef.h>\n#include <string.h>\n#import <Foundation/Foundation.h>\n#include <dispatch/dispatch.h>\n"
        declarations, definitions = [], []
        for index, (ty, bits, signed) in enumerate(TYPES):
            definitions.append(f'_Static_assert(sizeof({ty})*8=={bits}, "Apple LP64 width");')
            definitions.append(f'_Static_assert((({ty})-1 < ({ty})0)=={int(signed)}, "Apple signedness");')
            for operation in OPERATIONS:
                name = f"hs_native_{index}_{operation}"
                result = ty if operation in {"load", "exchange"} else "void"
                parameters = f"{'const ' if operation == 'load' else ''}volatile {ty} *pointer, uint64_t index"
                if operation in {"store", "exchange", "truncate"}:
                    parameters += f", {'uint64_t' if operation == 'truncate' else ty} value"
                declaration = f"{result} {name}({parameters})"
                body = (f"pointer[index]=({ty}){0 if operation == 'zero' else -1};" if operation in {"zero", "ones"} else
                        "return pointer[index];" if operation == "load" else
                        f"pointer[index]=({ty})value;" if operation == "truncate" else
                        "pointer[index]=value;" if operation == "store" else
                        f"{ty} previous=pointer[index]; pointer[index]=value; return previous;")
                declarations.append((name, declaration + ";", index, operation))
                definitions.append(declaration + " {" + body + "}")
        fixture = root / "original.m"
        fixture.write_text(preamble + "\n".join(definitions))
        for optimization in ["-O0", "-O1", "-O2", "-Os"]:
            binary = root / "original.dylib"
            command(["xcrun", "clang", optimization, "-fno-stack-protector", "-dynamiclib", str(fixture), "-framework", "Foundation", "-o", str(binary)])
            digest = hashlib.sha256(binary.read_bytes()).hexdigest()
            prototypes = root / "prototypes.tsv"
            prototypes.write_text(f"# source_sha256={digest}\n" + "".join(f"_{name}\t{declaration}\n" for name, declaration, _, _ in declarations))
            report = requests.request(worker, binary, "--analyze")
            ranges = {}
            for line in report["functions"].splitlines():
                match = re.match(r"^(0x[0-9a-f]+)\s+(0x[0-9a-f]+)\s+\d+\s+bytes\s+(\S+)", line)
                if match:
                    ranges[match[3]] = (match[1], match[2])
            generated = []
            for name, _, index, operation in declarations:
                artifact = requests.request(worker, binary, "--reconstruct", *ranges["_" + name], prototypes=prototypes)
                ReconstructionArtifact(artifact)
                payloads = [node["payload"] for node in artifact["nodes"]]
                bodies = [payload["source"] for payload in payloads if payload.get("artifact_kind") == "recovered-source"]
                headers = [payload["source"] for payload in payloads if payload.get("artifact_kind") == "recovered-header"]
                assert len(bodies) == len(headers) == 1, (name, optimization, [payload for payload in payloads if "reason" in payload])
                ty, bits, signed = TYPES[index]
                loads = [node for node in artifact["ast"]["nodes"] if node["kind"] == "source-memory-load"]
                assert len(loads) == (1 if operation in {"load", "exchange"} else 0), (name, optimization)
                for load in loads:
                    source_type = load["attributes"]["source_type"]
                    assert (source_type["spelling"], source_type["bits"], source_type["signed"]) == (ty, bits, signed), (name, optimization, source_type)
                if operation in {"zero", "ones", "truncate"}:
                    ast_by_id = {node["id"]: node for node in artifact["ast"]["nodes"]}
                    stores = [node for node in artifact["ast"]["nodes"] if node["kind"] == "source-memory-store"]
                    assert len(stores) == 1, (name, optimization)
                    stored_type = ast_by_id[stores[0]["children"][1]]["attributes"]["source_type"]
                    assert (stored_type["spelling"], stored_type["bits"], stored_type["signed"]) == (ty, bits, signed), (name, optimization, stored_type)
                prefix = "s" if signed else "u"
                for access in ["load", "store"]:
                    expected = operation in {access, "exchange"} or (access == "store" and operation in {"zero", "ones", "truncate"})
                    helper = f"hs_v{access}_{prefix}{bits // 8}_{ty.replace(' ', '_')}("
                    assert bodies[0].count(helper) == (2 if expected else 0), (name, optimization, access)
                assert f"volatile {ty} *p=(volatile {ty} *)a;" in bodies[0], (name, optimization)
                header = root / f"{name}.h"
                header.write_text(headers[0])
                command(["xcrun", "clang", "-fsyntax-only", "-x", "objective-c++-header" if ty.startswith("NS") else "c++-header", "-std=c++17", "-Wall", "-Wextra", "-Werror", str(header)])
                source = root / (name + (".m" if ty.startswith("NS") else ".c"))
                source.write_text(f'#define {name} recovered_{name}\n#include "{header.name}"\n' + bodies[0])
                generated.append(str(source))
                configurations += 1
            runner = [preamble]
            for name, declaration, _, _ in declarations:
                runner.extend([declaration, declaration.replace(name + "(", "recovered_" + name + "(")])
            runner.append("int main(void) {")
            for index, (ty, bits, _) in enumerate(TYPES):
                for operation in OPERATIONS:
                    name = f"hs_native_{index}_{operation}"
                    arguments = "a.values,offset" + (",wide[next]" if operation == "truncate" else ",seeds[next]" if operation in {"store", "exchange"} else "")
                    recovered_arguments = arguments.replace("a.values", "b.values")
                    call = (f"{name}({arguments}); recovered_{name}({recovered_arguments});" if operation in {"store", "zero", "ones", "truncate"} else
                            f"if({name}({arguments})!=recovered_{name}({recovered_arguments}))return 1;")
                    runner.append("{" + f"{ty} seeds[]={{0,1,({ty})-1,({ty})(UINT64_C(1)<<{bits-1}),({ty})((UINT64_C(1)<<{bits-1})-1)}};"
                                  + ("uint64_t wide[]={0,1,UINT64_MAX,UINT64_C(0x123456789abcdef0),UINT64_C(0x8000000000000001)};" if operation == "truncate" else "")
                                  + f"typedef struct {{ uint64_t before; {ty} values[5]; uint64_t after; }} Cell;"
                                  f"for(unsigned seed=0;seed<5;seed++)for(unsigned offset=0;offset<5;offset++)for(unsigned next=0;next<{'1' if operation in {'zero', 'ones'} else '5'};next++){{"
                                  "Cell a,b;memset(&a,0,sizeof(a));a.before=0x1234;a.after=0xabcd;"
                                  "for(unsigned j=0;j<5;j++)a.values[j]=seeds[(seed+j)%5];memcpy(&b,&a,sizeof(a));"
                                  + call + "if(memcmp(&a,&b,sizeof(a)))return 2;}}")
                    comparisons += 25 if operation in {"zero", "ones"} else 125
            runner.append("return 0;}")
            runner_file = root / "runner.m"
            runner_file.write_text("\n".join(runner))
            executable = root / "compare"
            command(["xcrun", "clang", "-std=c17", "-O1", "-Wall", "-Wextra", "-Werror", "-fsanitize=undefined",
                     *generated, str(runner_file), str(binary), "-framework", "Foundation", "-o", str(executable)])
            command([str(executable)])
            print(json.dumps({"optimization": optimization, "configurations": configurations,
                              "native_comparisons": comparisons, "status": "passed"}), flush=True)
    print(json.dumps({"native_pointee_configurations": configurations, "native_comparisons": comparisons, "status": "passed"}))


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else ROOT / "rust/target/debug/reworker").resolve())
