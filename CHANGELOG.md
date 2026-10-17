# HindSight Research Tool — Project recreation

- Rename the Xcode project, workspace, app target, scheme, app entry point, and bridge header to HindSight; keep the application bundle named HindSight.app.
- Use HindSight Research Tool as the window and display title; preserve stored projects, service identities, and ABI compatibility.
- Add the hindsight scripting module while retaining vectorglass and reworkbench import aliases.
- Disable VectorEmu release symbol stripping to avoid malformed host proc-macro dylibs rejected by macOS 27.
- Bound SwiftUI workspace type expansion to fix the reproduced Swift 6.4 SIL-generation crash in ContentView.
- Regenerate consistent build settings and add clean, commit-based source recreation without Git metadata, DerivedData, or Xcode user state.

# 0.5.11 — Evidence Graph Navigation and Analysis Cache Control

- Make Evidence Graph minimap navigation match Flow Graph with a draggable viewport rectangle and click-to-jump behavior.
- Preserve graph geometry while panning and make minimap clicks jump every graph to the selected location.
- Show function and symbol names wherever function information is presented, including runtime/evidence details and image context.
- Add bounded project-local analysis-cache clearing from the analysis toolbar.
- Replace the remaining SwiftUI `Form` in Settings and document the `List`/`NSOutlineView` live-resize AttributeGraph crash; keep the workbench on `ScrollView` + `LazyVStack` and guard against `List`/`OutlineGroup`/`Form` reintroduction.
- Extend the ARM64 semantic preview to the full move-wide immediate family (`MOVN`, `MOVZ`, `MOVK`), so proven 64-bit constant materialization carries instruction provenance; `MOVK` claims a constant only when the prior value is known. No report field or engine-identity change.
- Extend the ARM64 semantic preview to flag-neutral logical operations: `AND`/`ORR`/`EOR` bitmask immediates and unshifted register `AND`/`ORR`/`EOR` plus the negated `BIC`/`ORN`/`EON`, folding known constants and complements with instruction provenance. Reserved bitmask encodings, `ANDS`/`BICS`, and shifted register operands stay opaque. No report field or engine-identity change.
- Extend the ARM64 semantic preview to PC-relative `ADR`/`ADRP` address materialization, so `adrp`/`add` pointer constants carry instruction provenance. Address values are unslid and no source-level type is inferred. No report field or engine-identity change.
- Extend the ARM64 semantic preview to conditional and indirect control flow (`B.cond`, `CBZ`/`CBNZ`, `TBZ`/`TBNZ`, `BR`/`BLR`): condition codes, register zero/nonzero tests, single-bit tests, and unresolved indirect targets are rendered, and every such transfer clears tracked constants like any other barrier. No PSTATE-flag or indirect-target value recovery, and no report field or engine-identity change.
- Resolve `BR`/`BLR` indirect targets in the ARM64 semantic preview when the register holds a proven constant, printing the target address with its instruction provenance; untracked registers stay named as unresolved. No report field or engine-identity change.
- Extend the ARM64 semantic preview to immediate shifts (`LSL`/`LSR`/`ASR`, the canonical `UBFM`/`SBFM` aliases), folding known constants with sign-propagating arithmetic shift; bitfield inserts/extracts and the sign/zero extends stay opaque. No report field or engine-identity change.
- Extend the ARM64 semantic preview to multiply-add (`MADD`/`MSUB`, with the `MUL`/`MNEG` zero-addend aliases) with 32/64-bit constant folding; the wider `SMADDL`/`UMADDL`/... variants stay opaque. No report field or engine-identity change.
- Extend the ARM64 semantic preview to the sign/zero extend aliases (`UXTB`/`UXTH`/`SXTB`/`SXTH`/`SXTW`), folding width-correct signed and zero extensions with 32/64-bit destination widths; general bitfield extracts and inserts stay opaque. No report field or engine-identity change.
- Extend the ARM64 semantic preview to `LSL`/`LSR`/`ASR`-shifted register operands on the modeled `ADD`/`SUB` and logical operations, applying the shift before the enclosing operation and folding it into known constants; `ROR` and flag-setting variants stay opaque. No report field or engine-identity change.
- Extend the ARM64 semantic preview to the general bitfield extracts (`UBFX`/`SBFX`), rendering `dst = (src >> lsb) & mask;` and `dst = sext(src >> lsb, width);` for the non-wrapping field `[lsb, lsb + width)` and folding known constants with instruction provenance; `BFI`/`BFXIL`/`UBFIZ`/`SBFIZ` inserts stay opaque. No report field or engine-identity change.
- Extend the ARM64 semantic preview to 32/64-bit general-register loads and stores (`LDR`/`STR` with a scaled unsigned immediate offset), rendering `dst = *(base + off);` and `*(base + off) = src;`, resolving the address with provenance when the base is a proven constant, and treating a load as an unknown barrier for its destination while a store leaves tracked registers unchanged; byte/halfword, sign-extending, prefetch, scaled-FP, unscaled, and indexed forms stay opaque. No report field or engine-identity change.
- Extend the ARM64 semantic preview to the byte/halfword and sign-extending general-register loads and stores (`STRB`/`STRH`/`LDRB`/`LDRH`/`LDRSB`/`LDRSH`/`LDRSW` with a scaled unsigned immediate offset), naming the exact access width and signedness in the dereference when it differs from the data-register width (`w6 = *(u8 *)(x7);`, `x20 = *(s32 *)(x21);`) and keeping the whole-register forms untyped; prefetch, the unallocated `opc 3` 32/64-bit forms, scaled-FP, unscaled, and indexed forms stay opaque. No report field or engine-identity change.
- Extend the ARM64 semantic preview to register-offset single-register loads and stores (`LDR`/`STR` and their byte/halfword/sign-extending forms with `[base, index, extend/shift]` addressing), rendering the index expression with the appropriate extension (`UXTW`/`LSL`/`SXTW`/`SXTX`) and optional size scaling, and resolving the address with provenance when base and index are both proven constants; pre/post-index, pairs, and literal loads stay opaque. No report field or engine-identity change (banner advances to v15).
- Correct the ARM64 semantic preview PC-relative literal-load class: the earlier encoding (bits 29-24 == `011010`, `opc` read from bits 23-22, imm19 from bits 21-3, plus non-existent `LDRSB`/`LDRSH` literal forms) also matched the conditional-select and data-processing (1 source)/(2 source) groups, so real `csel`/`lsl`/`abs` words were rendered as fabricated literal loads. The class now requires bits 29-24 == `011000` with `opc` at bits 31-30 (`LDR` 32-bit `opc 0`/64-bit `opc 1`, `LDRSW` `opc 2`) and imm19 at bits 23-5, re-verified with `clang -arch arm64`; the literal `PRFM` (`opc 3`) stays opaque. Because this correction removes previously emitted statements, the analysis cache engine identity advances to `arm64-analysis-v19-schema72`. No report field change.
- Extend the ARM64 semantic preview to conditional select (`CSEL`/`CSINC`/`CSINV`/`CSNEG` and the `CSET`/`CSETM`/`CINC`/`CINV`/`CNEG` aliases), rendering `dst = cond ? rn : rm;` with the increment/complement/negate form for the false operand and naming the condition code directly. Because PSTATE flags are not tracked, a defined value (and its folded constant with provenance) is claimed only for the always-true `al` condition; every other condition writes an unknown barrier. The flag-setting form and the reserved encoding bit stay opaque. No report field or engine-identity change (banner advances to v21).
- Extend the ARM64 semantic preview to the register-defined variable shifts (`LSLV`/`LSRV`/`ASRV`/`RORV` from the data-processing (2 source) group), rendering `dst = src << amount;`, `dst = src >> amount;`, `dst = src >>> amount;`, and `dst = ror(src, amount);`. The shift amount is read from a whole register and reduced modulo the register width, so a shift by the full width is a no-op; proven constants fold with instruction provenance. The division forms share the encoding group and are modeled separately, and the unallocated opcodes of the group stay opaque. No report field or engine-identity change (banner advances to v22).
- Extend the ARM64 semantic preview to the data-processing (1 source) family (`RBIT`/`REV16`/`REV`/`REV32`/`CLZ`/`CLS`), rendering `dst = rbit(src);` and the `rev16`/`rev`/`rev32`/`clz`/`cls` forms, each computed at the register width and folding bit/byte reversals and leading-bit counts with instruction provenance. Within the group the opcode-2 word is `REV32` for a 64-bit register but 32-bit `REV` for a 32-bit one, opcode-3 `REV` is 64-bit only, and the unallocated opcodes (including the register pointer-authentication subset) stay opaque. No report field or engine-identity change (banner advances to v23).
- Extend the ARM64 semantic preview to the concatenated-field extract (`EXTR`), rendering `dst = extr(left, right, lsb);` (the register-width field taken from the pair `left:right`) and collapsing to `dst = ror(src, lsb);` when both sources are the same register (the `ROR` immediate alias). The field folds with instruction provenance, the encoding's `N == sf` and `o0 == 0` requirements are enforced, a 32-bit `imms` is bounded to 0..31, and unallocated encodings stay opaque. No report field or engine-identity change (banner advances to v24).
- Extend the ARM64 semantic preview to the flag-neutral add/subtract with carry (`ADC`/`SBC`), rendering `dst = left + right + carry;` and `dst = left + ~right + carry;`. The carry input is `PSTATE.C`, which the preview does not track, so the destination is rendered as an explicit unknown barrier and no constant is ever folded; the write invalidates any previously tracked value. The flag-setting `ADCS`/`SBCS` forms (like `ADDS`/`SUBS`) and the conditional compares stay opaque. No report field or engine-identity change (banner advances to v25).
- Extend the ARM64 semantic preview to the add/subtract (extended register) form (`ADD`/`SUB` with `UXTB`/`UXTH`/`UXTW`/`UXTX`/`SXTB`/`SXTH`/`SXTW`/`SXTX` and the `imm3` shift, flag-neutral only). The second operand is the extension applied before the shift, rendered as `dst = left + (zext(w2, 8) << 2);` and the signed counterparts, with `UXTX`/`SXTX` using the operand unchanged; the destination and the first source name the stack pointer when their field is 31 while `Rm` still names `ZR`. The extension and shift fold with instruction provenance; the flag-setting `ADDS`/`SUBS` forms and the unallocated `imm3` values stay opaque. No report field or engine-identity change (banner advances to v26).
- Extend the ARM64 semantic preview to the exclusive and acquire/release single-register loads and stores (`LDXR`/`STXR`/`LDAXR`/`STLXR` and the ordered `LDAR`/`STLR`), addressed by the base register alone: loads render `dst = ldxr(base);`/`ldaxr`/`ldar`, an exclusive store renders its success status as the assignment target (`status = stxr(base, src);`/`stlxr`), and the ordered `STLR` renders `stlr(base, src);`, with the `b`/`h` suffix naming the byte/halfword width. A load defines its destination as an unknown barrier while an exclusive store defines only its status register, and the register pair forms (`LDXP`/`STXP`/`LDAXP`/`STLXP`) stay opaque. No report field or engine-identity change (banner advances to v27).
- Extend the ARM64 semantic preview to the `NOP`-family system instructions as no-register-effect markers: the barriers `DMB`/`DSB`/`ISB`/`CLREX` (rendering their domain, e.g. `/* dmb ish */`, `/* isb */`), the modeled HINTs `YIELD`/`WFE`/`WFI`/`SEV`/`SEVL`/`ESB`/`CSDB`, and the exception-generating `SVC`/`BRK`/`HLT` (rendering `/* svc(0): exception; value flow stops here */` and clearing tracked values, since the trap may not return and its handler may clobber registers). An unallocated `DMB`/`DSB` option, an unmodeled HINT number (e.g. `BTI`, `HINT #8`) and the `DCPS` group stay opaque. No report field or engine-identity change (banner advances to v28).
- Audit the ARM64 semantic preview against the encodings adjacent to the recently modeled families (no new instruction is modeled and no emitted statement changes, so the banner is unchanged): the extended-register `imm3` values 5..7, the register-offset option values 000/001/100/101, the conditional-select bit-29 word, and the SBFM/BFM/UBFM `N != sf` words all stay opaque, and a cross-mask test pins one representative word per family so a mask added later cannot silently swallow a neighbouring family.
- Extend the ARM64 semantic preview to the flag-only comparison aliases: `CMP`/`CMN` (the `SUBS`/`ADDS` forms with `Rd == 31`, in the immediate, shifted-register, and extended-register classes) and `TST` (the `ANDS` form with `Rd == 31`, immediate or shifted register), rendered as no-register-effect markers such as `/* cmp x0, 0x5 */` and `/* tst x0, 0xff */`. Because the result lives only in `PSTATE`, which the preview does not track, these write no general-purpose register and preserve tracked constants instead of clearing them (a constant now folds through a compare). The flag-setting arithmetic and logical forms that also write a general-purpose register (`ADDS`/`SUBS`/`ANDS` with `Rd != 31`), `BICS`, the conditional compares (`CCMP`/`CCMN`), and the add-with-carry forms stay opaque because their `PSTATE` side effect is not modeled. No report field or engine-identity change (banner advances to v29).
- Complete the 0.5.11 product/build identity across the app, Coordinator, Rust bridge/worker, VectorEmu server, packaging scripts, tests, and docs.

