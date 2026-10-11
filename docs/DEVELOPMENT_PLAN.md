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
- [x] Add bounded ARM64 stack-slot variable-location candidates from decoded stack-relative loads/stores; source names, lifetimes, layouts, and types remain unresolved.
- [x] Label validated Swift type-descriptor names with relative-pointer provenance and confidence.
- [x] Surface bounded Swift field-variable counts with explicit unresolved layout/declaration boundaries.
- [x] Add bounded AArch64 call-argument/return-register provenance; source-level types and callee prototypes remain explicit unknowns.
- [x] Validate bounded executable-target pointer runs for jump-table triage; dispatch use and index bounds remain explicit unknowns.
- [x] Add metadata-guided automatic ARM/Thumb control-flow transitions; register-indirect mode changes remain explicit unresolved evidence.
- [x] Expand bounded UTF-16 Unicode string decoding with provenance-preserving output.
- [x] Add bounded IMG4/IM4P container inspection with compression-marker inventory; plaintext analysis remains explicit.
- [x] Improve bounded structural pseudocode with operand-preserving call, value-flow, memory-write, compare, and arithmetic sketches; full type/side-effect lifting remains open.
- [x] Add bounded non-ARM64 opcode-semantic labels to annotated disassembly; full lifting/decompilation remains open.
- [x] Distinguish bounded direct versus indirect calls and unconditional versus conditional/indirect control flow in non-ARM64 labels; operand recovery and full lifting remain open.

Acceptance: every new inference is provenance-labelled, bounded, and covered by synthetic and real-input regression tests.

### Phase 3 — Runtime investigation

- [x] Add explicit stop-event and stop-reason context to static-to-runtime pivots; richer breakpoint-condition context remains open.
- [x] Distinguish module/slide address correlation from validated static-function correlation in runtime explanations.
- [x] Add bounded `.vgcase` runtime trace capture/replay and restore the saved timeline into Trace Explorer without debugger attachment.
- [x] Add clearer host capability, permission, and entitlement diagnostics, including explicit architecture gates and honest deferred runtime-probe status.
- [x] Expand Intel and Apple Silicon qualification coverage for host gates and non-ARM64 semantic classification; full hardware release qualification remains open.

Acceptance: an analyst can connect a static finding to a reproducible runtime observation and include both in a handoff.

### Phase 4 — Product readiness

- [x] Surface the bundled synthetic ARM64 Mach-O tutorial fixture in Quick Start with a reproducible analyst walkthrough.
- [x] Add workspace-level help and unsupported-capability explanations.
- [x] Add bounded recent-project management with stale-path cleanup and persisted startup/layout preferences.
- [x] Add explicit compact, medium, and wide adaptive-layout modes with source-level regression coverage for the shared pane primitive.
- [ ] Complete signing, notarization, release-host, memory-pressure, and crash-diagnostics gates. Source-side identity, crash parser, startup-pressure, universal-slice, gate-coverage, strict Clippy, ordinary Rust tests, and the elevated universal Debug/Release candidate gate pass on this arm64 host; Developer ID/notary credentials and the manual real-target, memory-pressure, and crash-diagnostics qualification matrix remain required.

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

The Phase 1 usability slices, bundled synthetic tutorial fixture and walkthrough, bounded ARM64 stack-slot variable-location candidates with operand-derived value-shape hints, bounded IMG4/IM4P container inventory, compact-unwind record ingestion, executable-target pointer-run validation, metadata-guided mixed ARM/Thumb decoding, bounded AArch64 call-register provenance, Swift type-descriptor provenance labels, Swift field-variable boundaries, bounded non-ARM64 semantic labels with direct/indirect and conditional-control distinctions, bounded structural pseudocode sketches, explicit runtime correlation strength, explicit runtime stop-event/reason context, `.vgcase` runtime trace replay, host capability/permission/entitlement diagnostics, workspace-level help/unsupported-capability explanations, bounded recent-project management, persisted startup/layout preferences, architecture-focused qualification tests, startup-pressure diagnostics, shared compact/medium/wide layout coverage, cross-file Swift access-control validation, current bridge smoke expectations, optional release-argument handling, elevated universal Debug/Release candidate qualification, and non-secret release preflight reporting are complete. The remaining workload is the credentialed notarization path and the manual real-target, memory-pressure, and crash-diagnostics matrix.
