# HindSight 0.4.0-beta.14 — Analysis evidence-quality audit

## Scope

Beta.14 builds on beta.13's bounded per-function dominator solver. The objective is to improve the usefulness and honesty of large-binary analysis without widening speculative claims: expose how much of the image was actually analyzed, preserve direct-call evidence that leaves the bounded scan, distinguish proven calls from candidate cross-function transfers, and make local call neighborhoods easier to navigate.

## Corrections and enhancements

- Added **Analysis Quality**, a bounded report containing executable scan coverage, decoder coverage, data-in-code exclusions, function/basic-block counts, direct BL count, tail-call-candidate count, relocation count, validated PC-relative-reference count, and dominator budget omissions.
- Added explicit evidence classes (`observed/container`, `decoded/derived`, `validated-reference`, `candidate`) and a guardrail that decoder coverage is not semantic completeness.
- Direct BL collection now retains targets outside the bounded executable scan as external outgoing evidence instead of discarding them.
- Function Intelligence only attributes an incoming edge when the target is actually within the admitted executable range. Out-of-range BL targets remain external and cannot inflate the last internal function.
- Added **Tail-Call Candidates**. A row is emitted only for an unconditional direct `B` whose source belongs to one admitted function and whose target is exactly the entry of a different admitted function. The result remains a candidate, not a proven call/return semantic claim.
- Added **Function Neighborhoods**, a one-hop view joining direct BL callers/callees with separately labelled direct-B tail-call candidates. External BL targets remain visible on the outgoing side.
- Extended Analysis Insights with quality/coverage context and cross-function candidate counts.
- Added all three result sections to global analysis search and bounded diagnostic export.
- Extended the exact `analysis_result` schema from 63 to **66 fields** while retaining protocol ABI 1 and the existing 8 MiB worker / 12 MiB XPC ceilings.
- Added `analysis_quality_ledger`, `function_neighborhoods`, and `cross_function_tail_candidates` capabilities.
- Product/protocol identity is `0.4.0-beta.14`; Apple bundle build is `4014` with `CFBundleShortVersionString=0.4.0`.

## Validation completed in this environment

- `scripts/validate-source.py`: PASS.
- Python/native unittest suite: **92 passed, 6 skipped**. The six skips are optional LLVM-oracle differential checks unavailable in this container.
- Portable Swift contracts: PASS, including the **66-field** AnalysisReport contract, Analysis Status, Disassembly, CFG, Binary Workspace, Binary Diff, Runtime Correlation, Trace Explorer, Evidence Graph, Investigation Ledger, and Target model contracts.
- Full Swift syntax parsing of shipped `App`, `Shared`, and `PlatformServices` sources: PASS.
- Capability schema: **192 fields**, generated from and matching the bridge payload.
- Portable Hypervisor API-stub compile contract: PASS.
- Cargo lockfile structural preflight: PASS.
- VectorEmu Rust 1.90 MSRV source preflight: PASS.
- Shell syntax checks: PASS.
- Release identity/version checks: PASS.

## Release-host gates not claimed here

This environment does not provide Cargo/Rust or macOS/Xcode. The following remain Apple Silicon release-host gates and are deliberately not represented as passed:

1. Rust workspace build/test/clippy for the worker/bridge and VectorEmu.
2. Xcode Debug and Release builds for the app and CoordinatorService.
3. Re-analysis of the real firmware image that exposed beta.12 memory pressure, including observation of peak RSS on the 8 GiB M2 baseline.
4. Real debugger, Hypervisor.framework, and VectorEmu execution tests.
5. Release signing and notarization.

The beta.14 source contracts specifically require that candidate direct-B transfers remain labelled candidates and that external direct-call targets cannot be assigned as incoming evidence to an internal function.
