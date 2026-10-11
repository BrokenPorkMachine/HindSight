# HindSight 0.4.0-beta.11 — Responsive Workbench Re-architecture

## Why this release exists

HindSight had reached the point where adding capabilities to an existing fixed desktop composition was actively reducing usability. Several flagship workspaces assumed that a navigator, primary canvas, contextual inspector, application sidebar, and workbench inspector could all retain their preferred widths simultaneously. Once the window was smaller than that combined intrinsic width, SwiftUI had no meaningful way to preserve every control. The result was compressed panes, clipped headers, and controls that technically existed but were no longer practically visible.

Beta.11 treats that as an information-architecture defect rather than a padding defect. The engine, evidence model, Coordinator contracts, workspace identities, and command/search model remain intact. The composition model changes.

## New layout contract

HindSight now has three layout tiers for dense analytical workspaces:

- **Wide** — navigator, primary canvas, and inspector can remain side-by-side.
- **Medium** — the primary canvas keeps priority. Navigator and inspector collapse into one switchable auxiliary dock.
- **Compact** — the auxiliary dock moves below the primary canvas using a vertical split. Nothing is scaled to illegibility merely to preserve a three-column picture.

This is implemented by `VGAdaptiveThreePane` in `App/WorkbenchChrome.swift`. Two-pane report/tool surfaces use the companion `VGAdaptiveTwoPane`, which converts a horizontal split to a vertical split below its breakpoint.

The breakpoints are workspace composition decisions, not claims about physical display classes. They are based on the actual width available to the workspace after navigation chrome is accounted for.

## Surfaces migrated in beta.11

The adaptive three-pane contract now covers:

- Binary Workspace
- Flow Graph / CFG
- Trace Explorer
- Runtime Correlation
- Evidence Graph
- Investigation Ledger
- Selected Symbol evidence desk

The adaptive two-pane contract now covers:

- Binary Diff master/detail and side-by-side code evidence
- Debugger registers/live-code and memory/process-map regions
- Targets browser/detail
- Python stdout/stderr
- Plugin stdout/stderr
- Extensions Platform detail
- SoC Virtualization profile/limitations
- VectorEmu register/code and region/protocol desks
- Stop Context live-code/backtrace

## Workbench shell changes

The application shell no longer requires an 1180-point minimum width. The beta.11 minimum is 900 × 680 and the default opens at 1480 × 920.

The global contextual inspector is also responsive:

- at wider detail widths it remains a trailing split;
- below 1040 points of detail width it becomes a bottom split;
- Focus Mode still removes navigation and inspector chrome for sustained code reading.

The header is now two-tiered. Workspace identity and the primary analysis action occupy the first row. Address navigation, task layout, mentor mode, inspector, Focus Mode, command palette, and cancellation occupy a separate action row. This prevents one increasingly long intrinsic-width toolbar from determining the width of the whole application.

The status surface is also two-tiered. Coordinator/target/job indicators live on the first line and can overflow safely; the explanatory status message has its own line instead of being squeezed between pills and the release identifier.

## Dense toolbar rule

Workspace toolbars may be information dense, but they must not rely on a single unbounded horizontal `HStack` for all actions. Beta.11 reorganizes the primary toolbars in Binary Workspace, CFG, Trace Explorer, Runtime Correlation, Evidence Graph, Investigation Ledger, Binary Diff, Debugger, and Targets into semantic rows.

Metric/status bands use `VGHorizontalOverflowBar` where the content is intrinsically tabular or summary-like. This keeps the primary canvas width independent from the aggregate width of every metric label.

## What does not scale

Machine evidence, addresses, register values, disassembly columns, graph nodes, and other evidence-bearing content are not visually shrunk just to fit a smaller window. Where a representation has a legitimate minimum width, it remains horizontally scrollable or becomes the primary pane while auxiliary controls move elsewhere.

That distinction is deliberate: responsive layout changes *composition*, not the meaning or legibility of evidence.

## Regression contract

`scripts/validate-source.py` now rejects a release that removes the adaptive layout primitives or migrates the six original dense flagship workspaces back to fixed three-column composition. It also verifies the responsive global inspector, the lower minimum window size, and the published capability flags:

- `adaptive_workspace_topology`
- `responsive_workbench_chrome`
- `overflow_safe_metric_bands`

The macOS release-host matrix should test at least 900 × 680, 1180 × 760, 1440 × 900, and a full-screen/large external-display width while exercising every migrated surface.
