# HindSight Workbench UX System

HindSight 0.3 replaces the original utility-style shell with an investigation-oriented workbench. The UI should optimize for two users at once: a new analyst who needs strong orientation and an expert who needs low-friction access to every capability.

## Core interaction model

1. **Investigation first.** The project and captured sources live in the sidebar as persistent context. Tools never replace the identity of the investigation.
2. **Curate the common path.** The sidebar exposes the highest-frequency tools by analyst job: Start, Static Analysis, Dynamic Analysis, Machine Lab, Automation, Evidence & Notes.
3. **Do not hide power.** Every specialist surface remains available from the global Command Palette (`⌘K`) and sidebar filter.
4. **One active workspace.** The document header names the current workspace, its purpose, current artifact, and architecture. Navigation history is explicit.
5. **Evidence follows selection.** Contextual evidence and extension panels belong in the inspector, not scattered across unrelated windows.
6. **Persistent system state.** Coordinator health, selected unified target, active request, and release identity remain visible in the bottom status surface.
7. **Progressive disclosure.** Primary actions are visible; advanced actions live in menus, command search, or contextual panels. A user should not have to scan eighty controls to find the next move.
8. **Keyboard parity.** Anything hidden from the primary navigation must remain searchable and invokable without the mouse.
9. **Uncertainty is UI state.** Proven, correlated, modeled, unresolved, and unavailable evidence should remain distinguishable. Visual polish must never make a weak inference look stronger.
10. **Dense, not cramped.** Analysis views can be information-dense, but chrome should stay quiet. Typography, spacing, iconography, and material surfaces establish hierarchy instead of excess borders.

## Navigation hierarchy

### Start
- Investigation Home
- Targets
- Selected Symbol

### Static Analysis
- Search & Navigate
- Disassembly
- Functions
- Control Flow
- Strings
- Cross References
- Runtime Graph
- Symbol Provenance

Specialist SSA, linker, Objective-C, Swift, dyld, ARM64e, and provenance tools remain in `⌘K`.

### Dynamic Analysis
- Debugger
- Live Disassembly
- Threads
- Memory Map
- Loaded Images
- Stop Context

### Machine Lab
- VectorEmu
- Hypervisor
- Firmware Lab
- SoC Virtualization
- dyld Cache
- IPSW Catalog

### Automation
- Python Console
- Extensions
- Extension Manager

### Evidence & Notes
- Notes

## Visual system

- Use native macOS materials rather than custom faux-window chrome.
- Use one rounded panel language for GroupBox-based tools.
- Accent color indicates active navigation and primary actions, not decoration.
- Monospaced text is reserved for addresses, bytes, registers, logs, and machine evidence.
- Rounded system typography is used selectively for major workspace titles and dashboard cards.
- Empty states explain the next useful action instead of showing blank panels.
- The main canvas consumes available width. Artificially narrow analysis columns are avoided.

## Command palette

`⌘K` is the escape hatch for the complete platform. Search matches tool name, group, purpose, and keywords. The palette must remain fast enough to be the preferred navigation method for expert users.

## Inspector

The inspector is contextual, optional, and persistent across workspace changes. It contains:

- current context summary,
- selection/evidence results,
- extension-provided inspector panels,
- pin state.

The inspector should never become a second primary navigation system.

## Release rule

A HindSight release must not reintroduce the old monolithic workspace picker as the primary navigation surface. The source validator checks the workbench shell, command palette, catalog, inspector, and status surface as release-contract features.

## Analyst Desk additions (0.3.1)

- **Workspace tabs:** recently used workspaces persist as a bounded tab strip; tool switching no longer destroys the user's spatial context.
- **Structured code surface:** disassembly is rendered as address/offset/word/instruction evidence rows rather than a monolithic text blob. Selecting a row updates the shared address/evidence context.
- **Focus Mode:** the sidebar and contextual inspector can collapse together for sustained code reading without changing the investigation or active target.
- **Inspector modes:** Context, Evidence, and Target are separate mental models. Target mode exposes backend state, capabilities, and address spaces without forcing a trip to the Targets workspace.
- **Dense center, quiet chrome:** the center canvas may be information-dense; navigation and status surfaces remain visually restrained.

## Graph Workspace additions (0.3.2)

The CFG workspace is a spatial analysis instrument rather than a report page. Basic blocks are rendered from a typed Foundation-only graph model; edge types remain explicit, unresolved targets are not invented, and the canvas is bounded for predictable resource use.

