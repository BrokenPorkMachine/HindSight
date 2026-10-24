#!/usr/bin/env python3
"""Prove Apple Clang declarations reach real AArch64 worker pseudocode."""

import hashlib
import json
import importlib.util
import os
import re
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import uuid

from darwin_sdk_catalog import collect_declarations, verify_macho_target, worker_prototypes

ROOT = Path(__file__).resolve().parents[1]


def fixture_offset(data, address):
    offset=32
    for _ in range(struct.unpack_from("<I",data,16)[0]):
        command,length=struct.unpack_from("<II",data,offset)
        if command==0x19:
            for index in range(struct.unpack_from("<I",data,offset+64)[0]):
                section=offset+72+index*80
                start,size,file_offset=struct.unpack_from("<QQI",data,section+32)
                if start<=address<start+size:return file_offset+address-start
        offset+=length
    raise AssertionError("fixture address has no section mapping")


def analyze(worker, binary, prototype_path=None):
    raw = binary.read_bytes()
    environment = dict(os.environ)
    environment.pop("HINDSIGHT_PROTOTYPE_DB", None)
    if prototype_path:
        environment["HINDSIGHT_PROTOTYPE_DB"] = str(prototype_path)
    result = subprocess.run([str(worker), "--analyze", str(binary), hashlib.sha256(raw).hexdigest(), str(len(raw)), str(uuid.uuid4())],
                            env=environment, check=True, capture_output=True, timeout=120)
    if len(result.stdout) < 4:
        raise AssertionError("truncated worker frame")
    size = struct.unpack(">I", result.stdout[:4])[0]
    assert size <= 32 * 1024 * 1024 and len(result.stdout) == size + 4
    return json.loads(result.stdout[4:])