# 0.5.9 — Apple Firmware Decoding and Kernel Name Guesses

- Add bounded Apple firmware container/codec detection and macOS Compression.framework decoding for LZFSE, LZ4, LZMA, and zlib, plus guarded legacy `complzss`/LZSS handling.
- Add bounded IPSW ZIP local-entry inventory and stored-entry payload candidates; deflate/encrypted entries remain explicit inventory-only evidence.
- Add bounded raw-deflate decoding for IPSW local entries, including recursive handoff to nested IM4P/compressed payload candidates.
- Resolve IPSW data-descriptor entries through bounded central-directory parsing, while rejecting encrypted entries without credentials.
- Add opt-in bounded legacy ZipCrypto decryption through `HINDSIGHT_ZIP_PASSWORD`; AES ZIP entries remain explicitly unsupported.
- Add bounded DER/ASN.1 TLV structure inventory for IMG4 candidates with malformed-input, depth, and node-budget checks.
- Add safe bounded `bvxn`/LZVN decoding with a known-good Mach-O candidate test.
- Add heuristic XNU/SEP kernel function-name candidates from nearby strings with explicit provenance.

# 0.5.8 — Pressure-Aware Launch and Enterprise Operations

- Add pressure-aware startup deferral with explicit manual-connect recovery.
- Enforce managed extension policy modes and fail-closed capability governance.
- Add conflict-safe review decisions, handoff readiness explanations, and case integrity visibility.
- Document enterprise deployment, signed-release-manifest, retention, and diagnostics boundaries.
- Preserve protocol ABI 1, the exact 71-field analysis report, and analysis engine identity.

# 0.5.7 — Scheduling, Swift Demangling, Structural Pseudocode

- Priority-aware bounded scripting/firmware lanes with stale-reply invalidation and cancellation hooks.
- Rust priority scheduling model backed by existing memory leases and admission budgets.
- On-demand Swift ABI demangling using swift-demangle.
- Bounded structural pseudocode IR preview and standalone report CLI.
- Preserve analysis schema 71 and engine identity v15; no invented semantics.
- Update release metadata and add source contracts.

# VectorGlass 0.5.6 — Crash Resolution

- Add bounded, UUID-gated CLI crash symbol resolution with ASLR slide composition using unified symbol ranges.
- Add regression coverage for UUID mismatches, invalid image inventory, overlapping ranges, and ASLR slide.
- Update version/build metadata to 0.5.6 / 5600; keep the 71-field analysis protocol and engine identity unchanged.

# 0.5.6 — Completion Audit Hotfix

- Replace the HFS+ `read_file` placeholder with bounded catalog/CNID traversal, data/resource-fork reads, and extent-overflow continuation; validate real HFS+/HFSX signatures and volume geometry.
- Replace eMMC fake data completion with persistent block-backed CMD17/CMD24 transfers, fail-closed unsupported data commands, correct command-write execution, and state-preserving reset semantics.
- Remove the ARM64 logical-immediate fallback that could display fabricated masks; invalid encodings are now explicit.
- Wire DART timing into hit, miss, walker-fault, and early-fault translation paths.
- Correct stale 0.5.0/0.4.3 release expectations in active macOS/embedded-worker tests and `vg-emu-server` metadata.
- Reword bounded hardware models so the UI does not advertise stubs or a nonexistent secure-storage mock backend.
- Add a completion-audit regression suite and document explicit evidence/host boundaries instead of leaving semantic placeholders.
- Keep product version 0.5.6, report schema 71, and engine identity `arm64-analysis-v15-schema71`; this is a correctness/completeness hotfix with no report ABI change.

# 0.5.6 — Unified Symbols

- Add a bounded unified Symbol Resolver that combines exact `nlist` code symbols, admitted function starts, indirect-symbol stub/pointer evidence, and dyld import/export names with explicit provenance.
- Preserve aliases at identical addresses and keep imports/exports with no proven in-image address as name-only evidence instead of inventing virtual addresses.
- Add the Symbol Resolver workspace to navigation, typed evidence emission, and linkage-stage fingerprints.
- Advance the exact analysis report from 70 to 71 fields with `symbol_resolver` and invalidate older analysis caches via `arm64-analysis-v15-schema71`.
- Preserve all 0.5.4 analysis-hang budgets and off-main decoding/fingerprinting behavior.
- Advance stable product/service/worker identity to 0.5.6 / build 5600.

# 0.5.4 — Analysis Hang Fix

