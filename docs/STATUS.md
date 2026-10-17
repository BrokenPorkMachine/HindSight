# HindSight 0.5.11 Status

## Current milestone: stable universal-macOS analysis/review baseline

HindSight 0.5.11 is the current stable source identity (`stable`, protocol ABI 1, bundle build `5911`). The evidence engine is `arm64-analysis-v19-schema72` with the exact 72-field `AnalysisReport`, analysis-cache envelope schema 3 (with schema-2 backward reads), project SQLite schema 1, and analysis-continuity schema 5. The semantic preview now models the full move-wide immediate family (`MOVN`/`MOVZ`/`MOVK`), flag-neutral logical operations (`AND`/`ORR`/`EOR` bitmask immediates and register `AND`/`ORR`/`EOR`/`BIC`/`ORN`/`EON` with an optional `LSL`/`LSR`/`ASR`-shifted register operand, and the same shifted register operands for `ADD`/`SUB`), PC-relative `ADR`/`ADRP` address materialization, and conditional/indirect control flow (`B.cond`/`CBZ`/`CBNZ`/`TBZ`/`TBNZ`/`BR`/`BLR`), resolving indirect targets when a proven constant is available, and immediate shifts (`LSL`/`LSR`/`ASR`), register-defined variable shifts (`LSLV`/`LSRV`/`ASRV`/`RORV`), one-source bit/byte reversals and leading-bit counts (`RBIT`/`REV16`/`REV`/`REV32`/`CLZ`/`CLS`), the concatenated-field extract (`EXTR` with its same-register `ROR` immediate alias), the flag-neutral add/subtract with carry (`ADC`/`SBC`, with the untracked carry left as an unknown barrier), the add/subtract (extended register) form (`ADD`/`SUB` with `UXTB`/`UXTH`/`UXTW`/`UXTX`/`SXTB`/`SXTH`/`SXTW`/`SXTX` and the `imm3` shift), multiply-add (`MADD`/`MSUB`/`MUL`), integer division (`SDIV`/`UDIV`), widening and high-half multiplies (`SMADDL`/`UMADDL`/`SMSUBL`/`UMSUBL`/`SMULL`/`UMULL`/`SMULH`/`UMULH`), sign/zero extends (`UXTB`/`UXTH`/`SXTB`/`SXTH`/`SXTW`), general bitfield extracts (`UBFX`/`SBFX`) and inserts (`BFI`/`BFXIL`/`UBFIZ`/`SBFIZ`), conditional select (`CSEL`/`CSINC`/`CSINV`/`CSNEG` and the `CSET`/`CSETM`/`CINC`/`CINV`/`CNEG` aliases, selecting the true operand only for the always-true condition), and general-register loads and stores (`LDR`/`STR`/`STRB`/`STRH`/`LDRB`/`LDRH`/`LDRSB`/`LDRSH`/`LDRSW` with scaled unsigned, register-offset, unscaled `LDUR`/`STUR`, pre/post-index, and PC-relative literal `LDR`/`LDRSW` addressing), the exclusive and acquire/release loads and stores (`LDXR`/`STXR`/`LDAXR`/`STLXR` and the ordered `LDAR`/`STLR`), the `NOP`-family system instructions as no-register-effect markers (barriers `DMB`/`DSB`/`ISB`/`CLREX`, the modeled HINTs `YIELD`/`WFE`/`WFI`/`SEV`/`SEVL`/`ESB`/`CSDB`, and the exception-generating `SVC`/`BRK`/`HLT`, which clear tracked values), the flag-only comparisons (`CMP`/`CMN` from the `SUBS`/`ADDS` forms with `Rd == 31` in the immediate, shifted-register, and extended-register classes, and `TST` from `ANDS` with `Rd == 31`, rendered as no-register-effect markers that preserve tracked constants), the system-register reads and writes (`MRS`/`MSR`, with `MRS` making its destination an unknown barrier while preserving the other tracked registers and `MSR` preserving value flow), the conditional comparisons (`CCMP`/`CCMN`, rendered as markers preserving value flow), the LSE atomic memory operations (`LDAPR`/`LDADD`/`SWP`/`CAS` with size and ordering suffixes, with the load-type ops making their destination an unknown barrier and `CAS` preserving value flow), and cross-basic-block value propagation (constants survive unconditional and conditional branches, merging at joins), the flag-setting arithmetic and logical forms (`ADDS`/`SUBS`/`ANDS`/`BICS` with a general-purpose destination, modeled like their flag-neutral counterparts), the first SIMD/FP instructions (`MOVI.2D` and `FMov`, with `FMov` propagating a V register's low 64 bits into a general-purpose destination), and tracks proven constants through supported straight-line ARM64 operations; it is not source decompilation.

The 0.4.9 feature line added bounded analyst Follow-Up Queue workflow over the existing collection/review system. Older analysis caches are reused only when their source and engine identities match the current engine; the 0.5.3→0.5.7 engine bump intentionally invalidated prior analysis results so the scalability budgets and hang fixes are not bypassed by cache reuse.

## 0.5.9 Apple firmware decoding and kernel name guesses

- Resolver precedence is exact `nlist` code symbol → exact indirect-symbol name → validated unnamed function start.
- Stub/pointer aliases are retained; dyld import/export names lacking a proven absolute address remain name-only evidence.
- Resolver output is bounded and feeds the typed evidence index, address/symbol pivots, navigation, and linkage-stage fingerprints.
- dSYM/DWARF source-line recovery remains explicitly incomplete.

- Analysis JSON decoding, XPC reply-size accounting, and continuity-channel fingerprinting no longer perform multi-megabyte CPU work on the SwiftUI main actor.
- CFG rendering and predecessor analysis use one-pass bounded indexes instead of rescanning the full edge corpus per block.
- Dominator, structural-phi, phi-operand, and iterative value-flow work have explicit global/per-function/pass/cardinality budgets. When a budget is exceeded, HindSight preserves direct evidence and states that the expensive derived detail was omitted rather than publishing partial results as complete.
- Symbolication detection no longer materializes the entire Mach-O symbol-name table a second time, and linkage surfaces reuse the recovered indirect-symbol corpus.
- The report remains at 71 fields with `symbol_resolver`; engine identity advances to `arm64-analysis-v16-schema71` so older cached reports cannot masquerade as parameter/cast-analysis complete.

## 0.4.9 hotfix line

- **Hotfix 1:** repaired the shared `VGTagPill(text:systemImage:)` API so icon-bearing analysis-navigation pills compile without breaking text-only callers.
- **Hotfix 2:** made `VGInvestigationCollection` identity explicit at collection-backed SwiftUI `ForEach` sites, preventing accidental `Range<Int>` overload selection.
- **Hotfix 3:** bounded Function Neighborhoods presentation and worker-side detail materialization. Generic evidence surfaces initially expose 200 interactive rows in 200-row batches; Function Neighborhoods keeps full connectivity counts while materializing detail only for the top 256 connected functions, capped at 12 incoming and 12 outgoing detailed edges each.
- **Hotfix 4 / audit closure:** makes macOS target type-check coverage topology-derived instead of hand-maintained, so `AnalysisNavigation.swift`, `AnalysisContinuity.swift`, and future generated-target sources cannot silently escape the release gate. Project `objects`, `staging`, and `analysis-cache` directory components now explicitly reject symbolic-link substitution; staged capture and cache publication also require regular non-symlink files.
- **Hotfix 5 / Intel compatibility:** builds universal arm64+x86_64 app/service/Rust/VectorEmu artifacts. Cross-host static ARM64 analysis stays enabled; the Apple-Silicon Hypervisor backend and current ARM64 native debugger are explicitly disabled in the x86_64 process slice at UI and service/native boundaries.

## Stable-source invariants

- Product/service/worker version `0.5.11`, release channel `stable`, bundle build `5911`, protocol ABI 1.
- Final analysis transport is the exact 71-field report contract.
- Analysis progress is operational telemetry, not evidence and not an ETA.
- Coordinator transport health and project-session readiness remain separate states.
- Responsive wide/medium/compact topology remains required at the 900×680 supported minimum.
- Per-function dominator analysis remains bounded; Function Neighborhoods detail is bounded; candidate evidence is never silently promoted.
- Project schema remains version 1; continuity schema 5 reads schemas 1–5.
- Project-local cache/continuity/capture paths fail closed on symbolic-link boundaries.
- Production Swift crash primitives remain rejected by source validation.
- macOS app and Coordinator type-check source sets are derived from `Config/project-topology.json`; the generated topology is the source of truth.

## Distribution status

The source archive is portable/source validated in this environment, but this Linux container cannot complete the macOS-only release matrix. Before distribution run `./scripts/test-release-macos.sh` with Developer ID/notarization credentials and complete the dual-host Intel + Apple-Silicon matrix, including real-target, visual, Function Neighborhoods stress, symlink-boundary, universal-slice, and 8 GiB pressure checks in [`RELEASE_READINESS.md`](RELEASE_READINESS.md).

Enterprise deployment and support boundaries are documented in [`ENTERPRISE_OPERATIONS.md`](ENTERPRISE_OPERATIONS.md). Managed defaults and signed-manifest publication remain operator/release-host responsibilities; HindSight does not perform background diagnostic uploads.

Extension governance is active: capability allowlisting and managed `allow`/`verified-only`/`disabled` modes are enforced by Coordinator, with policy status visible in Extension Manager.

## Experimental surfaces

A12/T2/SEP research virtualization profiles and external full-system iPhemeral integration remain experimental. Stable HindSight product identity does not imply 1:1 hardware equivalence for those profiles.

## Evidence Core

Typed evidence indexing remains the primary in-process navigation/search representation. The current compatibility/cache transport is the 71-field worker report; historical 66-field details remain documented in `EVIDENCE_CORE_0.5.0.md`. Search, triage, exact related-evidence lookup, and collection-health membership avoid reparsing the full report on every SwiftUI recomputation.
