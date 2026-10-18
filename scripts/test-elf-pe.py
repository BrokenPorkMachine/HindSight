#!/usr/bin/env python3
"""Exercise ELF/PE capture, analysis and paging using the shipped worker wire API."""
import hashlib
import io
import json
import os
import pathlib
import struct
import subprocess
import sys
import tempfile
import uuid

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "python"))
from reworkbench_sdk import read_frame

WORKER = sys.argv[1]
REQUEST = str(uuid.uuid4())


def elf(bits=64, be=False, sections=False, arm=False):
    order = ">" if be else "<"
    data = bytearray(0x400)
    data[:16] = b"\x7fELF" + bytes([2 if bits == 64 else 1, 2 if be else 1, 1]) + bytes(9)
    base = 0x400000
    code = bytes.fromhex("1f2003d5c0035fd6") if arm else b"\x55\x48\x89\xe5\x5d\xc3\x90\x90"
    if bits == 64:
        struct.pack_into(order + "HHIQQQIHHHHHH", data, 16, 2, 183 if arm else 62, 1,
                         base + 0x100, 64, 0x300 if sections else 0, 0, 64, 56, 2, 64,
                         4 if sections else 0, 3 if sections else 0)
        struct.pack_into(order + "IIQQQQQQ", data, 64, 1, 5, 0x100, base + 0x100, 0, 8, 8, 0x100)
        struct.pack_into(order + "IIQQQQQQ", data, 120, 1, 6, 0x200, base + 0x200, 0, 32, 64, 0x100)
        if sections:
            struct.pack_into(order + "IIQQQQIIQQ", data, 0x340, 1, 1, 6, base + 0x100, 0x100, 8, 0, 0, 4, 0)
            struct.pack_into(order + "IIQQQQIIQQ", data, 0x380, 7, 1, 2, base + 0x200, 0x200, 32, 0, 0, 1, 0)
            struct.pack_into(order + "IIQQQQIIQQ", data, 0x3c0, 15, 3, 0, 0, 0x280, 25, 0, 0, 1, 0)
            names = b"\0.text\0.rodata\0\0.shstrtab\0\0\0"
            data[0x280:0x280 + len(names)] = names
    else:
        struct.pack_into(order + "HHIIIIIHHHHHH", data, 16, 2, 3, 1, base + 0x100, 52, 0, 0, 52, 32, 2, 40, 0, 0)
        struct.pack_into(order + "IIIIIIII", data, 52, 1, 0x100, base + 0x100, 0, 8, 8, 5, 0x100)
        struct.pack_into(order + "IIIIIIII", data, 84, 1, 0x200, base + 0x200, 0, 32, 64, 6, 0x100)
    data[0x100:0x108] = code
    data[0x200:0x210] = b"format-evidence\0"
    return data


def pe(bits=64, arm=False):
    data = bytearray(0x600)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 60, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    optional = 240 if bits == 64 else 224
    struct.pack_into("<HHIIIHH", data, 0x84, 0xaa64 if arm else 0x8664 if bits == 64 else 0x14c, 2, 0, 0, 0, optional, 2)
    start = 0x98
    struct.pack_into("<H", data, start, 0x20b if bits == 64 else 0x10b)
    struct.pack_into("<I", data, start + 16, 0x1000)
    struct.pack_into("<Q" if bits == 64 else "<I", data, start + (24 if bits == 64 else 28), 0x140000000 if bits == 64 else 0x400000)
    struct.pack_into("<II", data, start + 32, 0x1000, 0x200)
    struct.pack_into("<II", data, start + 56, 0x3000, 0x200)
    struct.pack_into("<I", data, start + (108 if bits == 64 else 92), 16)
    table = start + optional
    struct.pack_into("<8sIIIIIIHHI", data, table, b".text", 8, 0x1000, 0x200, 0x200, 0, 0, 0, 0, 0x60000020)
    struct.pack_into("<8sIIIIIIHHI", data, table + 40, b".rdata", 0x300, 0x2000, 0x200, 0x400, 0, 0, 0, 0, 0x40000040)
    data[0x200:0x208] = bytes.fromhex("1f2003d5c0035fd6") if arm else b"\x55\x48\x89\xe5\x5d\xc3\x90\x90"
    data[0x400:0x410] = b"format-evidence\0"
    return data


def run(*args, env=None):
    reply = subprocess.run([WORKER, *map(str, args)], capture_output=True, timeout=60, check=True, env=env)
    stream = io.BytesIO(reply.stdout)
    # Analysis has a larger frame allowance than control replies.
    size = struct.unpack(">I", stream.read(4))[0]
    assert size <= 32 * 1024 * 1024
    result = json.loads(stream.read(size))
    assert read_frame(stream) is None
    return result