- Build hotfix 1: fix three Rust type errors introduced by the indexed symbol lookup optimization by borrowing the owned top-level symbol vector as a slice; also remove the unused initial `byte` assignment in the SLEB128 decoder.
- Remove repeated whole-CFG edge scans from CFG rendering and predecessor-value-flow generation; both now build bounded indices in linear passes.
- Bound global structural analysis, phi provenance, and iterative value propagation so malformed or unusually branch-dense binaries cannot turn analysis into effectively unbounded work.
- Reuse indirect-symbol state across linkage reports instead of reparsing and reallocating the Mach-O symbol/string tables several times.
- Narrow Apple Symbolication framework detection to matching symbol names instead of materializing the entire symbol-name table; cache `LC_FUNCTION_STARTS` presence once per symbolication pass.
- Decode large `AnalysisReply` payloads off the SwiftUI main actor, including Binary Diff, move XPC UTF-8 reply-size accounting off the main actor, and fingerprint continuity snapshots off-main.
- Expand worker progress phases through structural, runtime, value-flow, linkage, synthesis, evidence, and finalize stages so bounded expensive work remains observable.
- Keep the exact 70-field report schema, but advance cache engine identity to `arm64-analysis-v14-schema70` and stable product identity to 0.5.4 / build 5400.

# 0.5.3 — Symbolication + Apple Symbolication Decode

- Add static symbolication evidence: `LC_UUID`, `LC_FUNCTION_STARTS`, direct nlist symbols, bounded function ranges, and debug/unwind section inventory.
- Add Apple Symbolication/CoreSymbolication reference decoding without loading private frameworks at runtime.
- Advance the analysis report to 70 fields and cache engine identity to `arm64-analysis-v13-schema70`.
- Correct current status/validation documentation drift around the previous report field count.
- Add symbolication contract tests and an explicit incomplete/review backlog.

# 0.5.3 — dyld + Soft-Linking Decode

- Decode legacy dyld rebase, regular bind, weak-bind, and lazy-bind opcode streams from `LC_DYLD_INFO` / `LC_DYLD_INFO_ONLY`.
- Add a dedicated Soft Linking evidence surface for weak dylib load commands, weak imports, SoftLinking.framework references, and runtime lookup helpers.
- Add bounded dyld shared-cache virtual-address reads through validated primary mapping records.
- Advance worker report identity to 68 fields / `arm64-analysis-v13-schema70`; bump stable product identity to 0.5.3 / build 5300.

# 0.5.1 — Typed Worker Evidence

- Emit bounded typed navigation evidence directly from the Rust worker over a structured sideband while preserving the exact 66-field stdout compatibility report.
- Add metadata/structure/linkage/synthesis stage fingerprints.
- Persist typed evidence in cache envelope schema 3 while continuing to read schemas 1 and 2.
- Prefer worker-native records in Swift and supplement only unmigrated channels through the 0.5.0 adapter.
- Keep protocol ABI 1, `arm64-analysis-v11-schema66`, project SQLite schema 1, and universal macOS support.
- Bump stable identity to 0.5.1 / build 5100.

# 0.5.0 — Evidence Core

- Add a typed immutable `VGEvidenceRecord` / `VGAnalysisEvidenceIndex` layer with stable semantic IDs, normalized search text, exact address/symbol indices, source ranges and precomputed focus ordering.
- Build the evidence index off the SwiftUI render path at utility priority and publish it atomically.
- Route Analysis Search, Analysis Triage, Related Evidence and current collection-health membership through the typed index instead of repeatedly splitting/reparsing all 66 report strings.
- Bound the in-process evidence corpus to 50,000 records and surface explicit indexing/truncation state.
- Bound resident full compatibility reports to an 8-entry LRU; evicted reports can be reloaded from the project-local analysis cache for Binary Diff.
- Debounce launch-session persistence by 350 ms so typing in search/address/symbol fields no longer writes UserDefaults on every keystroke.
- Preserve protocol ABI 1, the exact 66-field worker report, `arm64-analysis-v11-schema66`, cache envelope schema 2, project SQLite schema 1, and universal Intel/Apple Silicon behavior.
- Bump stable product/service/worker identity to 0.5.0 and Apple bundle build to 5000.

# 0.5.0 — Evidence Core

- Add a typed, immutable `VGEvidenceRecord` / `VGAnalysisEvidenceIndex` layer with stable semantic IDs, normalized search text, exact address/symbol indices, source ranges and precomputed focus ordering.
- Build the evidence index off the SwiftUI render path at utility priority and publish it atomically.
- Route Analysis Search, Analysis Triage, Related Evidence and current collection-health membership through the typed index instead of repeatedly splitting/reparsing all 66 report strings.
- Bound the in-process evidence corpus to 50,000 records and surface explicit indexing/truncation state.
- Bound resident full compatibility reports to an 8-entry LRU; evicted reports can be reloaded from the project-local analysis cache for Binary Diff.
- Debounce launch-session persistence by 350 ms so typing in search/address/symbol fields no longer writes UserDefaults on every keystroke.
- Preserve protocol ABI 1, the exact 66-field worker report, `arm64-analysis-v11-schema66`, cache envelope schema 2, project SQLite schema 1, and universal Intel/Apple Silicon host behavior.
- Bump stable product/service/worker identity to 0.5.0 and Apple bundle build to 5000.

# 0.4.9 Hotfix 5 — Universal macOS / Intel compatibility

- Build the Xcode app/XPC service for `arm64 x86_64`.
- Build/lipo the Rust bridge, `reworker`, and built-in `vg-emu-server` as universal artifacts.
- Keep static ARM64/ARM64e analysis, projects, evidence/review, automation, firmware tooling, and VectorEmu available on Intel Macs.
- Add shared host-architecture capability detection and diagnostics.
- Close out Apple-Silicon-only Hypervisor RE/SoC execution and the current ARM64 native debugger when running the x86_64 slice.
- Apply that closeout consistently across menu, sidebar/tool-card, restored-workspace, selected-artifact, and no-artifact navigation paths; Coordinator/native guards remain the final fail-closed boundary.
- Enforce the same host gates in Coordinator/native code so UI state is not the correctness boundary.
- Type-check app and Coordinator for both `arm64-apple-macos14.0` and `x86_64-apple-macos14.0`.
- Add dual-architecture release-gate and universal-artifact regression contracts.

# Changelog
### Hotfix 4 — Audit closure and release-gate completeness
- Replace hand-maintained macOS Swift type-check source lists with `Config/project-topology.json` as the source of truth; this explicitly brings `AnalysisNavigation.swift`, `AnalysisContinuity.swift`, and all future generated target Swift sources under the release-host type-check gate.
- Add portable release-gate coverage tests that fail if generated target membership and type-check coverage diverge.
- Harden `ProjectStore` intermediate filesystem trust boundaries: `objects`, `staging`, and `analysis-cache` must be real non-symlink directories; staged capture and existing cache destinations must be regular non-symlink files.
- Add project-store security regression contracts for directory substitution, staged capture, and cache publication.
- Refresh current STATUS/VALIDATION/release-host documentation to the actual 0.4.9 hotfix baseline and add explicit Function Neighborhoods/type-check/symlink qualification steps.
- Keep product identity 0.4.9 / build 4109, protocol ABI 1, the exact 66-field analysis report, cache envelope schema 2, project SQLite schema 1, and `arm64-analysis-v11-schema66` unchanged.

### Hotfix 3 — Function Neighborhoods responsiveness
- Fixed a Function Neighborhoods hang caused by eagerly presenting up to 4,000 interactive evidence rows with context menus in a single SwiftUI surface.
- Replace the generic evidence renderer with `VGBoundedEvidenceList`, which initially presents 200 rows and reveals additional rows in explicit 200-row batches while preserving evidence selection and Code/CFG context navigation.
- Rework `function_neighborhood_report` so it keeps full connectivity counts but materializes detailed edge vectors only for the bounded top 256 functions, capped at 12 incoming and 12 outgoing rows per function.
- Avoid redundant caller-function binary searches because recovered direct-call records already carry the admitted caller function entry.
- Add regression contracts for bounded evidence rendering and bounded Function Neighborhood edge materialization.

### Hotfix 2 — SwiftUI collection identity
- Fixed ambiguous `ForEach` overload resolution in `AnalysisNavigation.swift` by making `VGInvestigationCollection.id` explicit at all collection-backed render sites.
- Prevents Xcode from selecting the `Range<Int>` overload and producing `[VGInvestigationCollection]`/`Int` conversion errors.
- Added a regression contract for explicit collection identity.


## 0.4.9 Hotfix 1 — Tag pill icon initializer

- Fix the macOS compile failure in `App/AnalysisNavigation.swift` where icon-bearing `VGTagPill(text:systemImage:)` calls did not match the shared `VGTagPill` component.
- Extend `VGTagPill` with a source-compatible optional `systemImage` initializer; existing text-only call sites remain unchanged.
- Render icon-bearing pills through SwiftUI `Label` while preserving the existing typography, padding, and capsule styling.
- Add a source-validation regression guard for the icon-capable initializer.

