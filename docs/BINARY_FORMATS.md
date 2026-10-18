# ELF and PE static projects

Import Binary accepts Auto, ELF (static), and PE/COFF image (static). Imports
capture immutable bytes and retain the selected format, architecture and image
offset in the existing project schema. Header addresses determine the preferred
address space; a nonzero raw load-address override is rejected. Embedded images
can be selected using an explicit file offset.

ELF relocatable objects and executable/shared images support i386, x86-64 and
AArch64, in 32/64-bit classes with matching machine widths. The parser validates
program headers, load extents, section-to-load mapping consistency, names and
entry points. Relocatable `SHF_ALLOC` sections receive deterministic,
nonoverlapping analysis-only addresses, and defined symbols are rebased from
section-relative values without claiming loader placement or applying relocations.
Section-backed SHT_SYMTAB/SHT_DYNSYM records supply names, function starts,
undefined imports and visible dynamic exports with table/binding/visibility
provenance. Only STT_FUNC records in validated executable sections become
function starts. TLS values remain offsets; undefined, absolute and special
section symbols do not invent mapped addresses. Native symbol names outrank
external catalogs. Symbol parsing has a 200,000-record, 16 MiB table-read and
1 MiB aggregate-name budget. Multi-architecture cache identity advances to v2
so old structural reports are reanalyzed.

Section-backed `SHT_REL` and `SHT_RELA` tables are validated against their
linked symbol table and target section. Records expose the relocation site,
type, symbol index, explicit addend, and target-section provenance under a
200,000-record/16 MiB budget. Object relocation sites use the analysis-only
section mapping. Relocations are evidence only and are never applied.

Section-header-free images additionally recover dynamic-link evidence from
`PT_DYNAMIC`: `DT_RELA`/`DT_REL`/`DT_JMPREL` relocation tables are resolved
through load mappings with validated entry sizes, sites, symbol indices, and
addends; symbol indices are bounded by the `DT_HASH` chain count when present,
or by the `DT_GNU_HASH` bucket/chain structure. When `DT_HASH` or
`DT_GNU_HASH` and the dynamic string table are proven, `DT_SYMTAB` records
supply defined dynamic symbols, function starts, undefined imports and visible
exports with table/binding/visibility provenance; a missing hash table leaves
symbol counts and names unrecovered and relocation symbol indices transcribed
as raw evidence. `DT_RELR` records are noted but remain unapplied. The same
200,000-record/16 MiB budgets apply. Relocations remain evidence only and are
never applied.

Images without section tables use PT_LOAD mappings and, when present, recover
dynamic-link evidence from `PT_DYNAMIC` (`DT_RELA`/`DT_REL`/`DT_JMPREL`
relocation records, `DT_HASH`/`DT_GNU_HASH`-proven symbol counts, and
`DT_SYMTAB` names, imports, exports and function starts). Big-endian headers can
produce metadata, while instruction decoding remains disabled. Extended section
numbering, core files and other architectures are rejected.

PE32/PE32+ images support i386, x86-64 and AArch64. The parser validates the DOS
and PE signatures, COFF and optional headers, directory-table framing, alignments,
section extents, preferred image base and entry point. File alignment padding is
excluded from code, and virtual zero-fill tails are separate from captured bytes.
PE imports, exports and base relocations are recovered from data directories as
bounded evidence. Authenticode (PKCS#7/CMS) security directories are validated
structurally and reported as evidence.

Standalone COFF objects (`.obj`) are admitted for i386, x86-64 and AArch64 with
bounded section, symbol and relocation tables. Section contents receive
deterministic analysis-only addresses, symbols supply names, function starts,
undefined imports and visible exports, and relocations remain unapplied evidence.

PDB files (MSF 7.0) are admitted with validated superblock, block map and stream
directory. Stream count, sizes and well-known streams (PDB info, TPI, DBI) are
reported as evidence; contents remain uninterpreted.

Both parsers reject overlapping mappings, overflowing addresses, out-of-file
tables, architecture mismatches and entries outside file-backed executable
mappings. ELF tables have a 256-entry limit; PE has a 96-section limit. Existing
4 MiB scan and 60,000-byte evidence-view budgets continue to apply.

The structural backend supplies instruction previews, selected-section CFG and
direct-call candidates, strings, pointer candidates, address-bearing evidence,
and SHA-256-verified instruction paging. It does not apply relocations, recover
PE symbols, provide semantic decompilation, or execute/attach to Linux or
Windows processes. PE imports, exports and base relocations are recovered as
bounded evidence, as are sectionless ELF dynamic symbols or relocations. PIE and ASLR addresses remain
preferred image addresses until runtime mapping evidence exists. Decoding uses
the existing LLVM availability requirements.

Run `python3 scripts/test-elf-pe.py rust/target/debug/reworker` after building
the worker. The portable release gate runs this suite alongside existing
Mach-O/firmware format checks. It tests capture, analysis, paging, byte order,
32/64-bit layouts, AArch64, relocatable objects, sectionless images with dynamic-link evidence, embedded offsets, malformed
ranges, overlaps, architecture mismatches and changed-source rejection.

Layout references: [ELF ABI header specification](https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.eheader.html)
and [Microsoft PE/COFF specification](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format).
