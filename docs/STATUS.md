# HindSight 0.5.7 Status

## Current milestone: stable universal-macOS analysis/review baseline

HindSight 0.5.7 is the current stable source identity (`stable`, protocol ABI 1, bundle build `5700`). The evidence engine is `arm64-analysis-v15-schema71` with the exact 71-field `AnalysisReport`, analysis-cache envelope schema 3 (with schema-2 backward reads), project SQLite schema 1, and analysis-continuity schema 5.

The 0.4.9 feature line added bounded analyst Follow-Up Queue workflow over the existing collection/review system. Older analysis caches are reused only when their source and engine identities match the current engine; the 0.5.3→0.5.7 engine bump intentionally invalidates prior analysis results so the new scalability budgets and hang fixes are not bypassed by cache reuse.

## 0.5.7 unified symbol resolution

- Resolver precedence is exact `nlist` code symbol → exact indirect-symbol name → validated unnamed function start.
- Stub/pointer aliases are retained; dyld import/export names lacking a proven absolute address remain name-only evidence.
- Resolver output is bounded and feeds the typed evidence index, address/symbol pivots, navigation, and linkage-stage fingerprints.
- dSYM/DWARF source-line recovery remains explicitly incomplete.

- Analysis JSON decoding, XPC reply-size accounting, and continuity-channel fingerprinting no longer perform multi-megabyte CPU work on the SwiftUI main actor.
- CFG rendering and predecessor analysis use one-pass bounded indexes instead of rescanning the full edge corpus per block.
- Dominator, structural-phi, phi-operand, and iterative value-flow work have explicit global/per-function/pass/cardinality budgets. When a budget is exceeded, HindSight preserves direct evidence and states that the expensive derived detail was omitted rather than publishing partial results as complete.
- Symbolication detection no longer materializes the entire Mach-O symbol-name table a second time, and linkage surfaces reuse the recovered indirect-symbol corpus.
- The report advances from 70 to 71 fields with `symbol_resolver`; engine identity advances to `arm64-analysis-v15-schema71` so older cached reports cannot masquerade as resolver-complete.

## 0.4.9 hotfix line

- **Hotfix 1:** repaired the shared `VGTagPill(text:systemImage:)` API so icon-bearing analysis-navigation pills compile without breaking text-only callers.
- **Hotfix 2:** made `VGInvestigationCollection` identity explicit at collection-backed SwiftUI `ForEach` sites, preventing accidental `Range<Int>` overload selection.
- **Hotfix 3:** bounded Function Neighborhoods presentation and worker-side detail materialization. Generic evidence surfaces initially expose 200 interactive rows in 200-row batches; Function Neighborhoods keeps full connectivity counts while materializing detail only for the top 256 connected functions, capped at 12 incoming and 12 outgoing detailed edges each.
- **Hotfix 4 / audit closure:** makes macOS target type-check coverage topology-derived instead of hand-maintained, so `AnalysisNavigation.swift`, `AnalysisContinuity.swift`, and future generated-target sources cannot silently escape the release gate. Project `objects`, `staging`, and `analysis-cache` directory components now explicitly reject symbolic-link substitution; staged capture and cache publication also require regular non-symlink files.
- **Hotfix 5 / Intel compatibility:** builds universal arm64+x86_64 app/service/Rust/VectorEmu artifacts. Cross-host static ARM64 analysis stays enabled; the Apple-Silicon Hypervisor backend and current ARM64 native debugger are explicitly disabled in the x86_64 process slice at UI and service/native boundaries.

## Stable-source invariants

- Product/service/worker version `0.5.7`, release channel `stable`, bundle build `5700`, protocol ABI 1.
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

## Experimental surfaces

A12/T2/SEP research virtualization profiles and external full-system iPhemeral integration remain experimental. Stable HindSight product identity does not imply 1:1 hardware equivalence for those profiles.

## Evidence Core

Typed evidence indexing remains the primary in-process navigation/search representation. The current compatibility/cache transport is the 71-field worker report; historical 66-field details remain documented in `EVIDENCE_CORE_0.5.0.md`. Search, triage, exact related-evidence lookup, and collection-health membership avoid reparsing the full report on every SwiftUI recomputation.
