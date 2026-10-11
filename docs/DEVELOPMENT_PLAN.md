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

- [ ] Add DWARF/dSYM and unwind metadata ingestion where available.
- [ ] Improve type, variable, and call-signature recovery.
- [ ] Add validated indirect-dispatch and jump-table reconstruction.
- [ ] Add automatic mixed ARM/Thumb control-flow transitions.
- [x] Expand bounded UTF-16 Unicode string decoding with provenance-preserving output.
- [ ] Add IMG4/IM4P container inspection and supported decompression paths.
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

The Phase 1 usability slices are complete through saved searches, inline annotation editing, and the Overview handoff summary. Next loop: begin Phase 2 with a bounded analysis-depth improvement, starting with the most useful metadata path available in the current worker.
