# HindSight 0.4.9 Hotfix 4 — Correctness & Completeness Audit Closure

## Baseline

This audit starts from **HindSight 0.4.9 Hotfix 3** and re-audits the shipped tree after the SwiftUI compile fixes and Function Neighborhoods responsiveness fix rather than relying on the pre-hotfix 0.4.9 release audit.

Product identity intentionally remains:

- Product/service/worker version: **0.4.9**
- Apple bundle build: **4109**
- Release channel: **stable**
- Protocol ABI: **1**
- Analysis report: exact **66-field** contract
- Analysis engine: **arm64-analysis-v11-schema66**
- Analysis cache envelope: **schema 2**
- Analysis continuity document: **schema 5** (reads schemas 1–5)
- Project SQLite schema: **1**
- Capability contract: **226 fields**

Hotfix 4 is an audit-closure/source-hardening package, not a protocol or evidence-schema release.

## Audit scope

The pass covered:

- generated Xcode topology and Swift target membership;
- macOS type-check/release gate completeness;
- all App/Shared/Coordinator Swift syntax;
- production Swift crash primitives;
- analysis protocol/schema/cache identity;
- Function Neighborhoods scalability and incremental presentation;
- project filesystem trust boundaries and capture/cache publication;
- continuity schemas 1–5 and review/follow-up invariants;
- native Mach-O/paging/cache/project SQL contracts;
- Rust 1.90 source admission and Cargo lockfile structure;
- Hypervisor.framework portable API shim contract;
- shell/build scripts and deterministic Xcode generation;
- current STATUS/VALIDATION/macOS/release-readiness documentation;
- archive manifest/integrity process.

## Findings and resolutions

### A-01 — P1: macOS app type-check gate omitted generated target sources — **resolved**

**Finding.** `scripts/test-app-typecheck.sh` used a hand-maintained source list that omitted `App/AnalysisNavigation.swift` and `App/AnalysisContinuity.swift`, despite both files belonging to the generated `HindSight` Xcode target. This explains how the `VGTagPill(systemImage:)` and collection-backed `ForEach` compile failures could escape the scripted type-check story and only appear in Xcode.

**Resolution.** Added `scripts/list-target-swift-sources.py`. Both app and Coordinator type-check gates now derive their Swift source lists directly from `Config/project-topology.json`, which is generated alongside the Xcode project. Portable regression tests require helper output to exactly match generated target membership and explicitly verify that Analysis Navigation/Continuity are covered.

**Result.** Current generated source counts are **32 Swift files for HindSight** and **14 Swift files for CoordinatorService**. Future generated-target Swift files cannot silently fall outside the type-check gate.

### A-02 — P1: intermediate project-directory symlink boundary was incomplete — **resolved**

**Finding.** The project root/database and individual cache/captured-file admission paths had symlink protections, but the intermediate `objects`, `staging`, and `analysis-cache` directory components were only verified as directories. A substituted directory symlink could redirect pathname resolution outside the project before final-file no-follow checks.

**Resolution.** `ProjectStore` now centralizes directory admission in `validatedDirectory(...)` and requires each project storage directory to be a real non-symlink directory. Additional hardening requires:

- staged capture input to be a regular non-symlink file with the expected size;
- existing analysis-cache destinations to be regular non-symlink files before atomic publication;
- captured-source lookup to validate the `objects` directory before returning a path;
- abandoned-staging cleanup to reject a symlinked staging directory.

Portable source contracts protect all of these conditions, and the Apple Silicon manual matrix now includes explicit directory/file substitution tests on a disposable project.

### A-03 — P2: current operational documentation was stale at 0.4.3 — **resolved**

**Finding.** `docs/STATUS.md` and `docs/VALIDATION.md` still described 0.4.3 as the current stable baseline even though the shipped tree was 0.4.9 Hotfix 3. This made the release contract incomplete and obscured the hotfix-specific qualification requirements.

**Resolution.** STATUS and VALIDATION were rewritten for the 0.4.9 hotfix baseline. `MACOS_VALIDATION.md` and `RELEASE_READINESS.md` now explicitly include topology-derived type-check coverage, Function Neighborhoods stress, and project-subdirectory symlink substitution tests. Source validation checks the current operational-document headers/required hotfix topics.

### A-04 — P2: stable-line candidate gate still emitted beta/RC readiness messaging — **resolved**

**Finding.** `scripts/test-beta-macos.sh` is still intentionally the stronger candidate package gate used by the stable release flow, but its terminal messaging referred to “beta qualification,” “RC promotion,” and `BETA_READINESS.md`, which is misleading on the stable 0.4.9 line.