with tempfile.TemporaryDirectory() as directory:
    root = pathlib.Path(directory)

    def analyze(data, fmt="auto", arch="auto", base="0", offset="0"):
        path = root / "input.bin"
        path.write_bytes(data)
        digest = hashlib.sha256(data).hexdigest()
        args = [path, digest, len(data), REQUEST]
        return run("--analyze-image", *args, fmt, arch, base, offset), args

    for label, data, fmt, address in [
        ("ELF64", elf(), "elf", 0x400100),
        ("ELF32", elf(32), "elf", 0x400100),
        ("ELF64 sections", elf(sections=True), "elf", 0x400100),
        ("ELF AArch64", elf(arm=True), "elf", 0x400100),
        ("PE32+", pe(), "pe", 0x140001000),
        ("PE32", pe(32), "pe", 0x401000),
        ("PE AArch64", pe(arm=True), "pe", 0x140001000),
    ]:
        for selected in ["auto", fmt]:
            result, args = analyze(data, selected)
            assert result["kind"] == "analysis_result", (label, result)
            assert result["analyzed_instructions"] > 0, (label, result["analysis_quality"])
            assert f"0x{address:016x}" in result["functions"], label
            assert "format-evidence" in result["strings"], label
            page = run("--page-image", *args[:3], "0", args[3], selected, "auto", "0", "0")
            assert page["kind"] == "page_result", (label, page)
            assert int(page["section_address"]) == address
        source = root / "source.bin"
        destination = root / (label.replace(" ", "-") + ".capture")
        source.write_bytes(data)
        result = run("--capture-image", source, destination, REQUEST, fmt, "auto", "0", "0")
        assert result["kind"] == "header_result", (label, result)
        assert result["structure_status"] == "checked" and result["disassembly_status"] == "preview", (label, result)
        assert destination.read_bytes() == data
        assert result["file_kind"].startswith("ELF" if fmt == "elf" else "PE/COFF")
        assert result["source_sha256"] == hashlib.sha256(data).hexdigest()

    for bits in [32, 64]:
        result, _ = analyze(elf(bits, be=True))
        assert result["kind"] == "analysis_result" and result["analyzed_instructions"] == 0
        assert "Big-endian" in result["function_intelligence"]
    for data, fmt in [(elf(), "elf"), (pe(), "pe")]:
        result, args = analyze(bytes(64) + data, fmt, offset="64")
        assert result["kind"] == "analysis_result", result
        page = run("--page-image", *args[:3], "0", args[3], fmt, "auto", "0", "64")
        assert int(page["section_offset"]) == (0x100 if fmt == "elf" else 0x200) + 64
        for invalid in [data[:n] for n in [0, 4, 16, 32, 63, 127, 255]]:
            result, _ = analyze(invalid, fmt)
            assert result["kind"] == "error", result
        for settings in [(fmt, "i386" if fmt == "elf" else "aarch64", "0"), (fmt, "auto", "4096"), ("pe" if fmt == "elf" else "elf", "auto", "0")]:
            result, _ = analyze(data, *settings)
            assert result["kind"] == "error", (settings, result)

    malformed = []
    for offset, value in [(32, 0xfffffffffffffff0), (24, 0x500000), (120 + 16, 0x400100)]:
        data = elf()
        struct.pack_into("<Q", data, offset, value)
        malformed.append(data)
    data = elf(sections=True)
    struct.pack_into("<Q", data, 0x340 + 24, 0xfffffffffffffff0)
    malformed.append(data)
    for offset, value in [(60, 0xfffffff0), (0x98 + 16, 0x5000), (0x98 + 240 + 40 + 12, 0x1000), (0x98 + 240 + 20, 0x100), (0x98 + 108, 17)]:
        data = pe()
        struct.pack_into("<I", data, offset, value)
        malformed.append(data)
    for data in malformed:
        result, _ = analyze(data)
        assert result["kind"] == "error", result
    data = elf()
    result, args = analyze(data)
    (root / "input.bin").write_bytes(data[:-1] + b"!")
    assert run("--analyze-image", *args, "elf", "auto", "0", "0")["kind"] == "error"

    # Exchange-derived catalogs remain bound to the verified artifact in the worker.
    result, args = analyze(elf())
    names = root / "names.txt"
    digest = hashlib.sha256(elf()).hexdigest()
    env = dict(os.environ, HINDSIGHT_SYMBOL_DB=str(names))
    names.write_text(f"# source_sha256={digest}\n0x400100 imported_entry\n")
    result = run("--analyze-image", *args, "elf", "auto", "0", "0", env=env)
    assert result["kind"] == "analysis_result" and "imported_entry" in result["functions"], result
    assert "source=external-symbol-db" in result["functions"]
    names.write_text("# source_sha256=" + "0" * 64 + "\n0x400100 wrong_image\n")
    result = run("--analyze-image", *args, "elf", "auto", "0", "0", env=env)
    assert result["kind"] == "error" and "SHA-256 mismatch" in result["message"], result
    names.write_text("# comment\n" * 200001)
    result = run("--analyze-image", *args, "elf", "auto", "0", "0", env=env)
    assert result["kind"] == "error" and "budget" in result["message"], result

print("ELF/PE capture, mappings, decode, evidence, paging, byte order, embedded offsets and malformed-input checks passed")
