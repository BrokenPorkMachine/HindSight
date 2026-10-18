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


def elf_symbols(bits=64, be=False):
    data = elf(bits, be)
    data.extend(bytes(0x900 - len(data)))
    order = ">" if be else "<"
    stride = 64 if bits == 64 else 40
    symbol_stride = 24 if bits == 64 else 16
    strings = b"\0named_entry\0missing_api\0tls_slot\0"
    names = b"\0.text\0.rodata\0\0.shstrtab\0"
    data[0x280:0x280 + len(names)] = names
    data[0x700:0x700 + len(strings)] = strings
    struct.pack_into(order + ("Q" if bits == 64 else "I"), data, 40 if bits == 64 else 32, 0x300)
    struct.pack_into(order + "HHH", data, 58 if bits == 64 else 46, stride, 7, 3)

    def section(index, name, kind, flags, address, offset, size, link=0, entry_size=0):
        struct.pack_into(order + ("IIQQQQIIQQ" if bits == 64 else "IIIIIIIIII"), data, 0x300 + index * stride,
                         name, kind, flags, address, offset, size, link, 0, 1, entry_size)

    section(1, 1, 1, 6, 0x400100, 0x100, 8)
    section(2, 7, 1, 2, 0x400200, 0x200, 32)
    section(3, 15, 3, 0, 0, 0x280, len(names))
    section(4, 0, 2, 0, 0, 0x500, symbol_stride * 4, 5, symbol_stride)
    section(5, 0, 3, 0, 0, 0x700, len(strings))
    section(6, 0, 11, 0, 0, 0x600, symbol_stride * 4, 5, symbol_stride)
    for offset in [0x500, 0x600]:
        for index, (label, value, size, info, target) in enumerate([
                ("named_entry", 0x400100, 8, 0x12, 1),
                ("missing_api", 0, 0, 0x22, 0),
                ("tls_slot", 0, 8, 0x16, 2)], 1):
            name = strings.index(label.encode())
            if bits == 64:
                struct.pack_into(order + "IBBHQQ", data, offset + index * symbol_stride, name, info, 0, target, value, size)
            else:
                struct.pack_into(order + "IIIBBH", data, offset + index * symbol_stride, name, value, size, info, 0, target)
    return data


def elf_object():
    """Minimal ELF64 ET_REL with one executable section and one function."""
    data = bytearray(0x500)
    data[:16] = b"\x7fELF" + bytes([2, 1, 1]) + bytes(9)
    struct.pack_into("<HHIQQQIHHHHHH", data, 16, 1, 62, 1, 0, 0, 0x300, 0, 64, 0, 0, 64, 6, 4)
    data[0x100:0x108] = b"\x55\x48\x89\xe5\x5d\xc3\x90\x90"
    strings = b"\0object_entry\0"
    section_names = b"\0.text\0.symtab\0.strtab\0.shstrtab\0.rela.text\0"
    data[0x200:0x200 + len(strings)] = strings
    data[0x240:0x240 + len(section_names)] = section_names

    def section(index, name, kind, flags, offset, size, link=0, info=0, alignment=1, entry_size=0):
        struct.pack_into("<IIQQQQIIQQ", data, 0x300 + index * 64,
                         name, kind, flags, 0, offset, size, link, info, alignment, entry_size)

    section(1, section_names.index(b".text"), 1, 6, 0x100, 8, alignment=4)
    section(2, section_names.index(b".symtab"), 2, 0, 0x180, 48, link=3, info=1, alignment=8, entry_size=24)
    section(3, section_names.index(b".strtab"), 3, 0, 0x200, len(strings))
    section(4, section_names.index(b".shstrtab"), 3, 0, 0x240, len(section_names))
    section(5, section_names.index(b".rela.text"), 4, 0, 0x220, 24, link=2, info=1, alignment=8, entry_size=24)
    struct.pack_into("<IBBHQQ", data, 0x180 + 24, strings.index(b"object_entry"), 0x12, 0, 1, 0, 8)
    struct.pack_into("<QQq", data, 0x220, 1, (1 << 32) | 2, -4)
    return data