**Resolution.** The gate now reports candidate qualification and directs stable distribution to `test-release-macos.sh` plus `docs/RELEASE_READINESS.md`. Its optional notarization behavior is unchanged.

### A-05 — Prior hotfix regressions re-verified — **resolved/guarded**

The audit re-verified the three earlier 0.4.9 hotfixes and their regression guards:

- **HF1:** `VGTagPill(text:systemImage:)` remains source-compatible with text-only callers.
- **HF2:** collection-backed `ForEach` sites use explicit collection identity rather than ambiguous `Range<Int>` overload resolution.
- **HF3:** evidence surfaces reveal interactive rows incrementally; Function Neighborhoods retains full connectivity counts while detailed edge materialization remains bounded to top connected functions and bounded in/out detail.

No regression in the 66-field analysis report, protocol ABI, cache identity, project database schema, or capability contract was introduced by Hotfix 4.

## Verification results

On the final audit-closure source tree in this environment:

- `scripts/validate-source.py`: **passed**.
- Full portable pytest suite: **147 passed**, **6 optional LLVM-oracle tests skipped**, **63 subtests passed**.
- Source validator's unittest/native contract pass: **115 tests**, **6 optional LLVM-oracle skips**.
- New release-gate/project-store audit contracts: **6/6 passed**.
- All **40** shipped Swift files under App/Shared/PlatformServices: syntax parse **passed** with Swift 6.2.1.
- Generated app target: **32 Swift sources**; generated Coordinator target: **14 Swift sources**; topology-derived helper matches both exactly.
- Deterministic Xcode project/topology regeneration: **byte-for-byte unchanged**.
- Exact Swift/Rust analysis protocol: **66 fields**.
- Capability schema check: **226 fields**, passed.
- Hypervisor Apple-silicon API shim compile contract: **passed**.
- Cargo lockfile structural preflight: **passed**.
- VectorEmu Rust 1.90 MSRV source preflight: **passed**.
- Shell syntax checks: **passed**.
- Active-source TODO/FIXME/HACK marker scan: **none**.

`scripts/test-portable.sh` correctly stops immediately in this container because **Cargo is not installed**. That is an environment prerequisite, not a passing Rust compile claim.

## Completeness review

### Analysis/evidence

No schema drift was found. The current line still preserves the bounded analysis model, per-function dominator budget, conservative candidate semantics, external-call accounting, Analysis Quality, Function Intelligence, Data References, Tail-Call Candidates, and bounded Function Neighborhoods detail.

### UI/navigation/review workflow

The responsive workbench contracts, Targets density fix, Analysis Triage/navigation, pins/history/collections/review/journal/follow-up continuity, and the 0.4.9 hotfix compile/responsiveness guards remain represented in source validation. No new fixed-layout overlap pattern was found by the existing density contract suite.

### Persistence/security

Project SQLite integrity checking, writer locking, bounded records/notes, captured-source identity, project-local cache identity, continuity-state bounds, and symlink-aware storage boundaries remain intact. Hotfix 4 closes the previously incomplete intermediate directory checks.

### Build/release engineering

The generated topology is now authoritative for macOS Swift type-check coverage. Debug/Release build, embedded worker/VectorEmu, signing, notarization, and Gatekeeper paths remain present in the release scripts. Current operational documentation now matches the actual stable/hotfix line.

## Remaining host-only qualification — not source defects

The following cannot be truthfully completed in this Linux container and remain mandatory before public macOS distribution:

1. Real Cargo build/test/clippy and embedded worker smoke on the pinned Rust 1.90 toolchain.
2. Xcode Debug/Release type-check + link of the 32-source app target and 14-source Coordinator target against the macOS SDK.
3. Reproduction of the prior Function Neighborhoods stress artifact on the 8 GiB M2 baseline, including memory-pressure observation and repeated evidence expansion/navigation.
4. Disposable-project `objects` / `staging` / `analysis-cache` symlink substitution tests on APFS/macOS.
5. Real Coordinator/XPC recovery, continuity schema migration, debugger, Hypervisor, VectorEmu, firmware-runtime and UI-size qualification.
6. Developer ID signing, notarization, stapling, and Gatekeeper assessment.

These are explicitly release-host gates; they are not marked passed by this audit.

## Conclusion

All actionable correctness/completeness issues discovered by this portable Hotfix-3 re-audit have been addressed in Hotfix 4. No unresolved portable/source-verifiable **P0/P1** defect remains in the audited tree. The package should still be treated as a **release-candidate source archive** until the mandatory Apple Silicon/macOS matrix above passes.
