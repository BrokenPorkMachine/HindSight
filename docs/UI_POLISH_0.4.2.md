# HindSight 0.4.2 — UI density audit and Targets redesign

0.4.2 keeps the beta.11 wide/medium/compact workbench topology, but tightens the next layer down: **controls and metadata inside a responsive pane must also adapt**. A responsive split view is not sufficient if its child still assumes four fixed 150–170 pt columns or a single unbreakable toolbar row.

## Targets surface v2

The Targets workspace was the clearest failure case. Its selected-target pane could shrink to a few hundred points while **Address Spaces** still reserved fixed widths for name, kind, base, and size before rendering evidence text. The address-pivot result used the same pattern. That geometry could not fit by construction.

0.4.2 replaces that layout with:

- a wrapping action bar for refresh/open/export actions;
- card-style target selection that wraps identity text instead of truncating the entire row;
- wrapping capability pills;
- adaptive address-space cards with stacked Base / Size / Evidence fields;
- an adaptive Session Identity key/value grid;
- a horizontal-or-stacked address input using `ViewThatFits`;
- adaptive pivot-result cards rather than fixed table columns.

No target fact is discarded at narrow widths; presentation topology changes instead.

## Reusable density primitives

`VGFlowLayout` is the default for groups of compact actions that may no longer fit on one row. It wraps controls while preserving native SwiftUI control sizing. `VGKeyValueField` and `VGTagPill` provide compact metadata that can collapse to one column without overlap. Existing `VGAdaptiveThreePane`, `VGAdaptiveTwoPane`, and `VGHorizontalOverflowBar` remain the higher-level topology and evidence-overflow primitives.

The intended rule is:

1. **Controls wrap or restack.** They must not disappear behind siblings.
2. **Metadata restacks.** Key/value evidence is not forced into desktop-table widths inside a compact inspector.
3. **Evidence tables may stay fixed-width only when the evidence surface owns scrolling.** Disassembly addresses, words, mnemonics, register lanes, and graph canvases are examples where shrinking the data itself would reduce correctness/readability.
4. **Primary canvases keep priority.** Auxiliary panes dock or stack before the evidence canvas is compressed below useful size.

## Surfaces covered by the 0.4.2 pass

The density audit covers Targets, Debugger, Stop Context, Selected Symbol, Navigator/context inspection, Plugins, Extension Manager, Extensions Platform, Python tooling, SoC Virtualization, VectorEmu, dyld Cache Browser, IPSW Catalog, Firmware Keys, iPhemeral handoff, Hypervisor RE, Binary Workspace, Flow Graph, Trace Explorer, Runtime Correlation, Evidence Graph, Investigation Ledger, and Binary Diff.

Binary Diff summary metrics and runtime-correlation evidence pills use independently scrollable overflow bands because those are evidence/status strips rather than action controls.

## Regression contract

`python/tests/test_ui_density.py` prevents the specific failure class from returning. It requires the shared flow primitives, checks the Targets redesign for adaptive cards/fields, rejects the old fixed target table widths and known dense one-line control patterns, and verifies that the major high-density workspaces use wrapping/adaptive controls.

The portable source validator also checks this contract. Real visual qualification is still required on the macOS release host at 900×680, 1180×760, 1440×900, full-screen, and with the global inspector both visible and hidden.
