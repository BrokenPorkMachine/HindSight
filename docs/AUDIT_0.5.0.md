# HindSight 0.5.0 — Evidence Core audit

## Scope

0.5.0 promotes the 0.4.9 Hotfix 5 Universal Mac baseline into an architecture/performance release. The goal is to remove repeated report-text parsing from the SwiftUI render path without forcing a project/cache/protocol flag day.

## Result

**Portable/source audit: PASS.**

The worker/Coordinator compatibility contract remains protocol ABI 1 with the exact 66-field `AnalysisReport` and `arm64-analysis-v11-schema66` engine identity. Existing schema-2 analysis caches remain admissible when source/engine identity validates. Product identity is **0.5.0 stable / build 5000**.

## Implemented architecture improvements

### Typed Evidence Core

- Added `VGEvidenceRecord` and immutable `VGAnalysisEvidenceIndex`.
- Each admitted evidence row carries provenance, workspace/kind, original text, pre-normalized search text, explicit parsed address/symbol where present, and a stable unique semantic/render identity.
- Index includes address and symbol lookup tables, source ranges, source-specific focus limits and precomputed focus ordering.
- Evidence corpus is bounded to **50,000 records**.
- Index construction runs through a utility-priority detached task and publishes atomically to the main actor.
- Search, Analysis Triage, Related Evidence, `filteredAnalysisLines`, and current collection-health membership consume the typed index instead of reparsing every report channel during SwiftUI recomputation.
- Search/Triage/Related surfaces expose an explicit indexing state so a not-yet-built index is never represented as “no evidence.”

### Memory bounding

- Complete compatibility `AnalysisReport` objects are retained in an **8-entry LRU** instead of growing with every analyzed artifact.
- The selected artifact owns the current typed evidence index; evicted reports can be restored through the existing project analysis cache path when needed (for example Binary Diff).

### Persistence write reduction

- Launch-session persistence is **350 ms debounced**. Rapid changes to search/address/CFG/symbol context no longer write UserDefaults on every keystroke.

### Universal-host baseline retained

- Xcode/Rust/VectorEmu architecture remains `arm64 + x86_64`.
- Cross-host ARM64 static analysis and software VectorEmu remain available on Intel.
- Native ARM64 debugger and Apple-Silicon Hypervisor/SoC execution remain fail-closed on x86_64 at UI and service/native boundaries.

## Compatibility

- Product/service/worker: **0.5.0 stable**
- Bundle build: **5000**
- Protocol ABI: **1**
- Analysis transport: exact **66 fields**
- Analysis engine/cache identity: **`arm64-analysis-v11-schema66`**
- Analysis cache envelope: **schema 2**
- Project SQLite schema: unchanged
- Analysis continuity schema: unchanged
- Capability schema: **238 fields**

## Validation executed in this environment

- Combined Python regression suite: **159 passed, 6 optional LLVM-oracle tests skipped, 63 subtests passed**.
- Legacy/native/source runner: **115 passed, 6 optional LLVM-oracle skips**.
- 0.5.0 Evidence Core contract tests: passed.
- All **42 shipped Swift source files** parse with `swiftc -frontend -parse`.
- Deterministic Xcode project/topology regeneration: byte-for-byte stable.
- Capabilities schema regeneration/check: passed (**238 fields**).
- Cargo lockfile structural preflight: passed.
- VectorEmu Rust 1.90 source-admission preflight: passed.
- Shell syntax validation: passed.
- Existing project-store symlink-boundary, Function Neighborhoods scaling, UI-density, continuity/review, paging, framing, Mach-O/native and universal-macOS contracts remain green.

## Environment-limited release gates

`./scripts/test-portable.sh` intentionally stops immediately in this container because **Cargo is not installed**. This is not reported as a passing Rust build. Before public distribution, run the documented release-host matrix on macOS:

- Cargo build/test/clippy for universal Darwin targets;
- Xcode Debug/Release type-check/link for arm64 and x86_64;
- real Intel and Apple-Silicon launch/runtime qualification;
- firmware-scale Evidence Core/Function Neighborhoods responsiveness and peak-RSS measurements;
- native ARM64 debugger and Hypervisor/SoC execution on Apple Silicon;
- Intel closeout checks for those host-bound features;
- Developer ID signing, notarization, stapling and Gatekeeper assessment.

## Remaining architectural roadmap

0.5.0 deliberately keeps the text `AnalysisReport` as a wire/cache adapter. The next major Evidence Core stages can move typed records into worker-side output/stage caches, add project-wide evidence persistence/FTS, and migrate plugin APIs to typed evidence without invalidating 0.5.0 projects.