## 0.4.9 — Review actions & follow-up queue

- Add a bounded per-collection Follow-Up Queue with explicit Open/Completed task state, concise notes, and up to 16 exact collection evidence identities captured as provenance context.
- Keep workflow actions separate from evidence semantics: follow-ups are analyst-authored tasks, never findings, severity, exploitability, reachability, or semantic-importance scores.
- Add responsive Review controls for creating, completing, reopening, and removing follow-ups plus collection-level open/completed summaries.
- Extend Investigation Handoff output with unresolved follow-ups and completed-action counts so review intent survives analyst handoff.
- Advance project-local analysis-continuity state to schema 5 while continuing to read schemas 1–4 and retaining the 1 MiB total storage ceiling plus explicit follow-up limits.
- Publish `review_follow_up_queue`, `follow_up_evidence_binding`, `collection_review_summary`, and `handoff_follow_up_context` capabilities.
- Keep protocol ABI 1, the exact 66-field `AnalysisReport`, `arm64-analysis-v11-schema66`, cache envelope schema 2, and project SQLite schema unchanged so compatible 0.4.2–0.4.8 analysis caches remain reusable.
- Bump stable product/service/worker identity to `0.4.9` and Apple bundle build to `4109`.

## 0.4.8 — Review journal & handoff

- Add a bounded per-collection review journal with explicit analyst workflow states: Open, In Review, Follow Up, and Reviewed.
- Capture the exact collection pin identities with each journal entry so review history remains tied to preserved evidence identity without claiming semantic meaning.
- Add a single-collection Review Journal surface; pairwise exact-identity collection comparison remains available when two or more collections exist.
- Add bounded Markdown Investigation Handoff generation with artifact/source identity, workflow state, collection health, exact review drift, preserved evidence, and recent journal entries; handoff is copied locally to the clipboard on explicit action.
- Advance project-local analysis-continuity state to schema 4 while continuing to read schemas 1–3, retaining the 1 MiB total budget plus explicit journal-entry/note ceilings.
- Publish `collection_review_journal`, `collection_workflow_state`, `investigation_handoff`, and `review_timeline` capabilities.
- Keep protocol ABI 1, the exact 66-field `AnalysisReport`, `arm64-analysis-v11-schema66`, cache envelope schema 2, and project SQLite schema unchanged so compatible 0.4.2–0.4.7 analysis caches remain reusable.
- Bump stable product/service/worker identity to `0.4.8` and Apple bundle build to `4108`.

## 0.4.7 — Collection review & evidence drift

- Add explicit per-collection review baselines over exact preserved pin identities and current-report presence.
- Add bounded review drift for collection membership plus current/preserved transitions; drift is provenance/identity evidence only and is not interpreted as severity, correctness, or semantic importance.
- Add a responsive Review mode to Analysis Triage for pairwise collection comparison: shared evidence, collection-A-only evidence, and collection-B-only evidence remain directly navigable.
- Advance the project-local analysis-continuity document to schema 3 while accepting schema 1 and 2 documents in place; storage remains capped at 1 MiB and atomically published.
- Keep the analysis engine identity `arm64-analysis-v11-schema66`, protocol ABI 1, exact 66-field report schema, project SQLite schema, and cache envelope schema 2 unchanged.
- Publish `collection_comparison`, `collection_review_baselines`, `collection_evidence_drift`, and `investigation_review_mode` capabilities.
- Bump stable product/service/worker identity to `0.4.7` and Apple bundle build to `4107`.

## 0.4.6 — Investigation workflow & collection health

- Add cross-collection filtering over collection names and preserved evidence text/source/symbol identity.
- Add safe collection renaming without changing pin identities or evidence provenance.
- Add bounded collection-health summaries against the currently loaded validated analysis corpus; preserved-only evidence is explicitly not treated as invalid or semantically stale.
- Add Previous/Next traversal through bounded recent-evidence pivots, with Workspace menu shortcuts.
- Keep the analysis engine identity `arm64-analysis-v11-schema66`, protocol ABI 1, exact 66-field report schema, project SQLite schema, and cache envelope schema 2 unchanged.
- Bump stable product/service/worker identity to `0.4.6` and Apple bundle build to `4106`.

## 0.4.5 — Investigation collections & resume

- Add named project-local investigation collections over existing evidence pins, bounded to 32 collections per artifact and 64 findings per collection.
- Make collection membership pin-backed: adding evidence to a collection preserves the source evidence, while unpinning safely removes stale collection membership.
- Add **Collections** to Analysis Triage with responsive collection cards and collection-aware evidence menus.
- Add **Resume Latest Evidence** plus per-visit Resume actions for the bounded Source/Code/CFG/Symbol trail; replay does not manufacture duplicate history entries.
- Advance analysis-continuity document schema to 2 while accepting schema-1 documents in place; storage remains `.vectorglass/analysis-continuity-v1.json`, bounded to 1 MiB and atomically published.
- Keep protocol ABI 1, the exact 66-field `AnalysisReport`, cache schema 2, and `arm64-analysis-v11-schema66` unchanged so valid 0.4.2–0.4.4 caches remain reusable.
- Publish `investigation_collections`, `collection_aware_pins`, and `evidence_resume` capabilities.
- Bump stable product/service/worker identity to `0.4.5` and Apple bundle build to `4105`.

## 0.4.4 — Investigation continuity

- Add project-local persistent analysis pins, bounded to 128 entries and scoped to the captured artifact.
- Add bounded recent-evidence navigation history for successful Source, Code, CFG, and Symbol pivots; recents are de-duplicated and capped at 96 entries.
- Add a four-mode Analysis Triage desk: Focus, Pinned, Recent, and Changes.
- Add per-artifact analysis snapshots and a conservative channel-change ledger using bounded channel fingerprints and line counts; change status is continuity evidence, not semantic diffing or severity.
- Upgrade launch-session persistence to schema 2 with address, CFG focus, analysis query, and selected-symbol restoration while continuing to accept schema-1 records.
- Store continuity metadata under `.vectorglass/analysis-continuity-v1.json`, cap it at 1 MiB, reject symlinked state paths/files, and publish atomically. Analyst note contents remain outside continuity state.
- Keep protocol ABI 1, the exact 66-field `AnalysisReport`, cache schema 2, and `arm64-analysis-v11-schema66` unchanged so 0.4.2/0.4.3 analysis caches remain reusable.
- Publish `persistent_analysis_pins`, `recent_evidence_history`, `analysis_change_ledger`, and `launch_context_continuity` capabilities.
- Bump stable product/service/worker identity to `0.4.4` and Apple bundle build to `4104`.

## 0.4.3 — Structured analysis navigation and triage

- Add **Analysis Triage**, a bounded source-prioritized investigation queue over existing validated analysis evidence. Ordering is explicitly for navigation efficiency, not vulnerability, severity, exploitability, semantic importance, or runtime reachability.
- Replace Navigator's anonymous flattened search output with structured, source-labelled evidence cards that preserve the originating workspace.
- Add direct evidence routing to source workspace, Disassembly, CFG, and Selected Symbol when the row exposes the required address/symbol identity.
- Add **Related Evidence** to the contextual inspector, correlating the selected row across analysis channels by exact parsed virtual address or explicit symbol identity without inventing semantic edges.
- Add address-aware context menus to the existing compact clickable-evidence surfaces so large reports gain navigation without permanent per-row control chrome.
- Bound structured navigation to 16 KiB evidence lines, 500 search hits, 180 triage rows, and 120 related-evidence rows (with defensive internal ceilings).
- Keep protocol ABI 1, the exact 66-field `AnalysisReport`, cache schema 2, and `arm64-analysis-v11-schema66` unchanged; valid 0.4.2 schema-2 analysis caches therefore remain reusable.
- Publish `structured_analysis_navigation`, `analysis_triage_workspace`, and `correlated_evidence_navigation` capabilities.
- Bump stable product/service/worker identity to `0.4.3` and Apple bundle build to `4103`.

## 0.4.2 — Staged analysis execution, cache continuity, and UI density hardening

