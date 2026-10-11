# HindSight 0.4.0-beta.12 — Correctness / completeness audit

## Release scope

Beta.12 is an analysis-synthesis release built on the beta.11 responsive-workbench architecture. It also incorporates the compile fix reported against beta.11 `App/ContentView.swift`: the VectorEmu run-step control binds a `UInt64` value/range while its SwiftUI `Stepper` increment is now the required `UInt64.Stride` (`Int`) rather than `UInt64`.

The new analysis layer deliberately improves **synthesis and navigation**, not speculative decompilation. Existing raw evidence remains available unchanged.

## New analysis evidence

### Function Intelligence

The worker produces a bounded row for each admitted discovered function with:

- discovered start and bounded size;
- incoming and outgoing direct BL edges within the bounded code scan;
- non-fallthrough direct branch count;
- basic-block count;
- direct self-call count; and
- a transparent attention score.

The score is exactly:

`5 × incoming + 3 × outgoing + 2 × branches + basic blocks + min(size / 64, 25)`

This is a navigation/triage heuristic only. The UI and worker output explicitly state that it is not confidence, vulnerability severity, maliciousness, semantic importance, or runtime reachability.

### Validated PC-relative data references

The worker now resolves bounded ARM64 references for:

- `ADR` signed PC-relative immediates;
- **immediately adjacent** 64-bit `ADRP` + `ADD (immediate)` pairs using the same source register; and
- literal-immediate reference sites.

A record is published only when the final virtual address maps into a parsed Mach-O section that is not marked as containing instructions. Pending ADRP evidence expires immediately when it is not consumed by the next instruction and is cleared across control-flow boundaries. This intentionally sacrifices recall to avoid propagating an address through unknown semantics.

String annotation is allowed only after the target is proven to be inside one of the supported string-bearing Mach-O sections and a bounded read succeeds.

### Analysis Insights

Analysis Insights is a synthesis report over existing facts. It surfaces bounded counts, the highest-ranked structural function rows, and representative validated data references. It adds an explicit interpretation guardrail and does not manufacture new semantic claims.

## Transport contract

The release-matched `analysis_result` schema grows from 60 to **63 exact fields**:

- `function_intelligence`
- `data_references`
- `analysis_insights`

Each added section remains below 64 KiB under `AnalysisReport.matches`. Worker analysis frames remain capped at 8 MiB and the enclosing XPC analysis reply at 12 MiB. Protocol ABI remains 1, but exact product-version and exact analysis-field parity mean beta.11 and beta.12 components must not be mixed.

The bridge publishes three matching capabilities: `function_intelligence`, `pc_relative_data_references`, and `analysis_insights`. The generated capability schema contains **186 fields**.

## UI integration

The three new evidence surfaces use the beta.11 responsive shell rather than adding fixed chrome:

- **Analysis Insights** is promoted in the Static Analysis sidebar.
- **Function Intelligence** and **Data References** are available through the workspace catalog / command palette.
- All three participate in global analysis search/navigation.
- Diagnostic analysis excerpts include the new synthesis fields.

No fixed-width three-pane composition was reintroduced.

## Verification completed in this environment

The packaged source passed:

- `scripts/validate-source.py`;
- **91 runnable Python/native/portable contract tests**;
- **6 optional LLVM-oracle tests skipped** because those specific optional test hooks are unavailable;
- exact Swift/Rust **63-field** analysis schema parity;
- portable Swift parsing for the modified app/shared sources;
- capability-schema regeneration/check;
- Hypervisor Apple-Silicon API-stub compile contract;
- Cargo lockfile structural preflight;
- VectorEmu Rust 1.90 MSRV source preflight; and
- shell syntax validation.

### Independent ARM64 immediate sanity check

Clang/LLVM was used independently of the HindSight worker to assemble and link AArch64 reference instructions. The beta.12 immediate arithmetic reproduced LLVM's resolved targets for:

- `0x100000c0` at `0x10268`: `ADR x0` → `0x10280`;
- `0x90ffff81` at `0x1026c` + `0x91096021` at `0x10270`: `ADRP` page `0x0` + `ADD #0x258` → `0x258`; and
- `0x580000a2` at `0x10274`: literal target → `0x10288`.

These known vectors are retained in `python/tests/test_analysis_synthesis.py` so the immediate arithmetic and conservative source guardrails remain part of portable regression testing.

## Gates not claimable here

This Linux container does not provide the pinned Cargo/Rust compiler toolchain or macOS/Xcode frameworks. Therefore the following remain release-host gates and are **not** represented as passed:

- compile/test/clippy of the modified Rust worker with the pinned Rust toolchain;
- Xcode Debug and Release type-check/build of the complete app and CoordinatorService;
- real Apple Silicon debugger, Hypervisor.framework, and VectorEmu execution;
- real ARM64 Mach-O beta.12 analysis smoke against symbol-rich and stripped binaries;
- 8 GiB long-run/memory-pressure qualification;
- Developer ID signing; and
- notarization/stapling.

`docs/MACOS_VALIDATION.md` contains the beta.12-specific manual smoke checks, including row-by-row verification of Function Intelligence and spot-checking emitted PC-relative references against disassembly and Mach-O section mappings.

## Release assessment

Within the portable/source-verifiable boundary, beta.12 is internally consistent and complete for its stated scope. The largest residual correctness risk is the uncompiled Rust delta: its syntax/types and real Mach-O behavior must still pass the pinned macOS/Cargo release host before RC consideration. The implementation is intentionally conservative so an uncertain address chain is omitted rather than promoted as evidence.
