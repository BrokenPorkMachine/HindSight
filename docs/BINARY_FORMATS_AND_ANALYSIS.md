# Firmware and multi-architecture static analysis

HindSight Research Tool imports captured, immutable bytes. Import configuration is persisted with each artifact and participates in page and analysis cache identity. Existing records without import options use automatic format selection when paged or analyzed. Configuration-aware cache keys cause their earlier reports to be recomputed with frame-pattern evidence. Continuity snapshots record the import interpretation, too. SHA-256 verification, regular-file/symlink checks, metadata continuity, bounded worker admission and cancellation apply to the new backends.

## Coverage

| Input | Loading and instruction display | Analysis |
| --- | --- | --- |
| Little-endian thin Mach-O 32, Intel i386 | Validated 28-byte header, 32-bit segments/sections, variable-length LLVM decode | Named functions, metadata, strings, relocation/import/export tables, pointers, direct transfers, structural CFG, frame/return candidates |
| Little-endian thin Mach-O 64, Intel x86_64 | Validated 32-byte header, 64-bit segments/sections, variable-length LLVM decode | Same structural evidence, plus supported RIP-relative data/string references |
| Little-endian thin Mach-O 64, AArch64 | LLVM display; original ARM64 analysis remains active for ordinary executable/dylib/bundle imports | Existing ARM64 metadata/linkage/runtime/SSA evidence, plus frame/authentication/return pattern candidates |
| ARM32 Mach-O / SEPI composite | ARM or Thumb-2 mode; inventory of validated embedded headers in the first 4 MiB; explicit image-offset selection | Structural evidence, metadata-backed names when present, Thumb-tagged pointers and bounded table walks |
| Universal FAT32/FAT64, either table byte order | Checked slice extents/alignment, overlap rejection, CPU/header agreement; select requested architecture or prefer AArch64, then x86_64, i386, ARM32 | Structural backend for the selected slice |
| Plaintext SecureROM/iBoot/raw firmware | Explicit architecture, file offset and raw load address | Linear instruction decode, direct-transfer and frame/return candidates, bounded raw string scan; headerless bytes have no verified code/data partition |
| Big-endian thin Mach-O with a supported CPU | Header/section/symbol/string/pointer metadata | Instruction decode and relocation-bitfield interpretation unavailable; never decoded with little-endian semantics |
| IMG3/IMG4/IM4P and IPSW/ZIP marker-bearing input | Bounded container/codec inventory; stored and deflated IPSW entries are selected from local headers or the central directory, including data-descriptor archives; legacy ZipCrypto entries can be decrypted with an explicit password | On macOS, bounded plaintext candidates are decoded with Compression.framework (LZFSE/LZ4/LZMA/zlib); `bvxn`/LZVN and legacy `complzss`/LZSS are decoded through bounded Rust paths. AES ZIP encryption and authenticated IMG4 envelopes remain unavailable without credentials/keys. A decoded payload is analyzed while the original artifact SHA-256 remains authoritative. |

The loader accepts supported file types with a validated file-backed instruction section, including ELF relocatable objects. ELF object sections receive deterministic analysis-only addresses and their defined symbols are rebased from section-relative values; loader placement and relocation application are not implied. Instruction paging and CFG currently use one primary executable section (`__text` or `.text` when present); metadata/table/string walkers inspect other mapped sections.

## Importing the supplied SEPI

Choose `/Users/failbr34k/aapl/sepi.decrypted.bin`, select **SEPOS / SEPI**, and leave architecture **From header**. Auto inventories and selects the first validated Mach-O at file offset `0x1000`. Import again with offset `0x94000` to select the second image. The verified embedded offsets are:

`0x1000`, `0x94000`, `0xb9000`, `0xdb000`, `0xf5000`, `0x103000`, `0x163000`, `0x173000`, `0x255000`.

All nine images passed header/section loading and bounded structural analysis locally. The first and second images begin with valid Thumb-2 prologues. For ARM32, Thumb-2 is an explicit import default, not a guarantee derived from section flags. Override **ARM32** or **Thumb-2** when the instruction mode differs. Mixed-mode control flow is not automatically reconstructed. Symbols that carry `N_ARM_THUMB_DEF` and Thumb-tagged pointers retain that provenance.

For a raw SecureROM/iBoot file, choose the architecture and its known load address. Address `0` gives an explicitly analyst-supplied file-relative mapping. A filename or an `iBoot`/`SEPOS` string does not establish plaintext, instruction mode or a hardware load address. No proprietary firmware bytes are included in the source or tests.

## Implemented table and function evidence