- Add structured analysis-stage progress from the isolated Rust worker through CoordinatorService to the SwiftUI status surface without changing the 66-field final `AnalysisReport`.
- Report bounded monotonic stage percentages for source verification, Mach-O mapping/metadata, instruction decode, CFG construction, structural synthesis, evidence formatting, final validation, and publication. Percentages represent pipeline position, not wall-clock ETA.
- Keep worker stdout reserved for the final length-framed result; progress travels over a strict `VG_PROGRESS` stderr record that Coordinator strips from crash diagnostics before presentation.
- Extend `InspectionStatus` with `progress_percent` and `progress_stage`, add cache-lookup status, determinate analysis progress UI, and diagnostics fields for the current stage/percentage.
- Upgrade the project-local analysis cache envelope to schema 2 with separate analysis-engine and producer-version identities. 0.4.2 can safely reuse/migrate a valid 0.4.1 cache because the ARM64 analysis engine/report schema is unchanged.
- Make unreadable cache entries non-fatal: cache I/O/decoding failure now falls back to a fresh isolated worker run instead of aborting analysis.
- Preserve atomic evidence publication: VectorGlass does not expose half-validated analysis sections merely to make progress look faster; any previously completed result remains authoritative until a new complete report validates.
- Redesign **Targets** around adaptive cards/key-value fields instead of fixed 140–170 pt metadata columns that could overlap when the detail pane collapsed. Capability tags and address-pivot results now wrap/restack without hiding evidence.
- Add `VGFlowLayout`, `VGKeyValueField`, and `VGTagPill` as reusable density primitives so compact action groups and metadata adapt inside already-responsive panes.
- Audit and refactor dense controls across Debugger, Stop Context, Navigator, plugins/extensions, SoC Virtualization, VectorEmu, firmware/IPSW tooling, Hypervisor RE, Binary Workspace, Flow Graph, Trace Explorer, Runtime Correlation, Evidence Graph, Investigation Ledger, and Binary Diff.
- Add `python/tests/test_ui_density.py` plus source-validation guards that reject the legacy Targets fixed-column layout and known one-line control regressions.
- Publish `analysis_stage_progress`, `analysis_progress_percent`, `analysis_progress_diagnostic_separation`, `analysis_cache_patch_compatibility`, and `analysis_cache_envelope_v2` capabilities.
- Bump stable product/service/worker identity to `0.4.2` and Apple bundle build to `4102`; protocol ABI remains 1.

## 0.4.1 — Stable-line consolidation and analysis reuse

- Promote product/protocol identity to `0.4.1`, release channel `stable`, Apple marketing version `0.4.1`, and build `4101` while retaining protocol ABI 1.
- Preserve the responsive workbench, Coordinator/session recovery, bounded analysis, analysis-quality ledger, Function Intelligence, Data References, Function Neighborhoods, and conservative tail-call-candidate semantics from the beta line.
- Add a bounded project-local analysis cache keyed by captured SHA-256 and exact product version; cached reports are atomically published and revalidated before reuse.
- Keep analysis-cache storage outside `project.sqlite`, so existing beta project schema version 1 opens without migration.
- Rebind a valid cached `AnalysisReport` to the new request UUID without changing source identity or evidence content.
- Add stable release-host qualification via `scripts/test-release-macos.sh`; stable distribution requires Developer ID signing, notarization, stapling, and the real-target/8 GiB pressure matrix.
- Publish `project_analysis_cache`, `analysis_cache_identity_binding`, and `stable_release_channel` capabilities.

## 0.4.0-beta.14 — Analysis evidence quality and neighborhoods

- Add an **Analysis Quality** ledger covering executable scan admission, decoder coverage, data-in-code exclusions, dominator budget omissions, and explicit evidence classes.
- Add **Function Neighborhoods** joining one-hop direct BL caller/callee evidence with separately labelled cross-function direct-B candidates.
- Add **Tail-Call Candidates** for unconditional direct branches that cross discovered function boundaries and land exactly on another admitted function entry; candidates are never silently promoted to proven calls.
- Preserve direct BL targets outside the bounded scan as external outgoing evidence while preventing them from being attributed as incoming edges to an internal function.
- Extend Analysis Insights with quality/coverage context and candidate-transfer counts.
- Extend the exact bounded analysis transport from 63 to 66 fields and expose all three new surfaces through search/diagnostic export.
- Publish `analysis_quality_ledger`, `function_neighborhoods`, and `cross_function_tail_candidates` capabilities.
- Bump product/protocol identity to `0.4.0-beta.14` and Apple bundle build to `4014`; retain `CFBundleShortVersionString=0.4.0`.

## 0.4.0-beta.13 — Scalable analysis hardening

- Replace the whole-program quadratic dominator allocation with a per-function bounded solver, preventing branch-heavy firmware CFGs from consuming unbounded memory.
- Add an explicit 1024-basic-block per-function dominator-detail budget. Functions over the budget keep CFG evidence but withhold dominator/frontier claims rather than publishing partial results or exhausting memory.
- Remove the now-unneeded successor-set copy from structural analysis to reduce working-set pressure further.
- Index symbol names for Function Intelligence instead of repeatedly scanning the full symbol table for every discovered function.
- Report worker termination-by-signal distinctly through CoordinatorService; the app remains alive and the analysis failure identifies signal termination rather than masquerading as an XPC/app crash.
- Add portable regression contracts that reject the former global `vec![all.clone(); n]` dominator pattern and require the bounded per-function path.
- Publish `bounded_dominator_solver`, `per_function_dominator_budget`, and `analysis_worker_signal_diagnostics` capabilities.
- Bump product/protocol identity to `0.4.0-beta.13` and Apple bundle build to `4013`; retain `CFBundleShortVersionString=0.4.0`.

## 0.4.0-beta.12 — Analysis synthesis

- Fix the beta.11 VectorEmu run-step `Stepper` compile error by keeping the bound value/range as `UInt64` while using the required `UInt64.Stride` (`Int`) increment.
- Add **Function Intelligence**, a bounded per-function triage table using a disclosed connectivity/size formula over discovered function boundaries, direct calls, control-flow branches, and basic blocks. The attention score is explicitly not a semantic, security, vulnerability, or reachability claim.
- Add **Data References** for architecturally explicit ARM64 ADR, immediately paired ADRP+ADD, and literal-immediate references whose resolved target validates inside a parsed Mach-O section. String text is attached only for verified string-bearing sections.
- Add **Analysis Insights**, an evidence-first synthesis view that surfaces top structural function candidates and representative validated data references without inventing intent.
- Extend the bounded analysis worker/Swift report contract from 60 to 63 exact fields and wire the new evidence into global analysis search and diagnostic export.
- Publish `function_intelligence`, `pc_relative_data_references`, and `analysis_insights` capabilities and regenerate the capability schema.
- Preserve the beta.11 responsive-workbench topology while adding the new analysis surfaces; no fixed-width pane regression is introduced.
- Bump product/protocol identity to `0.4.0-beta.12` and Apple bundle build to `4012`; retain `CFBundleShortVersionString=0.4.0`.

## 0.4.0-beta.11 — Responsive Workbench re-architecture

- Replace fixed three-column flagship compositions with adaptive wide / medium / compact topology via `VGAdaptiveThreePane`.
- Add `VGAdaptiveTwoPane` for debugger, Targets, Binary Diff, scripting/plugin output, virtualization, VectorEmu, and other report/tool desks.
- Move the global contextual inspector from trailing to bottom-docked when the detail canvas becomes narrow instead of continuing to squeeze the workspace.
- Rebuild the global header as two semantic rows and split the status surface into state and message bands.
- Reorganize dense workspace toolbars and make summary/metric bands overflow independently of the primary canvas.
- Lower the supported minimum window from 1180 × 760 to 900 × 680 while increasing the default workbench size to 1480 × 920.
- Add source-regression guards and capability declarations for adaptive topology, responsive chrome, and overflow-safe metric bands.
- Bump product/protocol identity to `0.4.0-beta.11` and Apple bundle build to `4011`; retain `CFBundleShortVersionString=0.4.0`.

## 0.4.0-beta.10 — Launch/session continuity

- Persist a bounded launch-session record containing only the last project path, selected artifact ID, workspace, and layout preset; unsaved analyst note text is deliberately not duplicated into preferences.
- Automatically reopen the last project after a successful Coordinator capability handshake on full app relaunch, then restore the prior artifact/workspace context only after the project snapshot is authoritative again.
- Validate saved-session size, absolute path shape, selected-record identifier length, workspace membership, and layout-preset membership before trusting persisted metadata.
- Degrade stale or missing saved project paths to the normal create/open flow instead of trapping the workbench in a restore state.
- Add explicit **Reopen Last Project** and **Forget Saved Project** controls plus privacy-bounded diagnostics that report only the saved project basename.
- Publish `launch_session_continuity`, `automatic_last_project_reopen`, and `stale_launch_session_recovery` capabilities; regenerate the capability schema to 180 fields.
- Bump product/protocol identity to `0.4.0-beta.10` and Apple bundle build to `4010`; retain `CFBundleShortVersionString=0.4.0`.

## 0.4.0-beta.9 — Coordinator session recovery

