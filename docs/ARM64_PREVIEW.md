# ARM64 preview contract

The import pipeline is: exclusive streamed capture → selected Mach-O layout checks → single executable `__text` selection → bounded instruction read from the captured file → limited decoding → lease check → object publication → record commit.

The selector only accepts little-endian CPU_TYPE_ARM64 Mach-O executables, dylibs and bundles. It requires a nonempty regular instruction-marked `__text` in an executable segment, consistent VA/file mapping, four-byte alignment, no overlap with the load-command region and no competing/overlapping selected mapping. Encryption metadata and data-in-code payloads block the preview. The original layout checker does not validate every command; this contract is intentionally narrower than executable validity.

The decoder accepts a 32-bit word and 64-bit image address, writes bounded text, and returns decoded / unsupported / failure. Supported families: NOP; RET/BR/BLR; B/BL; B.cond; CBZ/CBNZ; TBZ/TBNZ; ADR/ADRP; MOVN/MOVZ/MOVK; immediate ADD/ADDS/SUB/SUBS. Version 2 adds the integer families listed below. Register 31 follows each supported form's SP/ZR rules. PC-relative calculations use checked addition/subtraction. Target overflow remains unsupported instead of wrapping or fabricating an address.

Read at most 1 KiB of selected code (256 words); the Mach-O command window is separately capped at 16 MiB plus header. Rows preserve raw words and both address domains. Unknown words remain `.inst 0x…`; this does not imply an invalid instruction. Canonical forms may differ from aliases printed by other tools. No PAC stripping, ASLR slide, relocation, symbol resolution or code/data proof occurs.

The small decoder is an incremental backend behind a narrow C API called by Rust. A pinned full-featured decoder remains future work. LLVM is used only as a test oracle, not dynamically discovered by production code.

Reference material: [LLVM AArch64 format definitions](https://github.com/llvm/llvm-project/blob/main/llvm/lib/Target/AArch64/AArch64InstrFormats.td), [Arm A64 overview](https://developer.arm.com/-/media/Files/pdf/graphics-and-multimedia/ARMv8_InstructionSetOverview.pdf), and [Apple Mach-O loader definitions](https://github.com/apple-oss-distributions/xnu/blob/main/EXTERNAL_HEADERS/mach-o/loader.h). No external decoder source is vendored.


## On-demand pages in 0.5.0

Paging uses the existing captured object's full digest and size. `re_page_range` validates the complete section extent and a zero-based start word, then returns file offset, image VA and min(256, remaining words). The worker rehashes the opened descriptor, reads a fresh bounded command window, computes the mapping and reads only the selected code words. File identity/size/timestamps are compared over the operation.

The UI retains one page and checks request ID, digest, start index, page count, address extents, selection generation and project path before displaying it. Pages are never written into the project database. Changing selection clears the transient page. A failed request leaves the last verified page visible as such.


## Cache in 0.6.0

The coordinator keeps an LRU of serialized verified pages (16 entries, 1 MiB retained entry/key/value accounting). Keys include format version, resolved object path, source digest, expected size, start word and device/inode/size/mtime/ctime stamp. Cache lookup uses exact binary keys, including lengths. Value pointers are borrowed only until the next mutation; Swift immediately copies bytes before decoding them.

Cache hits do not read current file contents or establish a fresh hash. File stamps are checked before and after lookup. Metadata changes miss the key; project refresh/switch/disconnect and forced verification clear the cache. Cache allocation/insertion failures leave paging functional through ordinary worker requests. No cache result is written to SQLite.

A fresh worker response is cached only after normal frame/identity validation, process exit, successful lease publication and a final metadata comparison. Cached pages get the current request ID and are revalidated before reply. The UI's existing request/selection-generation checks remain in force.


## A64 subset v2 coverage

| Family | Implemented forms | Deliberate exclusions |
|---|---|---|
| Integer single-register memory | STR/LDR byte, halfword, W/X; signed loads; unsigned scaled, signed unscaled and pre/post-index offsets | SIMD/FP, register-offset, unprivileged, atomic and prefetch forms |
| Integer register pairs | STP/LDP and STNP/LDNP for W/X; signed scaled offsets and applicable pre/post indexing | LDPSW, SIMD/FP pairs, duplicate load destinations and overlapping writeback registers |
| Integer literals | W/X LDR and LDRSW with checked PC-relative targets | SIMD literals, prefetch and target overflow |
| Register arithmetic | Shifted ADD/ADDS/SUB/SUBS with LSL/LSR/ASR | Extended-register forms and reserved shifts |
| Register logic | AND/ORR/EOR/ANDS and BIC/ORN/EON/BICS with valid shifts | Logical immediates and invalid 32-bit shift amounts |

Loads/stores print effective-address expressions without accessing memory. Sign extension is reflected by mnemonic and destination width, not simulated. Base register 31 denotes SP in these memory forms; data register 31 denotes ZR. Shifted arithmetic uses ZR, not SP. Constrained-unpredictable cases are excluded rather than assigned guessed behavior. `.inst` therefore means outside decoder coverage/policy, not necessarily architecturally invalid.

Decoder identity appears in the report footer and cache key. The bundled synthetic fixture gives a compact import/mapping/decoding demonstration. Full semantic lifting and a complete decoder backend remain separate work.