def run(worker):
    if sys.platform != "darwin":
        print("Darwin SDK/worker integration requires macOS; skipped")
        return
    header = ROOT / "Tests/Fixtures/Decompilation/darwin_catalog_sample.h"
    declaration = collect_declarations(["HSNativeCatalogCount"], header)
    with tempfile.TemporaryDirectory(prefix="hindsight-darwin-worker-") as directory:
        spec=importlib.util.spec_from_file_location("collect_darwin_evidence",ROOT/"scripts/collect-darwin-evidence.py")
        native=importlib.util.module_from_spec(spec);spec.loader.exec_module(native)
        helper=Path(directory)/"native-oracle"
        native.compile_helper(helper,[header.with_suffix(".m")])
        contract = Path(directory) / "reconstruction-artifact-contract"
        swift_environment = dict(os.environ, CLANG_MODULE_CACHE_PATH=str(Path(directory) / "swift-module-cache"))
        subprocess.run(["xcrun", "swiftc", str(ROOT / "Shared/ReconstructionArtifact.swift"),
                        str(ROOT / "Tests/reconstruction_artifact_contract.swift"), "-o", str(contract)],
                       env=swift_environment, check=True, capture_output=True, timeout=120)
        sdk_contract = Path(directory) / "darwin-sdk-declarations-contract"
        subprocess.run(["xcrun", "swiftc", str(ROOT / "Shared/DarwinSDKDeclarations.swift"),
                        str(ROOT / "Tests/darwin_sdk_declarations_contract.swift"), "-o", str(sdk_contract)],
                       env=swift_environment, check=True, capture_output=True, timeout=120)
        native_declaration = subprocess.run([str(sdk_contract), "--collect", str(header), "HSNativeCatalogCount"],
                                            check=True, capture_output=True, text=True, timeout=90).stdout.splitlines()[0]
        assert native_declaration == declaration["functions"][0]["abi_declaration"], "native and Python SDK ABI declarations disagree"
        oracle=native.collect(helper,["HSNativeCatalogFixture"],[])["classes"][0]
        for optimization in ["-O0", "-O2"]:
            binary = Path(directory) / "sdk-sample"
            subprocess.run(["xcrun", "--sdk", "macosx", "clang", "-target", "arm64-apple-macos14.0", optimization,
                            "-fobjc-arc", "-DHS_CATALOG_STANDALONE", "-Wl,-no_fixup_chains", "-framework", "Foundation",
                            str(header.with_suffix(".m")), "-o", str(binary)],
                           check=True, capture_output=True, timeout=60)
            verify_macho_target(binary, "arm64-apple-macos14.0")
            baseline = analyze(worker, binary)
            digest = hashlib.sha256(binary.read_bytes()).hexdigest()
            catalog = Path(directory) / "prototypes.tsv"
            catalog.write_text(worker_prototypes(declaration, digest))
            recovered = analyze(worker, binary, catalog)
            assert recovered["kind"] == "analysis_result", recovered
            assert "unsigned long HSNativeCatalogCount(unsigned long value);" in recovered["call_values"], recovered["call_values"]
            assert "unsigned long HSNativeCatalogCount(unsigned long value);" not in baseline["call_values"]
            assert "value=x0" in recovered["call_values"] or "value=x0" in recovered["semantic_preview"]
            assert "// unsigned long HSNativeCatalogCount(unsigned long value) [" in recovered["semantic_preview"]
            assert "HSNativeCatalogCount((unsigned long)0x2);" in recovered["semantic_preview"]
            # Exercise the actual dump producer, not only analysis pseudocode.
            boundary = next(re.match(r"^(0x[0-9a-f]+)\s+(0x[0-9a-f]+)\s+\d+\s+bytes\s+_HSNativeCatalogCount\s", line)
                            for line in recovered["functions"].splitlines()
                            if re.match(r"^0x[0-9a-f]+\s+0x[0-9a-f]+\s+\d+\s+bytes\s+_HSNativeCatalogCount\s", line))
            environment = dict(os.environ, HINDSIGHT_PROTOTYPE_DB=str(catalog))
            output = subprocess.run([str(worker), "--reconstruct", str(binary), digest,
                                     str(binary.stat().st_size), boundary[1], boundary[2], str(uuid.uuid4())],
                                    env=environment, check=True, capture_output=True, timeout=120).stdout
            assert len(output) >= 4 and struct.unpack(">I", output[:4])[0] == len(output)-4
            artifact = json.loads(output[4:])
            headers = [node["payload"] for node in artifact["nodes"]
                       if node["payload"].get("artifact_kind") == "recovered-header"]
            assert len(headers) == 1, "SDK declaration did not reach reconstructed header"
            assert "unsigned long HSNativeCatalogCount(unsigned long value);" in headers[0]["source"], headers
            objc_boundary = next(line.split()[:2] for line in recovered["functions"].splitlines()
                                 if "-[HSNativeCatalogFixture score:count:]" in line)
            objc_output = subprocess.run([str(worker), "--reconstruct", str(binary), digest,
                                          str(binary.stat().st_size), *objc_boundary, str(uuid.uuid4())],
                                         env=environment, check=True, capture_output=True, timeout=120).stdout
            assert len(objc_output) >= 4 and struct.unpack(">I", objc_output[:4])[0] == len(objc_output)-4
            objc_artifact = json.loads(objc_output[4:])
            objc_headers = [node["payload"] for node in objc_artifact["nodes"]
                            if node["payload"].get("artifact_kind") in {"recovered-header", "best-effort-header"}]
            assert len(objc_headers) == 1, "Objective-C IMP lost its declaration evidence"
            objc_header = objc_headers[0]
            assert objc_header["languages"] == ["objective-c", "objective-c++"]
            for text in ["@interface HSNativeCatalogFixture", "@property", "displayLabel", "score:", "rangeForCount:", "_weight"]:
                assert text in objc_header["source"], f"missing Objective-C header declaration: {text}"
            assert "hs_review_context" not in objc_header["source"]
            assert "@interface HSNativeCatalogFixture : NSObject" in objc_header["source"]
            assert "super_source=dyld-bind" in recovered["objc_classes"]
            superclass_record = next(line for line in recovered["objc_classes"].splitlines()
                                     if line.startswith("reconstructed class ") and "name=HSNativeCatalogFixture " in line)
            provider = re.search(r" super_provider=(\S+)", superclass_record)
            assert provider and provider[1].startswith(("/usr/lib/", "/System/Library/Frameworks/")), superclass_record
            assert provider[1] in recovered["dylib_ordinals"]
            assert "superclass" in objc_header["source"] and "REVIEW ONLY" in objc_header["source"]
            header_path = Path(directory) / "reconstructed-objc.h"
            header_path.write_text(objc_header["source"])
            for language in ["objective-c", "objective-c++"]:
                subprocess.run(["xcrun", "--sdk", "macosx", "clang", "-target", "arm64-apple-macos14.0",
                                "-fsyntax-only", "-x", language, str(header_path)],
                               check=True, capture_output=True, timeout=60)
            derived_boundary = next(line.split()[:2] for line in recovered["functions"].splitlines()
                                    if "-[HSNativeCatalogDerived derivedValue]" in line)
            derived_output = subprocess.run([str(worker), "--reconstruct", str(binary), digest,
                                             str(binary.stat().st_size), *derived_boundary, str(uuid.uuid4())],
                                            env=environment, check=True, capture_output=True, timeout=120).stdout
            derived = json.loads(derived_output[4:])
            derived_headers = [node["payload"]["source"] for node in derived["nodes"]
                               if node["payload"].get("artifact_kind") in {"recovered-header", "best-effort-header"}]
            assert len(derived_headers) == 1
            assert "@interface HSNativeCatalogDerived : HSNativeCatalogFixture <NSCopying>" in derived_headers[0]
            assert "@protocol NSCopying;" in derived_headers[0]
            scalar_boundary = next(line.split()[:2] for line in recovered["functions"].splitlines()
                                   if "-[HSNativeCatalogFixture scalarCount:]" in line)
            scalar_output = subprocess.run([str(worker), "--reconstruct", str(binary), digest,
                                            str(binary.stat().st_size), *scalar_boundary, str(uuid.uuid4())],
                                           env=environment, check=True, capture_output=True, timeout=120).stdout
            scalar = json.loads(scalar_output[4:])
            scalar_artifact_path = Path(directory) / "scalar-artifact.json"
            scalar_artifact_path.write_bytes(scalar_output[4:])
            subprocess.run([str(contract), str(scalar_artifact_path), digest,
                            scalar_boundary[0][2:], scalar_boundary[1][2:]],
                           env=swift_environment, check=True, capture_output=True, timeout=60)
            objc_sources = [node["payload"] for node in scalar["nodes"] if node["payload"].get("artifact_kind") == "recovered-objective-c"]
            assert len(objc_sources) == 1, "qualified scalar IMP did not produce Objective-C code"
            objc_code = objc_sources[0]["source"]
            assert "@implementation HSNativeCatalogFixture (HSRecovered_" in objc_code
            assert "scalarCount:" in objc_code and "return hs_recovered_imp_" in objc_code
            assert "synthetic" in objc_code and "validated IR-derived IMP body" in objc_code
            objc_path = Path(directory) / "reconstructed-method.m"
            objc_path.write_text(objc_code)
            subprocess.run(["xcrun", "--sdk", "macosx", "clang", "-target", "arm64-apple-macos14.0",
                            "-fsyntax-only", "-x", "objective-c", str(objc_path)],
                           check=True, capture_output=True, timeout=60)
            # A deliberately wrong base method makes this a dispatch check,
            # not a comparison that could accidentally exercise original code.
            recovered_c = next(node["payload"] for node in scalar["nodes"]
                               if node["payload"].get("artifact_kind") == "recovered-source")
            method = recovered_c["declaration"].removesuffix(";")
            harness = Path(directory) / "reconstructed-method-run.m"
            harness.write_text('#include "reconstructed-method.m"\n'
                               '@implementation HSNativeCatalogFixture\n'
                               + method + ' { (void)arg0; return 0; }\n@end\n'
                               'int main(void) { @autoreleasepool {\n'
                               'HSNativeCatalogFixture *object = [HSNativeCatalogFixture new];\n'
                               'unsigned long long values[] = {0,1,2,42,0x7fffffffffffffffULL,0xffffffffffffffffULL};\n'
                               'for (unsigned i=0;i<sizeof(values)/sizeof(values[0]);i++) {\n'
                               'if ([object scalarCount:values[i]] != values[i]+3) return 1;\n'
                               '} return 0; } }\n')
            executable = Path(directory) / "reconstructed-method-run"
            compiled = subprocess.run(["xcrun", "--sdk", "macosx", "clang", "-target", "arm64-apple-macos14.0",
                                       optimization, "-fobjc-arc", str(harness), "-framework", "Foundation", "-o", str(executable)],
                                      capture_output=True, timeout=60)
            assert compiled.returncode == 0, compiled.stderr.decode(errors="replace")
            subprocess.run([str(executable)], check=True, capture_output=True, timeout=30)
            control = Path(directory) / "method-without-adapter.m"
            control.write_text(harness.read_text().replace('#include "reconstructed-method.m"', '#include "reconstructed-objc.h"', 1))
            control_executable = Path(directory) / "method-without-adapter"
            subprocess.run(["xcrun", "--sdk", "macosx", "clang", "-target", "arm64-apple-macos14.0",
                            optimization, "-fobjc-arc", str(control), "-framework", "Foundation", "-o", str(control_executable)],
                           check=True, capture_output=True, timeout=60)
            assert subprocess.run([str(control_executable)], capture_output=True, timeout=30).returncode == 1
            batch_path = Path(directory) / ("objc-batch" + optimization)
            subprocess.run([sys.executable, str(ROOT / "scripts/reconstruct-binary.py"), str(binary), str(batch_path),
                            "--worker", str(worker), "--prototypes", str(catalog),
                            "--start", scalar_boundary[0], "--end", scalar_boundary[1]],
                           check=True, capture_output=True, timeout=120)
            dump_path = Path(directory) / ("objc-dump" + optimization)
            dumped = subprocess.run([sys.executable, str(ROOT / "scripts/export-reconstruction-batch.py"), str(batch_path), str(dump_path),
                                    "--format", "objc"], capture_output=True, timeout=60)
            assert dumped.returncode == 0, dumped.stderr.decode(errors="replace")
            dump_manifest = json.loads((dump_path / "export-manifest.json").read_bytes())
            assert dump_manifest["available"] == 1
            assert dump_manifest["functions"][0]["representation"] == "recovered-objective-c"
            assert next(dump_path.glob("*.m")).read_text() == objc_code
            pseudo=recovered["semantic_preview"]
            assert "@interface HSNativeCatalogFixture {" in pseudo,pseudo[-16000:]
            assert "getter=displayLabel, setter=setDisplayLabel:" in pseudo
            assert "NSString * label;" in pseudo
            assert "- (NSString *)displayLabel {" in pseudo
            assert "+ (NSRange)rangeForCount:(unsigned long long)arg0 {" in pseudo
            for name in ["_count","_weight","_label"]:
                assert "self->"+name in pseudo,f"missing IR-proven ivar access: {name}\n{pseudo[-16000:]}"
            for ivar in oracle["ivars"]:
                pattern=rf"ivar (0x[0-9a-f]+) name={re.escape(ivar['name'])} type=[^\n]* offset_ptr=(0x[0-9a-f]+) offset=(0x[0-9a-f]+) size=(\d+) alignment=(\d+)"
                found=re.search(pattern,recovered["objc_classes"])
                assert found,recovered["objc_classes"]
                assert int(found[3],16)==ivar["offset"]
                assert int(found[4])==ivar["type"]["size"]
                assert int(found[5])==ivar["type"]["alignment"]
            # An unknown offset pointer or contradictory width must not become
            # a source field just because its name still appears in strings.
            original=binary.read_bytes()
            weight=re.search(r"ivar (0x[0-9a-f]+) name=_weight type=d offset_ptr=(0x[0-9a-f]+)",recovered["objc_classes"])
            assert weight
            for label,field_offset,value,format_string in [("unmapped-offset",0,0xfffffffffffffff0,"<Q"),("wrong-width",28,4,"<I")]:
                corrupt=bytearray(original)
                struct.pack_into(format_string,corrupt,fixture_offset(original,int(weight[1],16))+field_offset,value)
                malformed=Path(directory)/label;malformed.write_bytes(corrupt)
                withheld=analyze(worker,malformed)
                assert withheld["kind"]=="analysis_result",withheld
                assert "self->_weight" not in withheld["semantic_preview"],withheld["semantic_preview"][-16000:]
            catalog.write_text(worker_prototypes(declaration, "0" * 64))
            rejected = analyze(worker, binary, catalog)
            assert rejected["kind"] == "error" and "SHA-256 mismatch" in rejected["message"], rejected
            # Platform-tag variants exercise the same encoded Mach-O/dyld
            # paths. This is a parser qualification, not an iOS execution run.
            ios_bytes = bytearray(binary.read_bytes())
            offset = 32
            changed = False
            for _ in range(struct.unpack_from("<I", ios_bytes, 16)[0]):
                command, length = struct.unpack_from("<II", ios_bytes, offset)
                if command == 0x32:
                    struct.pack_into("<I", ios_bytes, offset + 8, 2)
                    changed = True
                    break
                offset += length
            assert changed
            ios_binary = Path(directory) / "ios-platform-variant"
            ios_binary.write_bytes(ios_bytes)
            verify_macho_target(ios_binary, "arm64-apple-ios16.0")
            try:
                verify_macho_target(ios_binary, "arm64-apple-macos14.0")
            except ValueError:
                pass
            else:
                raise AssertionError("macOS prototype binding admitted iOS-only platform record")
            ios_report = analyze(worker, ios_binary)
            assert ios_report["kind"] == "analysis_result", ios_report
            for field in ["dyld_imports", "dyld_linking", "chained_fixups", "export_trie", "resolved_imports", "indirect_symbols", "objc_classes"]:
                assert ios_report[field] == baseline[field], f"shared Mach-O/dyld path differs for {field}"
            print(f"Apple Clang prototype -> AArch64 pseudocode, ABI name and source-hash rejection passed ({optimization})")
            print(f"Shared macOS/iOS Mach-O/dyld decoder outputs and platform-aware prototype binding passed ({optimization})")
            print(f"Native-oracle ivar/property layouts -> typed declarations and IR-proven field accesses passed ({optimization})")
            print(f"Recovered Objective-C method passed Swift/batch dumps, syntax, six runtime inputs and failing no-adapter control ({optimization})")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: test-darwin-catalog.py /absolute/path/to/reworker")
    run(Path(sys.argv[1]).resolve(strict=True))