- Separate Coordinator transport reachability from project-session readiness so an XPC restart cannot leave project-bound actions falsely enabled against a service with no project open.
- Validate the Coordinator capability handshake before treating the service as usable: exact product version, beta release channel, ABI 1, Rust bridge, analysis, and execution capabilities must agree with the app.
- Automatically re-open the current project after a successful reconnect, preserving client-side unsaved note drafts and the selected artifact when the recovered snapshot still contains it.
- Add a real connection reset path so **Reconnect Coordinator** establishes a fresh XPC generation instead of reusing an existing connection object.
- Gate page loading, analysis, Binary Diff, note saves, and Import Binary on the recovered project session rather than on service reachability alone.
- Add explicit **Restoring project** / **Project session unavailable** UI states, manual Restore Project actions, and Support-menu reconnect controls.
- Add privacy-bounded **Copy Diagnostics** output with product/service identity, session state, project basename/artifact count, current workspace, active analysis phase, and runtime flags without exporting the full project path.
- Publish `coordinator_handshake_validation`, `coordinator_session_rehydration`, and `beta_diagnostics` capabilities; regenerate the capability schema.
- Bump product/protocol identity to `0.4.0-beta.9` and Apple bundle build to `4009`; retain `CFBundleShortVersionString=0.4.0`.

## 0.4.0-beta.8 — End-to-end workbench polish

- Probe CoordinatorService when the main workbench first appears and distinguish connecting, online, and offline states instead of reporting a false offline state until a project request succeeds.
- Add explicit Coordinator reconnect actions and surface disconnect recovery in the persistent status bar and first-run screens.
- Gate Import Binary everywhere on its real prerequisites: Coordinator online, project open, and no conflicting active operation; direct model calls now explain which prerequisite is missing instead of silently returning.
- Split the empty workbench into a true first-run investigation state and an open-but-empty project state with a focused first-artifact CTA.
- Add no-result feedback to the global command palette and sidebar tool filter.
- Keep the sidebar artifact preview bounded while exposing the exact overflow count and a one-click path to the full Targets workspace.
- Make persistent status text inspectable when clipped and keep menu enablement synchronized with model state.
- Bump product/protocol identity to `0.4.0-beta.8` and Apple bundle build to `4008`; retain `CFBundleShortVersionString=0.4.0`.
- Add a release audit covering portable validation results, environment limitations, residual RC gates, and the polish contract.
- Make the full portable release script fail fast when Cargo is absent while keeping `validate-source.py` independently usable for source/contract audits.

## 0.4.0-beta.7 — Analysis admission correctness
- Fixed a native Mach-O admission regression that confused `LC_FUNCTION_STARTS` (`0x26`) with `LC_DATA_IN_CODE` (`0x29`) and rejected ordinary modern ARM64 binaries before analysis.
- `LC_FUNCTION_STARTS` and `LC_DATA_IN_CODE` no longer veto a valid `__text` mapping.
- Data-in-code entries are honored as decode/SSA barriers instead of being interpreted as ARM64 instructions.
- Real `cryptid != 0` code is admitted for metadata/linkage analysis while instruction pages and code semantics remain unavailable rather than decoding ciphertext.
- Replaced the unavailable `doc.binary` SF Symbol with the SDK-baseline `doc` symbol.
- Added release guards for Mach-O load-command identities and the modern metadata admission regression.

## 0.4.0-beta.6 — Analysis reliability and observability

- Added a bounded 10-phase inspection state model: admitted, launching, running, receiving, validating, publishing, cancelling, completed, failed, and cancelled.
- Added the `inspectionStatus` XPC endpoint with bounded recent terminal-status retention for post-failure diagnosis.
- Added live analysis phase/elapsed-time reporting and a status-bar Cancel action without exposing request UUIDs as user-facing progress.
- Separated optional status-poll deadlines from connection-generation failures so a delayed progress query cannot invalidate or cancel the analysis it observes.
- Raised only the analysis worker deadline to 240 seconds and the matching client envelope to 260 seconds; capture/page deadlines remain unchanged.
- Preserved bounded worker stderr diagnostics on framed-result failures, including zero-exit parse/validation failures.
- Added a portable analysis-status protocol contract and release guards for status/schema integration.

## 0.4.0-beta.5 — Analysis lifecycle and worker supervision

- Fixed analysis results being discarded by CoordinatorService because its exact-field whitelist still described the early 13-field report instead of the current bounded AnalysisReport.
- Added shared protocol limits: 256 KiB control frames, 8 MiB analysis worker frames, 2 MiB default XPC replies, and 12 MiB analysis XPC replies.
- Kept page/capture/control traffic on the original small frame budget; only analysis uses the larger bounded transport.
- Reworked WorkerSupervisor to join stdout, stderr, and process termination asynchronously; removed user-initiated waitUntilExit()/DispatchGroup.wait() blocking that triggered Thread Performance Checker priority inversions.
- Added a portable analysis-protocol regression contract that exercises a modern >256 KiB report and exact field-set validation.
- Updated PROTOCOL_V1 documentation and release validators to enforce the split control/analysis transport contract.


## 0.4.0-beta.4 — Completeness/correctness audit

- Performed a full beta audit across Swift app/service code, Rust integration, Xcode topology, persistence, plugins, firmware/runtime helpers, release scripts, signing, and validation contracts.
- Removed remaining production Swift force-unwrap assumptions from debugger, virtualization, firmware, scripting, workspace, and graph paths; the source validator now rejects forced unwraps in shipping Swift sources.
- Added a complete CoordinatorService Swift type-check against the real bridging header; the beta gate now type-checks both the entire app target and the entire service target before Xcode builds.
- Hardened project persistence against symlinked roots/databases, creates private project directories, and rolls back newly captured object files if database publication fails.
- Hardened Investigation Ledger storage against symlink redirection and added a regression contract for malicious `.vectorglass` symlinks.
- Hardened plugin packages: symlinked bundles/manifests/entry/signed files are rejected and installs are bounded by entry count and total bytes.
- Reworked firmware/runtime synchronous callbacks and process-output capture to be lock-protected and bounded; helper tools drain output while running so large IPSW listings cannot deadlock on full pipes.
- Tightened network download completion to a final HTTPS 2xx response.
- Corrected beta packaging checks to the real `CoordinatorService.xpc` product and verify the service executable, Rust worker, and VectorEmu executable individually.
- Made notarization qualification real rather than aspirational: notarization requires an explicit Developer ID identity/team, hardened-runtime Release build, signature verification, submission, stapling, and staple validation.
- Made VectorEmu portable Cargo checks online-by-default for clean machines, with explicit `VECTORGLASS_CARGO_OFFLINE=1` opt-in for offline qualification.
- Normalized directly invokable Python script permissions and expanded release guards for the audit findings.

## 0.4.0-beta.3 — macOS 14 SwiftUI compatibility hotfix

- Fixed the main-app compile blocker caused by an invalid `frame(width:minHeight:)` overload in the Navigator context inspector.
- Migrated all remaining one-parameter `onChange(of:)` handlers to the macOS 14+ two-parameter form across ContentView, Binary Workspace, Graph Workspace, Runtime Correlation, and Trace Explorer.
- Added source guards so invalid mixed fixed/minimum frame overloads and deprecated one-parameter `onChange` handlers cannot re-enter the beta branch.
- Kept the beta feature freeze intact; this release changes SDK compatibility/qualification only.

## 0.4.0-beta.2 — Rust 1.90 compatibility hotfix

- Fixed the Apple-Silicon VectorEmu build on the beta release toolchain by replacing the Rust 1.97-only `usize::isolate_lowest_one()` call with the stable two's-complement equivalent `v & v.wrapping_neg()`.
- Declared Rust **1.90** as the VectorEmu minimum supported Rust version across all engine crates.
- Added a source/MSRV regression guard so post-1.90 integer convenience APIs cannot silently enter the beta branch without an intentional toolchain bump.
- Kept the beta feature freeze intact; this release changes compatibility and qualification only.

## 0.4.0-beta.1 — Alpha exit / beta hardening

- Promoted the product channel to **beta** without claiming RC readiness. Product/protocol SemVer is `0.4.0-beta.1`; Apple bundle marketing/build versions are `0.4.0` / `4001`.
- Removed the production CoordinatorService `fatalError`: Rust admission-governor initialization failure now produces a recoverable degraded-service error.
- Added project-open SQLite `quick_check` and bounded cleanup of abandoned UUID-shaped capture staging files after crashes/interrupted imports.
- Added Investigation Ledger last-known-good backup/recovery. A corrupt primary case ledger can recover from the previous valid copy and the UI reports the recovery.
- Marked all incomplete SoC/T2/SEP virtualization profiles **Experimental** in the service model and UI; incomplete device models are no longer presented as ordinary beta-qualified hardware.
- Added a beta source contract that rejects production Swift crash primitives outside tests and enforces the new recovery/maturity contracts.
- Rebuilt the capabilities schema from the actual bridge response (167 fields) and made schema/bridge parity a generated release contract; the stale alpha schema had contradicted `analysis`, `execution`, and most shipped capabilities.
- Made `scripts/build-app.sh` configuration-aware and added `scripts/test-beta-macos.sh` for Debug + Release build/signature/embedded-worker qualification with optional notarization submission.
- Added [`docs/BETA_READINESS.md`](docs/BETA_READINESS.md) with explicit RC exit gates instead of hiding host-only validation behind the beta label.

