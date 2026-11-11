# HindSight 0.6.0 — Workspace Space Allocation Contract

Status: **development design + first code increment**, not an Apple-Silicon UX certification.

## Product rule

**Content gets the space. Extra surfaces earn their place.** Do not multiply
simultaneous panels just because the data exists. Keep one main investigation
surface; bring out a secondary view only when simultaneous correlation,
comparison, or live context improves the task.

### Default topology by task

| Investigation task | Primary surface | Secondary surfaces | Default |
| --- | --- | --- | --- |
| Overview, imports, settings, notes | One readable document / list | Optional global inspector when context is requested | Single |
| Disassembly / Binary Workspace | Code viewer | Function browser, semantic reconstruction and function context | Code-first, optional browser/context; switch to split comparison deliberately |
| CFG / Evidence Graph | Large zoomable graph canvas | Functions and semantic/evidence details | Canvas-centered; auxiliary dock collapses on compact windows |
| Trace Explorer / Runtime Correlation | Selected observation and code correspondence | Timeline and registers/memory evidence | Responsive multi-surface when wide; one on-demand auxiliary view when compact |
| Binary Diff | Paired, linked binaries | Per-side metadata | Side-by-side only when both sides remain legible; otherwise selectable comparison |
| Investigation Ledger | Entries and current selected evidence | Details, source context and review actions | Master-detail when space permits; selected view when compact |
| Warhead Community | Current investigation remains primary | Conversation or result with citations | Opt-in agent panel, never permanently steals half the investigation |
| Debugger / emulator | Active machine state or code | Registers, memory, timeline when simultaneous inspection is necessary | Task-specific multi-surface; never force all panes simultaneously |

## Source and first implementation

- `VGAdaptiveThreePane` in `App/WorkbenchChrome.swift` already moves wide
  3-pane layouts to a center + switchable dock at medium sizes.
- The first 0.6.0 increment **keeps the center canvas alone on compact layouts**
  and offers the navigator/inspector as a labeled on-demand popover.
- `ContentView.swift` no longer displays an additional global inspector
  *by default* beside integrated desks (`CFG`, `Binary Workspace`,
  `Runtime Correlation`, `Trace Explorer`, `Evidence Graph`,
  `Investigation Ledger`). The toolbar toggle still exposes it, and
  per-workspace choices survive navigation within the session.
- `BinaryWorkspace.swift` opens in a full-height **Code** view rather than
  automatically splitting already-constrained vertical space with semantic
  output. **Split** and **Lifted** remain selectable through the existing picker.
- The workbench no longer forces `minHeight: 650` on the five integrated
  workflows when hosted inside a 680-point-height main window with headers,
  tab bar and status bar.

## Layout principles

1. **One authoritative selection.** Function, address, artifact and runtime
   identities synchronize across the compatible views. A panel is not a new
   project or new investigation.
2. **Main surface first.** Optimize the minimum visible central content area;
   do not distribute equal widths merely because there are three panes.
3. **Multi-surface is earned.** Allow simultaneous views for graph + evidence,
   disassembly + pseudocode, registers + memory, trace + correlating code and
   before/after diff. An ordinary settings page needs none.
4. **User control.** All auxiliary panes are reversible, resizable when docked,
   keyboard reachable and collapsible. Focus Mode should temporarily hide
   chrome without losing the user's preferred global-inspector choice.
5. **Narrow windows are not mini-desktops.** When the central canvas would
   become unreadable, collapse auxiliary panels into an on-demand chooser,
   not a permanent vertical stack or overlapping chrome.
6. **No pretend space savings.** A canvas must still permit selecting, zooming,
   panning, scrolling and copying evidence at the supported minimum size.
7. **Avoid nested fixed minima.** A child cannot claim an intrinsic height
   greater than the usable post-header viewport without an explicit scroll
   strategy.
8. **Do not force unrelated surfaces into the same frame.** An editor, terminal,
   preferences, or tutorial page does not need a permanent second inspector.
9. **Preserve orientation.** Switching panes must not wipe the selected function,
   graph viewport, scroll position, trace sample or unsaved notes where stable.
10. **Accessibility is a release gate.** Verify VoiceOver names, keyboard focus,
    contrast, minimum hit targets, reduced-motion transitions and pane-toggle
    announcements on the real UI; no source scan substitutes for this.

## Qualification matrix (NOT RUN)

- [ ] 900 × 680: graph readable and navigable without permanently occupying a
      third of height with an auxiliary pane; auxiliary pane can be opened,
      switched, closed and used from keyboard and VoiceOver.
- [ ] 900 × 680: Binary Workspace opens with a full-height code area; Split and
      Lifted remain available without clipping or losing selection.
- [ ] 1180 × 760: no overlap between main canvas and dock, no pane narrower
      than its useful minimum; window resize during analysis does not hang.
- [ ] 1440 × 900: relevant multi-surface layouts work, resize smoothly and
      preserve graph selection, code address and trace sample.
- [ ] Full screen and secondary display: native splitters preserve editor
      legibility and selected context.
- [ ] Inspector toggle persists per workspace during the session and Focus Mode
      does not destroy that choice.
- [ ] Binary Diff: both source and target values remain distinguishable;
      insufficient widths use a usable alternative to clipping.
- [ ] Verify dynamic type / accessibility text enlargement does not hide
      essential tool buttons. Horizontal overflow must be explicit.
- [ ] Test Project/Notes/Settings as **single-surface** screens and confirm
      no unnecessary sidebar duplication.
- [ ] Capture annotated screenshots at each reference size. Fail any scenario
      with overlapping, clipped or inaccessible primary controls.

## Next technical increments

1. Run macOS compile/source checks and capture interactive screenshots.
2. Add persistence for workspace-specific *internal* dock preferences/widths;
   validate no cross-project selection confusion.
3. Make Binary Diff responsive with stacked/alternating modes at narrow sizes,
   retaining selection synchronization and evidence provenance.
4. Review and streamline remaining `ContentView.swift` oversized, scroll-wrapped
   workspaces; route dense research surfaces through explicit canvas contracts.
5. Validate keyboard, VoiceOver and resize/freeze behavior with an 8 GiB
   Apple-Silicon device, using reproducible cases rather than intuition.

This is part of `INVESTIGATION_COMPLETE_0.6.0.md` W4 and the sole
authoritative `MASTER_ROADMAP.md` product-completion milestone.
