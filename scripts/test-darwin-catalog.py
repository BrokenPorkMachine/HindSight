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


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: test-darwin-catalog.py /absolute/path/to/reworker")
    run(Path(sys.argv[1]).resolve(strict=True))