## 0.3.8-alpha — Binary Diff

- Added a first-class **Binary Diff** workspace for comparing two captured project artifacts without relying on matching VM addresses or file offsets.
- Added conservative staged function identity: unique non-generic symbols first, then unique exact normalized ARM64 instruction fingerprints; weak/unmatched functions remain added/removed.
- Added independent change classification for normalized code, CFG structure, direct references/calls, and bounded lifted semantics.
- Added session-local per-artifact analysis caching and an **Analyze Pair** workflow that analyzes only missing sides without forcing selection changes.
- Added changed/added/removed/semantic/CFG filters, summary metrics, explicit match-reason display, and side-by-side bounded disassembly with address pivots.
- Added a Foundation-only Binary Diff model and executable contract test.
- Added Binary Diff sources to deterministic Xcode generation and the full macOS app-target type-check.
- Added bridge capability flags for binary diff, semantic/CFG/xref comparison, and layout-resilient function matching.
- Added [`docs/BINARY_DIFF.md`](docs/BINARY_DIFF.md) documenting identity rules, normalization, evidence limits, and non-claims.

## 0.3.7-alpha — Investigation Ledger

- Added a persistent project-local **Investigation Ledger** that records bounded analyst/machine events with explicit provenance instead of relying on transient UI state.
- Added first-class hypotheses with Proposed, Supported, Contradicted, Confirmed, and Unresolved states; status changes are recorded as analyst actions rather than rewriting prior evidence.
- Added investigation bookmarks and bounded session snapshots that preserve workspace/address/search/symbol/target context without claiming to rewind a live process or emulator.
- Added timeline capture for project opens, artifact selection, completed analysis, debugger stops, memory observations, breakpoint changes, note saves, hypotheses, bookmarks, snapshots, restores, and exports.
- Added a three-pane Ledger workspace with provenance filtering, case search, timeline inspection, hypotheses, bookmarks, snapshots, and direct context reopening.
- Added a single-file `.vgcase` export containing artifact identities/hashes, the persistent ledger, selected static evidence, debugger observations, and explicit memory observations without silently copying the original binary.
- Added deterministic Markdown, PDF, and JSON report/export paths.
- Added an 8 MiB persistent-ledger ceiling, bounded entity counts, atomic writes, and a 32 MiB `.vgcase` export ceiling.
- Added a Foundation-only Investigation Ledger model/contract and made both ledger sources part of deterministic Xcode generation and the full macOS app-target type-check.
- Added bridge capabilities for persistent cases, hypotheses, bookmarks, snapshots, `.vgcase`, Markdown/PDF/JSON export.

## 0.3.6-alpha — Evidence Graph

- Added a first-class **Evidence Graph** workspace that connects static functions/references/provenance, debugger observations, runtime↔static correlations, explicit memory reads, and analyst notes without flattening their provenance.
- Added explicit confidence classes: **observed**, **derived**, **correlated**, **asserted**, and **unresolved**.
- Added typed relationship edges for containment, static references, runtime observation correlation, chronological debugger ordering, nearby explicit memory reads, analyst annotations, and supporting provenance.
- Added a bounded deterministic graph layout, zoomable canvas, minimap, search, and provenance filters.
- Added a relationship inspector that explains why each edge exists and provides direct pivots to Binary Workspace, CFG, Runtime Correlation, Trace Explorer, and Notes.
- Temporal proximity is explicitly described as proximity rather than causation; analyst notes remain assertions rather than machine-derived evidence.
- Added a Foundation-only Evidence Graph model and executable contract; both model and workspace are included in deterministic Xcode generation and the macOS full-app type-check.
- Added bridge capability flags for the provenance graph, confidence edges, analyst-note nodes, and runtime evidence links.

## 0.3.5-alpha — Trace Explorer

- Added a first-class **Trace Explorer** workspace over the bounded debugger stop/snapshot history.
- Added typed observed-transition events that preserve runtime PC, correlated static PC, function, reason, thread, register deltas, breakpoints, and module evidence.
- Added evidence-bounded transition classification for adjacent observations, static call sites, branch sites, return sites, unresolved mappings, and otherwise-unproven observed transfers.
- Added an observed function-path summary, register-change band, nearby explicit memory observations, and synchronized static instruction context.
- Added debugger **PC → Trace** pivots and made the Dynamic Analysis task layout open Trace Explorer by default.
- Added a Foundation-only Trace Explorer model and executable contract; the UI is included in deterministic Xcode generation and the complete macOS app-target type-check.
- Added bridge capability flags for trace exploration, observed transitions, register-change bands, function summaries, and nearby memory evidence.

## 0.3.4-alpha — Runtime Correlation

- Added a bounded debugger stop/snapshot history with per-stop register deltas and breakpoint state.
- Added explicit runtime memory observations captured from debugger reads.
- Added evidence-bounded dyld slide correlation from runtime PC to static preferred address.
- Added a synchronized Runtime Correlation workspace with stop timeline, static/live code, execution heat, register deltas, breakpoints, memory observations, and loaded-module evidence.
- Ambiguous module matches remain unresolved; VectorGlass does not invent a static translation.
- Added a portable runtime-correlation contract and release guards for the new model/workspace.

## 0.3.3-alpha — Binary Workspace

- Added a first-class **Binary Workspace** for the high-frequency reverse-engineering loop: function selection, code reading, semantic lifting, references, and instruction data in one synchronized desk.
- Added a Foundation-only binary workspace model for direct references/calls, bounded IR rows, conservative lifted statements, and instruction-word byte interpretation.
- Added a resizable function navigator with name/address filtering and persistent function selection.
- Added split Code / Lifted presentation with a raw worker-IR mode; unsupported semantics remain explicit opaque barriers rather than invented pseudocode.
- Added a contextual References pane with clickable source/target addresses and deduplicated direct call/branch evidence.
- Added an instruction Data pane showing opcode word, little-endian bytes, signed/unsigned values, decode fields, annotations, and modeled data-flow evidence.
- Added debugger **Follow PC** synchronization so a live stop can drive function and instruction selection in the same static workspace.
- Changed the `Code` task-layout preset to open Binary Workspace while retaining dedicated Disassembly and Annotated Code specialist views.
- Added a portable executable Binary Workspace contract and made both new source files part of deterministic Xcode generation and the complete macOS app-target type-check.
- Added bridge capability flags for the synchronized binary desk, conservative lifted view, function browser, xref inspector, and instruction-data inspector.

## 0.3.2-alpha — Graph Workspace

- Replaced the old stacked CFG-card presentation with a true zoomable/pannable graph canvas.
- Added a Foundation-only typed CFG model for basic blocks, typed edges, function ranges, address lookup, and deterministic hierarchical layout.
- Added a persistent graph minimap and bounded 240-node visualization ceiling so very large analyses remain navigable.
- Added a dockable function navigator with symbol/address filtering and direct graph focus.
- Added a synchronized block-context pane with Code, conservative IR, and modeled Data Flow views; it does not mislabel IR as recovered source.
- Added live debugger-PC highlighting for the containing basic block when dynamic state is available.
- Added visual differentiation for call, fallthrough, conditional, and direct branch edges, with calls independently hideable.
- Added graph selection propagation into the shared VectorGlass address/evidence context.
- Added a portable executable graph-model contract and made GraphModel/GraphWorkspace part of deterministic Xcode generation and full app-target macOS type checking.
- Added bridge capability flags for the graph workspace, CFG canvas, minimap, synchronized semantics, debugger highlighting, and dockable analysis panes.

## 0.3.1-alpha — Analyst Desk

- Added a bounded workspace tab strip so investigations can keep several analysis surfaces open without losing context.
- Added Focus Mode to collapse navigation/inspector chrome for code-heavy sessions while preserving the active workspace.
- Replaced static page, annotated, and live ARM64 text dumps with structured selectable disassembly rows.
- Added address/file-offset/word/instruction columns, symbol headers, control-flow cues, annotation rendering, and instruction context actions.
- Added explicit Context, Evidence, and Target inspector modes with target capabilities and address-space summaries.
- Added investigation metrics for functions, basic blocks, and active unified targets.
- Added a dedicated `AnalystSurface.swift` UI layer and included it in deterministic Xcode generation and full app-target macOS type checking.
- Extended release validation so tabbed workspaces, rich disassembly, focus mode, and inspector modes are release-contract features.

## 0.3.0-alpha — Workbench UX redesign

- Rebuilt the application shell around analyst jobs instead of exposing the entire internal tool list at once.
- Added a curated, searchable task sidebar with investigation/source context and direct workspace navigation.
- Added a global ⌘K command palette covering every advanced VectorGlass workspace.
- Added a document-style workspace header with address pivot, analysis action, task-layout selection, Mentor Mode, inspector control, and navigation history.
- Added a persistent contextual inspector for evidence, selection state, and extension panels.
- Added a bottom status surface for Coordinator health, current target, active work, and release identity.
- Added a coherent material-based visual system for GroupBoxes, tool cards, status chips, typography, spacing, and selected navigation state.
- Enlarged the default workbench window and made the primary canvas use available width rather than artificially capping analysis surfaces.
- Added full app-target type-check coverage for the new UI source files.