def elf_sectionless_dynamic(gnu_hash=False):
    """Section-header-free ELF64 AArch64 ET_DYN with PT_DYNAMIC-derived evidence."""
    data = bytearray(0x400)
    data[:16] = b"\x7fELF" + bytes([2, 1, 1]) + bytes(9)
    base = 0x400000
    order = "<"
    struct.pack_into(order + "HHIQQQIHHHHHH", data, 16, 3, 183, 1,
                     base + 0x100, 64, 0, 0, 64, 56, 2, 64, 0, 0)
    data[0x100:0x108] = bytes.fromhex("1f2003d5c0035fd6")
    struct.pack_into(order + "IIQQQQQQ", data, 64, 1, 5, 0, base, 0, 0x400, 0x400, 0x1000)
    struct.pack_into(order + "IIQQQQQQ", data, 120, 2, 6, 0x300, base + 0x300, 0, 0x90, 0x90, 8)
    strings = b"\0main\0gflag\0_puts\0"
    data[0x260:0x260 + len(strings)] = strings
    struct.pack_into(order + "IBBHQQ", data, 0x200, 0, 0, 0, 0, 0, 0)
    struct.pack_into(order + "IBBHQQ", data, 0x200 + 24, strings.index(b"main"), 0x12, 0, 1, base + 0x100, 8)
    struct.pack_into(order + "IBBHQQ", data, 0x200 + 48, strings.index(b"gflag"), 0x11, 0, 1, base + 0x3f0, 4)
    struct.pack_into(order + "IBBHQQ", data, 0x200 + 72, strings.index(b"_puts"), 0x12, 0, 0, 0, 0)
    if gnu_hash:
        struct.pack_into(order + "IIII", data, 0x280, 1, 1, 1, 0)
        struct.pack_into(order + "Q", data, 0x290, 0)
        struct.pack_into(order + "I", data, 0x298, 1)
        struct.pack_into(order + "III", data, 0x29c, 2, 4, 1)
    else:
        struct.pack_into(order + "II", data, 0x280, 1, 4)
        struct.pack_into(order + "IIII", data, 0x288, 0, 0, 0, 0)
    dyn_tag = 0x6ffffef5 if gnu_hash else 4
    entries = [(7, base + 0x3a0), (8, 48), (9, 24), (dyn_tag, base + 0x280),
               (6, base + 0x200), (5, base + 0x260), (10, len(strings)), (11, 24), (0, 0)]
    for i, (tag, val) in enumerate(entries):
        struct.pack_into(order + "QQ", data, 0x300 + i * 16, tag, val)
    struct.pack_into(order + "QQq", data, 0x3a0, base + 0x3d8, (1 << 32) | 257, -4)
    struct.pack_into(order + "QQq", data, 0x3b8, base + 0x3e0, (0 << 32) | 1002, base + 0x100)
    return data


def elf_sectionless_dynamic32():
    """Section-header-free ELF32 i386 ET_DYN with DT_REL-derived evidence."""
    data = bytearray(0x400)
    data[:16] = b"\x7fELF" + bytes([1, 1, 1]) + bytes(9)
    base = 0x400000
    order = "<"
    struct.pack_into(order + "HHIIIIIHHHHHH", data, 16, 3, 3, 1, base + 0x100, 52, 0, 0, 52, 32, 2, 40, 0, 0)
    data[0x100:0x108] = b"\x55\x48\x89\xe5\x5d\xc3\x90\x90"
    struct.pack_into(order + "IIIIIIII", data, 52, 1, 0, base, 0, 0x400, 0x400, 5, 0x1000)
    struct.pack_into(order + "IIIIIIII", data, 84, 2, 0x300, base + 0x300, 0, 0x20, 0x20, 6, 4)
    entries = [(17, base + 0x380), (18, 16), (19, 8), (0, 0)]
    for i, (tag, val) in enumerate(entries):
        struct.pack_into(order + "II", data, 0x300 + i * 8, tag, val)
    struct.pack_into(order + "II", data, 0x380, base + 0x3c0, (1 << 8) | 8)
    struct.pack_into(order + "II", data, 0x388, base + 0x3c8, (0 << 8) | 2)
    return data


