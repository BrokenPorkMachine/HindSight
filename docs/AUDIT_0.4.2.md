# HindSight 0.4.2 Release Audit

## Release identity

- Product/version: **HindSight 0.4.2**
- Release channel: **stable**
- Apple bundle build: **4102**
- Coordinator/worker protocol ABI: **1**
- Analysis report transport: **66 exact fields**
- Capability schema: **200 required fields**
- Project database schema: unchanged from the 0.4.1 stable line

## Scope

0.4.2 combines two release themes that were validated together rather than shipped as separate patches:

1. **Observable staged analysis execution** for long firmware-scale jobs, including bounded progress telemetry, cache continuity, and atomic evidence publication.
2. **End-to-end UI density hardening**, prompted by real overlap on the Targets surface and expanded into a workbench-wide responsive-layout audit.

## Analysis execution audit

The isolated Rust worker keeps stdout reserved for the final framed analysis report. Operational progress is emitted as bounded `VG_PROGRESS` records on stderr. Coordinator recognizes those records, converts them to stage/percentage state, and removes them from crash diagnostics so progress text cannot masquerade as failure evidence.

Progress is deliberately a pipeline-position indicator rather than a time estimate. Final static evidence remains atomic: HindSight does not publish half-built xrefs, CFGs, or synthesis results merely to make the UI appear more active.

The project-local analysis cache advances to envelope schema 2. Cache identity separates the analysis engine (`arm64-analysis-v11-schema66`) from the producer version (`0.4.2`). A valid 0.4.1 cache may be admitted and migrated because 0.4.2 does not change the analysis engine or 66-field report schema. Invalid, oversized, malformed, symlinked, identity-mismatched, or unreadable cache state is treated as a cache miss and falls back to worker analysis.

## UI/UX root cause

The Targets overlap was not primarily a spacing defect. The responsive shell introduced in beta.11 could narrow a workspace pane to a few hundred points while some child surfaces still reserved multiple 140–170 point columns plus labels/actions in a single horizontal row. Those constraints were mathematically incompatible, so clipping or overlap was inevitable.

The correction is architectural: parent panes and their children now participate in the same responsive contract.

### Targets surface redesign

Targets no longer uses the old fixed-width address-space/pivot table composition. It now uses:

- adaptive target cards;
- wrapping capability tags;
- adaptive Address Space cards;
- stacked key/value evidence;
- horizontal-or-stacked pivot input/actions;
- adaptive pivot-result cards;
- wrapping action groups; and
- a two-pane topology that can restack rather than compress content below its usable width.

No target metadata is intentionally hidden to make the panel fit.

### Shared density primitives

0.4.2 adds reusable presentation primitives rather than one-off fixes:

- `VGFlowLayout` for compact action groups that wrap into additional rows;
- `VGKeyValueField` for metadata that remains legible in narrow cards;
- `VGTagPill` for wrapping capability/evidence tags;
- existing adaptive two-/three-pane topology, `ViewThatFits`, adaptive grids, and explicit overflow bands where the content itself is legitimately wider than the viewport.

The governing rule is now:

- **controls wrap or restack**;
- **metadata restacks**;
- **primary evidence canvases receive width priority**;
- **fixed evidence columns are acceptable only when the owning data surface explicitly scrolls/zooms horizontally or the width is intrinsic to the representation**.

### Workspaces audited/refactored

The same density assumptions were corrected across Targets, Debugger, Stop Context, Selected Symbol, Navigator/context inspection, Python scripting, Plugins, Automation SDK, Extensions Platform, Extension Manager, SoC Virtualization, VectorEmu, dyld cache tooling, IPSW Catalog, Firmware Keys, iPhemeral tooling, Hypervisor RE, Binary Workspace, Binary Diff, Graph, Trace Explorer, Runtime Correlation, Evidence Graph, Investigation Ledger, and overview/status metric bands.

High-density evidence tables and graph canvases retain their natural data widths only where their container owns overflow/zoom behavior; they are not shrunk into unreadable content.

## Regression protection

`python/tests/test_ui_density.py` adds source-level contracts that reject the concrete class of regression that caused Targets to overlap. The validator checks that:

- responsive flow/key-value/tag primitives remain present;
- Targets retains adaptive composition and does not restore the legacy fixed column widths;
- dense high-frequency workspaces use wrapping/adaptive control surfaces;
- ContentView does not reintroduce large fixed-width control columns; and
- known single-line overlap patterns remain absent.

`docs/UI_POLISH_0.4.2.md` documents the responsive design contract for future feature work.

## Validation completed in this environment

The final 0.4.2 source tree passed:

- **107 Python/native unit-test cases total: 101 passed, 6 skipped**;
- the 6 skips are the pre-existing optional LLVM-oracle checks where that oracle is not installed;
- source topology and release metadata validation;
- 66-field Swift/Rust analysis protocol contract;
- 11-phase analysis-status contract;
- target-model, disassembly, graph, Binary Workspace, Binary Diff, Runtime Correlation, Trace Explorer, Evidence Graph, and Investigation Ledger contracts;
- all shipped Swift files parse successfully with the available `swiftc`;
- portable Apple-Silicon Hypervisor API shim compile test;
- Cargo lockfile structural preflight;
- Rust 1.90 MSRV source-admission preflight for VectorEmu;
- shell syntax validation;
- capability-schema/bridge-response parity at **200 required fields**;
- 0.4.2 staged-analysis/cache regression contracts; and
- 0.4.2 UI-density regression contracts.

The aggregate `scripts/test-portable.sh` also correctly **fails fast** before the long build because this container has no Cargo executable. That is an environment qualification gap, not a reported passing Rust build.

## Release-host gates still required

Before public distribution, run the macOS release-host matrix on the intended Apple Silicon host. At minimum:

1. pinned Cargo test/clippy/offline build for the main Rust workspace;
2. VectorEmu Cargo check/test;
3. Xcode Debug and Release builds for app/services;
4. real Hypervisor, Debugger, Coordinator/XPC, VectorEmu, and firmware-analysis smoke tests;
5. UI visual qualification at **900×680, 1180×760, 1440×900, and full screen**, with the inspector shown and hidden, especially Targets/Address Spaces/Pivot, Debugger, VectorEmu, IPSW Catalog, Hypervisor RE, Binary Diff, Trace Explorer, and Runtime Correlation;
6. the 8 GiB memory-pressure matrix using representative large firmware/CFG inputs;
7. Developer ID signing, notarization, stapling, and final signature validation via `scripts/test-release-macos.sh`.

## Audit conclusion

0.4.2 resolves the reported Targets overlap at the layout-contract level rather than by adding another minimum width. The broader audit removes the same failure mode from the other dense workspaces and establishes reusable responsive primitives plus regression tests so new features can grow without consuming permanent horizontal space.

The staged-analysis work and UI refactor are source/contract validated together. The release is ready for the macOS/Apple-Silicon release-host qualification described above; those host-specific gates are intentionally not represented as completed in this environment.
