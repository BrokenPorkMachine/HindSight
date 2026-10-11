# HindSight 0.4.0-beta.11 — Responsive Workbench Audit

**Release:** 0.4.0-beta.11  
**Bundle build:** 4011  
**Protocol ABI:** 1  
**Capability fields:** 183

## Executive result

Beta.11 is an end-to-end UX/UI re-architecture of the HindSight workbench shell. The audit found that the prior density problem was architectural: multiple workspaces required two or three fixed horizontal panes, long intrinsic-width toolbars, the application sidebar, and the global inspector to coexist at once. Their combined minimum widths could exceed the usable center canvas, causing controls and evidence regions to be compressed or disappear from practical view.

The release replaces that assumption with adaptive composition. It preserves the engine, project model, Coordinator/XPC semantics, evidence semantics, workspace catalog, command palette, and recovery/session contracts while changing how those capabilities are presented at different widths.

## Responsive architecture

### Adaptive three-pane topology

`VGAdaptiveThreePane` provides three presentation tiers:

- **Wide:** navigator, primary canvas, and inspector remain simultaneously visible in a horizontal split.
- **Medium:** the primary canvas remains dominant while navigator and inspector share a switchable auxiliary side dock.
- **Compact:** the auxiliary dock moves below the primary canvas in a vertical split.

This prevents auxiliary chrome from forcing the evidence canvas below a usable width.

Migrated surfaces include Binary Workspace, Flow Graph/CFG, Trace Explorer, Runtime Correlation, Evidence Graph, Investigation Ledger, and Selected Symbol.

### Adaptive two-pane topology

`VGAdaptiveTwoPane` changes a horizontal primary/secondary split into a vertical split below its configured breakpoint. It is used by Binary Diff, Debugger regions, Targets, Python/plugin output, Extensions Platform, SoC Virtualization, VectorEmu, and Stop Context.

### Global workbench shell

The global contextual inspector remains a trailing pane when the available detail width is at least 1040 points and bottom-docks below that threshold. The window minimum is now 900 × 680 rather than 1180 × 760, while the default launch size is 1480 × 920.

The global header is split into semantic rows so workspace identity/primary analysis and secondary navigation/layout commands no longer compete in one unbounded horizontal line. The status surface similarly separates state pills from explanatory status text.

### Overflow-safe dense content

`VGHorizontalOverflowBar` is used for intrinsically wide metric/status bands. Dense toolbars across major analysis surfaces have been reorganized into semantic rows rather than one long `HStack`.

Evidence-bearing content is intentionally not scaled down to preserve a fixed desktop picture. Addresses, register values, disassembly columns, graph nodes, and similar evidence retain legible dimensions; when their intrinsic width matters, the evidence region scrolls while auxiliary controls change topology.

## Correctness and completeness checks

The beta.11 source tree was regenerated with the deterministic Xcode project generator and then validated from the regenerated topology.

Verified in this environment:

- 87/87 runnable Python/native tests passed.
- 6 optional LLVM-oracle differential tests skipped because the oracle is not installed; their non-oracle golden tests passed.
- Swift parser validation passed for `App/*.swift` and `Shared/*.swift`.
- Source topology and local dependency validation passed.
- Capability schema matches the bridge response and contains 183 fields.
- Cargo lockfile structural preflight passed.
- VectorEmu Rust 1.90 MSRV source preflight passed.
- Portable Hypervisor shim compile contract passed against the Apple-silicon API stub.
- Shell syntax validation passed.
- Responsive regression guards verify both adaptive three-pane and adaptive two-pane primitives, migrated flagship workspaces, the responsive global inspector, the 900 × 680 minimum, and the beta.11 capability flags.
- Product identity is consistently `0.4.0-beta.11`; bundle build identity is 4011.

The aggregate portable release script intentionally fails fast in this Linux sandbox because Cargo is not installed. The observed failure is the prerequisite guard:

`error: HindSight: Cargo not found. Install Rust with rustup or set VECTORGLASS_CARGO to the cargo executable.`

That result is environmental and is not represented as a passing Rust build.

## Protocol impact

The responsive UI does not add or change XPC methods or protocol payload fields. Protocol ABI remains 1. Exact product-version/release-channel handshake compatibility still applies, so the app and Coordinator both identify as beta.11. The new bridge flags `adaptive_workspace_topology`, `responsive_workbench_chrome`, and `overflow_safe_metric_bands` advertise presentation/release capabilities; they are not service action endpoints.

## Remaining macOS release-host gates

The following require the actual macOS/Apple Silicon release host and are therefore **not** claimed as passed here:

- pinned Cargo build/test/clippy/release matrix;
- Xcode Debug and Release build/link/run;
- real XPC Coordinator lifecycle and interruption testing;
- real Hypervisor.framework / Debugger / VectorEmu execution;
- the width matrix at 900 × 680, 1180 × 760, 1440 × 900, and large/full-screen sizes;
- 8 GB memory-pressure qualification;
- code signing, hardened runtime, packaging, and notarization.

## Release conclusion

Beta.11 removes the fixed-layout assumption that caused the packed-surface failure. The workbench now changes composition as width decreases instead of continuously compressing every pane. The key invariant for future HindSight releases is that feature growth must not restore mutually incompatible fixed-width panes or single-row control bands; new surfaces should use the adaptive layout primitives or provide an equivalent progressive-disclosure strategy.