def run(*args, env=None):
    reply = subprocess.run([WORKER, *map(str, args)], capture_output=True, timeout=60, env=env)
    if reply.returncode != 0:
        raise RuntimeError(f"worker exited {reply.returncode}: {reply.stderr.decode(errors='replace')}")
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
        for be in [False, True]:
            result, _ = analyze(elf_symbols(bits, be))
            assert result["kind"] == "analysis_result", result
            assert result["symbol_count"] == 8, result["symbol_count"]
            assert "name=named_entry source=ELF static-symbol" in result["functions"], result["functions"]
            assert "missing_api" in result["dyld_imports"] and "binding=2" in result["dyld_imports"]
            assert "named_entry" in result["export_trie"]
            assert "name=tls_slot value=0x0" in result["symbol_provenance"]
            assert "0x0000000000000000 name=tls_slot" not in result["symbol_provenance"]
            assert "missing_api" not in result["functions"] and "tls_slot" not in result["functions"]
    result, args = analyze(elf_object(), "elf")
    assert result["kind"] == "analysis_result", result
    assert result["analyzed_instructions"] > 0
    assert "0x0000000000010000 name=object_entry source=ELF static-symbol" in result["functions"], result["functions"]
    assert "address=analysis-only-section-rebase" in result["symbol_provenance"]
    assert "deterministic analysis-only addresses" in result["metadata"]
    assert "0x0000000000010001 ELF relocation type=2 symbol-index=1 addend=-4 target-section=1 source=SHT_RELA" in result["relocations"]
    page = run("--page-image", *args[:3], "0", args[3], "elf", "auto", "0", "0")
    assert page["kind"] == "page_result" and int(page["section_address"]) == 0x10000, page

    # Section-header-free ELF images recover dynamic-link evidence from PT_DYNAMIC.
    for gnu in [False, True]:
        result, args = analyze(elf_sectionless_dynamic(gnu_hash=gnu), "elf")
        assert result["kind"] == "analysis_result", result
        assert result["analyzed_instructions"] > 0, result
        assert f"0x{0x400100:016x}" in result["functions"], result["functions"]
        assert "0x00000000004003d8 ELF relocation type=257 symbol-index=1 addend=-4 source=DT_RELA" in result["relocations"], result["relocations"]
        assert "0x00000000004003e0 ELF relocation type=1002 symbol-index=0 addend=4194560 source=DT_RELA" in result["relocations"], result["relocations"]
        assert "0x0000000000400100 name=main source=ELF dynamic-symbol type=2 binding=1 visibility=0 section=1" in result["symbol_provenance"], result["symbol_provenance"]
        assert "name=_puts undefined source=ELF dynamic-symbol type=2 binding=1 visibility=0" in result["dyld_imports"], result["dyld_imports"]
        assert "0x00000000004003f0 name=gflag source=ELF dynamic-symbol type=1 binding=1 visibility=0 section=1" in result["symbol_provenance"], result["symbol_provenance"]
        assert result["symbol_count"] == 4, result["symbol_count"]
        assert "main" in result["export_trie"], result["export_trie"]
        assert "ELF PT_DYNAMIC VA=0x400300" in result["metadata"], result["metadata"]
        expected_hash = "DT_GNU_HASH proven symbol count=4" if gnu else "DT_HASH proven symbol count=4"
        assert expected_hash in result["metadata"], result["metadata"]
        assert "PT_DYNAMIC-derived dynamic-link evidence" in result["metadata"], result["metadata"]
        assert "ELF dynamic relocation records=2; references remain unapplied." in result["metadata"], result["metadata"]
    result, _ = analyze(elf_sectionless_dynamic32(), "elf")
    assert result["kind"] == "analysis_result", result
    assert result["analyzed_instructions"] > 0, result
    assert f"0x{0x400100:016x}" in result["functions"], result["functions"]
    assert "0x00000000004003c0 ELF relocation type=8 symbol-index=1 addend=0 source=DT_REL" in result["relocations"], result["relocations"]
    assert "0x00000000004003c8 ELF relocation type=2 symbol-index=0 addend=0 source=DT_REL" in result["relocations"], result["relocations"]
    assert "DT_HASH and DT_GNU_HASH are absent" in result["metadata"], result["metadata"]
    assert result["symbol_count"] == 0, result["symbol_count"]

    # Malformed sectionless dynamic-link inputs fail closed.
    data = elf_sectionless_dynamic()
    malformed = []
    for offset, value in [(0x308, 0x40800000), (0x328, 20), (0x318, 50), (0x3a0, 0x408000), (0x3a8, (7 << 32) | 257)]:
        copy = bytearray(data)
        struct.pack_into("<Q", copy, offset, value)
        malformed.append(bytes(copy))
    copy = bytearray(data)
    struct.pack_into("<H", copy, 72, 3)
    struct.pack_into("<IIQQQQQQ", copy, 0xb0, 2, 6, 0x300, 0x400300, 0, 0x90, 0x90, 8)
    malformed.append(bytes(copy))
    copy = bytearray(data)
    struct.pack_into("<Q", copy, 0x380, 21)
    malformed.append(bytes(copy))
    copy = bytearray(data)
    struct.pack_into("<Q", copy, 0x80, 0x250)
    malformed.append(bytes(copy))
    copy = bytearray(data)
    struct.pack_into("<I", copy, 0x284, 0)
    malformed.append(bytes(copy))
    for item in malformed:
        result, _ = analyze(item, "elf")
        assert result["kind"] == "error", result
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
    # ELF symbol metadata cannot invent addresses or escape section/string bounds.
    for offset, kind, value in [
        (0x300 + 4 * 64 + 40, "I", 99),
        (0x300 + 4 * 64 + 56, "Q", 16),
        (0x500 + 24, "I", 0xffff),
        (0x500 + 24 + 6, "H", 99),
        (0x500 + 24 + 6, "H", 0xffff),
        (0x500 + 24 + 8, "Q", 0x500000),
        (0x500 + 24 + 16, "Q", 0xffffffffffffffff),
    ]:
        data = elf_symbols()
        struct.pack_into("<" + kind, data, offset, value)
        result, _ = analyze(data)
        assert result["kind"] == "error", result
    for offset, kind, value in [
        (0x220, "Q", 8),
        (0x228, "Q", (99 << 32) | 2),
        (0x440 + 44, "I", 3),
        (0x440 + 56, "Q", 16),
    ]:
        data = elf_object()
        struct.pack_into("<" + kind, data, offset, value)
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
    result, args = analyze(elf_symbols())
    names.write_text("# source_sha256=" + hashlib.sha256(elf_symbols()).hexdigest() + "\n0x400100 lower_priority_name\n")
    result = run("--analyze-image", *args, "elf", "auto", "0", "0", env=env)
    assert "name=named_entry source=ELF static-symbol" in result["functions"]
    assert "lower_priority_name" not in result["functions"]

print("ELF/PE capture, mappings, decode, evidence, paging, byte order, embedded offsets, malformed-input and sectionless dynamic-link checks passed")