- Mach-O headers, load-command framing, segment/section bounds, nonoverlapping mappings, encrypted-code state and selected linkedit extents.
- Legacy ZipCrypto decryption is opt-in through `HINDSIGHT_ZIP_PASSWORD`; the bounded path validates the encryption header and never logs the password. AES-encrypted ZIP entries remain inventory-only.
- Bounded DER/ASN.1 TLV parsing for IMG4 candidates: definite lengths, high-tag numbers, constructed nesting, depth/node caps, and malformed/truncated rejection. This establishes syntax and offsets, not Apple's authenticated IMG4 schema or signature validity.
- `LC_SYMTAB` 32/64-bit nlist names with type, section and descriptor provenance; malformed name/section indices are errors.
- `LC_FUNCTION_STARTS` checked ULEB128 walks; `LC_MAIN` and supported architecture thread-state entry addresses.
- Export-trie names and mapped executable exports, with cycle/depth/node/size bounds; reexports remain name/ordinal records.
- `LC_DYSYMTAB` indirect-symbol tables, pointer/stub widths and section table indices; ordinary relocation records and validated symbol indices. Scattered relocations are reported as metadata.
- ASCII and UTF-16LE ASCII-range strings with original file offsets and section addresses. Unicode outside that range is not decoded yet.
- Aligned pointer walks into validated sections, Thumb-bit normalization and runs of at least three code pointers reported as table candidates. Those tables do not create CFG dispatch edges without evidence of use.
- Supported architecture direct-call/branch targets and bounded basic-block structure. Unknown instructions and data-in-code are barriers. Supported PC/literal/RIP-relative references are resolved through mapped sections; printable data targets become string-reference evidence.
- Intel frame setup/leave/return patterns, ARM/Thumb stack push/pop returns, AArch64 frame pair saves/restores and pointer-authentication frame hints. These are candidate boundaries; optimized leaf functions need not have frame patterns.
- Apple/XNU/SEP name candidates from bounded kernel strings near discovered starts. C++-mangled, `Apple*`, `IO*`, `mach_*`, `vm_*`, `bsd_*`, panic/assert and related names retain `source=nearby-kernel-string`; they are heuristic navigation labels and never outrank nlist, export-trie or `LC_FUNCTION_STARTS` evidence.
- Optional analyst-supplied XNU/SEP address catalogs through `HINDSIGHT_SYMBOL_DB`; validated executable-section addresses receive `source=external-symbol-db`, while nlist and export-trie names retain precedence. See `docs/SYMBOL_DATABASE_FORMAT.md`.

Exact nlist names take precedence over synthesized `sub_<address>` names. Direct-call targets, frame patterns and section starts retain candidate labels. Function extents, indirect transfers and pointer-table use are not invented from naming or byte-pattern evidence.

## Decoder dependency and limits

Instruction display uses the documented [LLVM C disassembler API](https://llvm.org/doxygen/group__LLVMCDisassembler.html), loaded from installed Xcode, Command Line Tools or LLVM. The project does not redistribute Apple's toolchain library. Without a compatible library, new-format imports still show checked metadata and an actionable decoder message; instruction requests report the missing dependency. The original ARM64 subset decoder remains available through its existing worker commands.

Load-command reads are limited to 16 MiB, embedded-image scans to 4 MiB, 512 candidate headers and 128 images, sections to 256 and segments to 64. Symbols and indirect-symbol tables each allow 200,000 entries; decoded symbol names have an aggregate 16 MiB budget. Structural instruction scans allow 4 MiB / 262,144 instructions; string and pointer walks each allow 4 MiB. Text views are capped at 60,000 bytes and can display fewer rows than the full bounded counts. Paging uses 4-byte units for AArch64/ARM, 2-byte units for Thumb and byte units for Intel. Next/Previous preserve observed variable-length page starts; arbitrary address jumps start a fresh linear decode and are not proof of an existing instruction boundary.

Mach-O structure definitions follow Apple's [loader header](https://github.com/apple-oss-distributions/xnu/blob/main/EXTERNAL_HEADERS/mach-o/loader.h). Unsupported command payloads and signatures are not validated by this loader.

## Remaining commercial-disassembler gaps

This change supplies bounded Apple-container/codec ingestion and conservative stripped-kernel name candidates; it does not establish parity with mature commercial products. Intel/ARM32 semantic lifting, SSA and high-level decompilation; automatic mixed ARM/Thumb transitions; unwind/DWARF-driven boundary recovery; validated indirect-dispatch reconstruction; interactive multi-section selection; complete Unicode decoding; AES ZIP encryption; Apple-specific IMG4 schema interpretation; authenticated IMG4 signature/decryption; and complete private XNU/SEP symbol databases remain outstanding. Existing ARM64 semantic/runtime features continue on their established path, with their documented bounds.

## Validation

`python3 scripts/test-binary-formats.py <worker>` exercises synthetic 32/64-bit Mach-O, FAT32/FAT64, embedded composites, raw mappings, architecture mismatch, malformed commands/symbols/export tries, strings, named functions, pointer-table candidates, entry/function-start/data-in-code tables, relative data references and variable-length page continuity. No Apple firmware is needed for these tests. The Swift page/cache contracts and existing native/worker tests are exercised by `scripts/test-macos.sh`. The aggregate portable script currently stops at pre-existing Clippy warnings in the legacy bridge/ARM64 worker; those lints are not suppressed by this change. Individual source, Rust, worker, Swift and build gates were run separately. Full Xcode builds verify the embedded universal app/service/worker artifacts.
