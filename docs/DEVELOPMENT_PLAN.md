# HindSight Development Plan

## Product direction

HindSight should help a reverse engineer move from an immutable artifact to a traceable, analyst-owned conclusion. The product must make the next investigation step obvious while keeping verified observations, inferred structure, runtime facts, and analyst judgement distinct.

## Current baseline

The app already has a broad macOS workbench covering project/artifact management, bounded multi-architecture analysis, evidence triage, binary diffing, debugger/runtime surfaces, firmware and emulation tools, scripting/plugins, investigation continuity, collections, review, follow-ups, exports, Quick Start, and evidence annotations.

## Priority roadmap

### Phase 1 — Investigation usability

- [x] Replace visible legacy branding with HindSight.
- [x] Add Quick Start and the Import → Analyze → Triage → Document → Export path.
- [x] Add evidence annotations with notes, tags, confidence, persistence, handoff export, and Annotated Triage mode.
- [x] Add saved searches and reusable triage filters.
- [x] Make annotations available from every evidence card without requiring the inspector.
- [x] Add an analyst-facing “annotated handoff” summary to the main Overview.

Acceptance: a new user can import an artifact, run analysis, find a result, annotate it, retrieve it later, and export a handoff without searching through advanced workspaces.

### Phase 2 — Analysis quality and depth

- [x] Add bounded compact-unwind metadata ingestion; DWARF/dSYM sections remain explicitly inventory-only.
- [ ] Improve type and variable recovery.
- [x] Add bounded AArch64 call-argument/return-register provenance; source-level types and callee prototypes remain explicit unknowns.
- [x] Validate bounded executable-target pointer runs for jump-table triage; dispatch use and index bounds remain explicit unknowns.
- [x] Add metadata-guided automatic ARM/Thumb control-flow transitions; register-indirect mode changes remain explicit unresolved evidence.
- [x] Expand bounded UTF-16 Unicode string decoding with provenance-preserving output.
- [x] Add bounded IMG4/IM4P container inspection with compression-marker inventory; plaintext analysis remains explicit.
- [ ] Improve non-ARM64 semantic lifting and pseudocode quality.

Acceptance: every new inference is provenance-labelled, bounded, and covered by synthetic and real-input regression tests.

### Phase 3 — Runtime investigation

- [ ] Improve static-to-runtime pivots and breakpoint context.
- [ ] Add persistent trace capture/replay and runtime snapshot browsing.
- [ ] Add clearer host capability, permission, and entitlement diagnostics.
- [ ] Expand Intel and Apple Silicon qualification coverage.

Acceptance: an analyst can connect a static finding to a reproducible runtime observation and include both in a handoff.

### Phase 4 — Product readiness

- [ ] Add a bundled sample project or synthetic tutorial artifact.
- [ ] Add workspace-level help and unsupported-capability explanations.
- [ ] Add recent-project management and preferences.
- [ ] Add UI regression coverage for compact, medium, and wide layouts.
- [ ] Complete signing, notarization, release-host, memory-pressure, and crash-diagnostics gates.

Acceptance: a clean install is understandable, testable, supportable, and distributable on supported macOS hosts.

## Work loop

For every loop:

1. Read this plan and inspect the current repository state.
2. Select the highest-priority incomplete item that can be completed safely in the current workload.
3. State the intended outcome and implementation boundary.
4. Implement the smallest complete slice.
5. Run focused tests, parser/type checks, and `git diff --check`.
6. Update this plan when an item is completed or its scope changes.
7. Commit only the completed slice and push `main`.
8. Confirm the remote and working tree state, then begin the next loop.

## Today's workload

The Phase 1 usability slices, bounded IMG4/IM4P container inventory, compact-unwind record ingestion, executable-target pointer-run validation, metadata-guided mixed ARM/Thumb decoding, and bounded AArch64 call-register provenance are complete. Next loop: prioritize validated type/variable metadata recovery or the remaining non-ARM64 semantic lifting slice.