- **Function navigator:** filters discovered function ranges and moves the shared address selection to the chosen entry point.
- **Canvas:** scroll provides pan, magnification provides zoom, and layout is deterministic so a reopened analysis does not reshuffle gratuitously.
- **Minimap:** gives whole-graph orientation without replacing the evidence-bearing primary canvas.
- **Block context:** Code, IR, and Data Flow are synchronized to the selected block. IR/Data Flow are labeled as modeled analysis, not decompiled source.
- **Dynamic correlation:** when a debugger PC is available, its containing block receives a distinct execution highlight. Static evidence and live observation remain distinguishable.
- **Dockability:** function and semantic panes can be independently hidden so the same surface supports orientation, graph tracing, and focused code reading.

The graph selection writes through to the same address/evidence context as disassembly. The analyst should never have to manually copy an address merely to pivot between two first-party HindSight surfaces.


## Binary Workspace additions (0.3.3)

The Binary Workspace is the default code-oriented desk. It reduces context switching between the common tasks that previously required separate report pages.

- **Function browser:** discovered function ranges remain visible, searchable, and directly selectable.
- **Code + semantics:** the center can show code, conservative lifted statements, or both in a resizable vertical split. Raw worker IR remains one click away.
- **Reference context:** direct branch/call evidence is deduplicated and presented with clickable source and target addresses.
- **Instruction data:** the selected ARM64 word is shown as raw little-endian bytes plus signed/unsigned interpretations and any available annotation/data-flow evidence.
- **Follow PC:** when native debugger state is available, the active PC can synchronize the function and instruction selection without erasing the static evidence distinction.
- **No fake decompiler:** Lifted statements are a readability layer over bounded IR. Opaque semantics remain explicit barriers and must not be presented as recovered source code.

The dedicated Disassembly, Annotated Code, IR, Xrefs, and CFG workspaces remain available for deep inspection. Binary Workspace is the integrated desk, not a replacement for evidence-specific surfaces.

## Runtime Correlation additions (0.3.4)

Runtime Correlation is a dedicated dynamic-analysis desk, not a replacement for raw debugger views. The left timeline contains bounded observed stop/snapshot evidence; the center correlates static and live code; the right pane shows register deltas, breakpoints, memory observations, and the loaded-image evidence used for address translation. Runtime-to-static addresses are displayed only when a unique module match and observed dyld slide support the mapping. Execution heat means "number of captured stop observations at this address," not instruction-count profiling.


## Trace Explorer additions (0.3.5)

Trace Explorer is a temporal evidence surface, not a profiler or an instruction-trace simulator. The left timeline contains bounded debugger observations. The center explains the selected transition and shows synchronized static code only when address correlation is established. The right evidence band summarizes register changes and explicit memory reads near the observation. Call/branch/return labels require supporting static source-instruction evidence; arbitrary endpoint changes are labeled **observed-transfer**, and ambiguous module correlation remains **unresolved**.


## Evidence Graph additions (0.3.6)

Evidence Graph is the provenance-first investigation surface. It may visually connect static analysis, debugger observations, static/runtime correlations, explicit memory reads, and analyst notes, but it must not collapse their epistemic status. Nodes and edges display one of **observed**, **derived**, **correlated**, **asserted**, or **unresolved**.

The graph is deliberately bounded and deterministic. Search and provenance filters reduce clutter; selecting an address-bearing node synchronizes the shared investigation address, and the inspector can pivot to Binary Workspace, CFG, Runtime Correlation, Trace Explorer, or Notes. Temporal proximity (for example, a memory read captured near a debugger stop) must be described as proximity rather than causation. Analyst notes remain assertions, never machine-derived facts.


## Investigation Ledger additions (0.3.7)

The Ledger is the durable case layer, not another analysis report. Its timeline, hypotheses, bookmarks, and snapshots use the same provenance language as Evidence Graph. The workspace is logically three-region: case navigation/provenance, the active ledger surface, and an immutable-entry inspector/context pivot. Beta.11 composes those regions side-by-side only when width permits; medium and compact layouts move auxiliary regions into a switchable side/bottom dock rather than squeezing the ledger canvas. Restoring a snapshot restores workbench context only; live target state is never implied to rewind.


## Responsive Workbench re-architecture (0.4.0-beta.11)

The shell now treats width as a composition constraint instead of a scaling request. `VGAdaptiveThreePane` and `VGAdaptiveTwoPane` are the canonical primitives for dense first-party workspaces. Wide layouts may use simultaneous navigator/canvas/inspector regions; medium layouts protect the primary canvas and multiplex auxiliary regions; compact layouts move auxiliaries below the canvas. Evidence-bearing content keeps readable intrinsic sizes and may scroll where appropriate.

The global inspector follows the same rule: trailing on wide detail canvases, bottom-docked below the responsive threshold. Workspace headers use semantic rows rather than one ever-growing toolbar, and summary bands may overflow independently without forcing the canvas wider. The supported minimum workbench window is 900 × 680; release validation must exercise multiple widths instead of validating only the default 1480 × 920 window.