## 0.2.9-alpha — Main app Swift compile hotfix

- Replaced the nonexistent `JSONEncoder.vectorGlassPretty` reference with an explicit pretty/sorted `JSONEncoder`.
- Fixed all malformed `runModal()==.OK` comparisons in `WorkbenchModel`.
- Added the missing bounded `firstHexAddress(in:)` evidence parser.
- Made the VectorEmu post-launch delay use explicit `DispatchTime`/`DispatchTimeInterval` types.
- Added a macOS release gate that type-checks the complete VectorGlass app target before the full Xcode build.

## 0.2.8-alpha — Xcode VectorEmu staging hotfix

- Fixed the Xcode `Build VectorEmu` phase passing literal `$(TARGET_BUILD_DIR)` / `$(EXECUTABLE_FOLDER_PATH)` text into `/bin/sh`, where it was interpreted as command substitution.
- VectorEmu staging now derives its bundle destination directly from Xcode-exported environment variables.
- Added hard guards against empty/root output destinations and against staging outside `TARGET_BUILD_DIR` during Xcode builds.
- Added release validation that rejects `$(...)` build-setting syntax inside the VectorEmu shell phase.


## VectorGlass 0.2.7-alpha — VectorEmu Hex Dependency Hotfix

- Fixed `vg-emu-server` release compilation by removing four production references to the undeclared external `hex` crate.
- Added bounded internal hexadecimal encode/decode helpers instead of expanding the engine dependency graph.
- Updated the VectorEmu unit test to use the same dependency-free codec.
- Added source validation that rejects any reintroduction of `hex::` into `vg-emu-server` unless the dependency is explicitly declared.


## VectorGlass 0.2.6-alpha — Apple Silicon Hypervisor API Hotfix

- Corrected Apple-silicon VM creation to call `hv_vm_create(NULL)`; `HV_VM_DEFAULT` is the Intel-style creation flag and is not exposed for the Apple-silicon signature.
- Replaced the nonexistent ARM64 `HV_REG_SP` access with active-stack selection through `HV_SYS_REG_SP_EL0` / `HV_SYS_REG_SP_EL1`, chosen from CPSR/PSTATE.
- Added a macOS SDK compile gate for `native/virtualization/hv_vm.c` using the real Hypervisor.framework headers.
- Added a portable Apple-silicon Hypervisor API stub compile gate so removed/wrong constants are caught before Xcode.
- Added source-regression guards for the two SDK drift failures reported by Xcode.

## VectorGlass 0.2.5-alpha — Firmware Runtime Compile Hotfix

- Fixed the `FirmwareRuntimeHost` reply encoder so Swift has a concrete `FirmwareRuntimeReply` contextual type for shorthand `.init(...)` calls.
- Eliminated the cascade where Swift attempted to construct the `Encodable` protocol at every firmware/runtime reply site.
- Added a dedicated macOS `FirmwareRuntimeHost.swift` type-check gate before the full Xcode build.
- Added portable source-regression guards preventing the generic `Encodable` reply encoder from returning.

## VectorGlass 0.2.4-alpha — Scripting Host Compile Hotfix

- Fixed Swift 6 parsing/type-inference failures in `ScriptingHost.swift` around `DispatchTime` deadlines.
- Made plugin-name ordering explicit so `== .orderedAscending` is parsed correctly.
- Gave plugin annotation JSON an explicit `[[String: Any]]` shape so nullable addresses can use `NSNull`.
- Added a macOS scripting-host type-check gate before the full Xcode build.
- Added source-regression guards for the exact four diagnostics reported by Xcode.
- Silenced intentional Mach-O layout dead-field warnings in the Rust worker without removing the modeled fields.

## VectorGlass 0.2.3-alpha — Virtualization Service Type-Check Hotfix

- Added `Shared/VirtualizationMessages.swift` to the `CoordinatorService` compile target.
- Corrected the opaque Hypervisor VM handle to Swift `OpaquePointer`, matching Clang importer behavior for the incomplete C struct.
- Corrected four guest-memory helper call sites whose argument labels parsed but did not type-check.
- Added a portable Swift type-check gate for `VirtualizationHost.swift` using the real bridging header and shared virtualization messages.
- Added target-topology regression guards for virtualization protocol membership.
- Kept the Rust worker dead-field warnings non-fatal; they are warnings, not the current build blocker.

## VectorGlass 0.2.2-alpha — macOS Rust Artifact Path Hotfix

### Xcode/Rust build correctness
- Replaced hard-coded `/usr/bin/mkdir` calls with the macOS-safe `/bin/mkdir` path in all Rust artifact/embed build phases.
- Moved Rust bridge/worker handoff artifacts from target-specific `TARGET_TEMP_DIR` to configuration-wide `CONFIGURATION_TEMP_DIR/HindSightRustArtifacts`, so CoordinatorService and downstream build phases share one deterministic artifact location.
- Declared both `libreworkbench_bridge.a` and `reworker` as Xcode build-phase outputs.
- Added stale non-directory artifact cleanup plus parent-directory diagnostics before failing artifact creation.
- Updated linker search paths and worker embedding to consume the same shared artifact directory.
- Added source-validation guards against regressions to `/usr/bin/mkdir` and target-local Rust artifact handoff paths.

## VectorGlass 0.2.1-alpha — Rust Worker Compile Hotfix

### Rust worker correctness
- Fixed four unresolved `string_at_va(...)` references in Objective-C/Swift metadata recovery by adding one bounded, section-aware string helper.
- Fixed three unresolved signed-relative `resolve(...)` references in Swift field ownership/record recovery. The helper uses checked arithmetic and `unsigned_abs()` so malformed negative offsets cannot wrap.
- Fixed the iterative value-flow `BTreeSet::insert` branch so its boolean return value is discarded and the branch evaluates to `()`.
- Added release-validation guards for all three worker compile-regression signatures.
- Retained Cargo `--locked` reproducible-build enforcement.

## VectorGlass 0.2.0-alpha — Unified Target Model

This release turns VectorGlass's separate static, native, emulated, and virtualized analysis surfaces into one capability-driven target layer.

### Unified targets
- Added the **Targets** workspace as a first-class analyst surface.
- Added common `VGTargetIdentity`, `VGSessionHeader`, `VGAddressSpace`, capability, and address-pivot models.
- Static artifacts, attached native processes, VectorEmu machines, and Hypervisor guests now publish target identity through one model.
- Added capability discovery so the workbench can reason about what a target actually supports instead of inferring operations from backend names.
- Added evidence-backed address pivoting across validated Mach-O file offsets/preferred VM mappings, dyld runtime slides, VectorEmu regions, and Hypervisor guest regions.
- Unresolved address relationships are explicitly reported as unresolved rather than guessed.
- Added copy/export of target session JSON for reproducible research handoff.
- Python/extension selection context now includes target ID, target kind, backend, and declared target capabilities.

### Build correctness
- Fixed the shipped core `Cargo.lock`, which still contained inherited REWorkbench `0.11.8` package versions while the VectorGlass manifests declared `0.1.0`; this was the direct cause of `cargo --locked` refusing to build.
- Rebuilt the trimmed VectorEmu lockfile around the actual extracted workspace and added the previously missing `vg-emu-server` package entry.
- Added `scripts/check-lockfiles.py` and run it from core builds, VectorEmu builds, and portable tests so manifest/lock drift fails with a precise release-engineering diagnostic before compilation.
- Added `scripts/regenerate-lockfiles.sh` as the canonical maintainer path for intentional dependency/manifest changes.
- Kept `--locked`; reproducible builds are repaired rather than weakened.

### Product/UI
- Added a global Targets toolbar entry and Targets task layout.
- Added address-space, capability, session-fingerprint, and backend-state inspection.
- Added target-aware primary-view navigation.
- Bumped app, Coordinator, Rust worker/bridge, and VectorEmu product versions to `0.2.0` / `0.2.0-alpha` consistently.

### Validation
- Added lockfile structural validation to source validation.
- Added a portable Swift contract test for target parsing, slide arithmetic, deduplication, and module-row parsing.
- Regenerated deterministic Xcode topology with the new Target workspace source.

## VectorGlass 0.1.0-alpha — Product Genesis

- Split VectorGlass into an independent reverse-engineering research studio rather than an REWorkbench rename or an iPhemeral UI.
- Added the Machine Lab and Mentor Mode product direction.
- Extracted VectorEmu from reusable iPhemeral ARM64 CPU/MMU/device/SoC components.
- Added local-loopback VectorEmu execution/debugging, native debugging, Hypervisor research surfaces, Python automation, and `.vgplugin` extensions.
- Reframed full iPhemeral as an optional advanced external backend.
