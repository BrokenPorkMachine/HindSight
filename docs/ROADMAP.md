# HindSight Roadmap

## 0.1 — Genesis (foundation complete)

- Independent HindSight product identity and macOS workbench.
- Existing Mach-O/Apple-runtime semantic analysis retained.
- Native debugger, scripting/extensions, and Hypervisor research surfaces retained.
- VectorEmu extracted from reusable iPhemeral execution components.
- Machine Lab workflow with registers, code, trace, regions, breakpoints, and lifecycle controls.
- Evidence doctrine and Mentor Mode foundation.

**Exit gate:** clean Apple Silicon Xcode build; Cargo tests for both Rust workspaces; launch/attach/step/reset smoke test; memory-pressure test on an 8 GB M-series Mac; package/sign/notarize test.

## 0.2 — Unified Target Model (historical alpha milestone)

- [x] Common target/session/address-space identity across static, native, VectorEmu, and hypervisor backends.
- [x] Capability discovery rather than backend-name conditionals at the target layer.
- [x] One address pivot resolves file offset, preferred VM address, runtime slide, and declared emulated/guest mappings where a mapping is provable.
- [x] Exportable session header with target/engine/model/source fingerprints.
- [ ] Promote engine/model fingerprints to cryptographic build hashes and add trace identity after the evidence graph lands.

## 0.3 — Evidence Graph + Investigation Time

- [x] First-class provenance graph over static/runtime/analyst evidence.
- [x] Pin current context into bookmarks and first-class hypotheses.
- [x] Persistent investigation ledger with replayable bounded timeline.
- [x] Session snapshots that restore workbench context without claiming to rewind live targets.
- [x] Trace-to-static correlation and before/after register evidence.
- [x] Portable `.vgcase` plus Markdown/PDF/JSON investigation export.
- [ ] Deterministic backend state snapshots and memory/map/control-flow diff for engines that can prove replay identity.

## 0.4 — Query & Recovery

- Structured semantic query language across symbols, xrefs, types, runtime records, traces, and evidence.
- Better function/type recovery and confidence reporting.
- Searchable trace index with bounded storage.
- Extension API v3 around target/evidence abstractions.
- Headless CLI for reproducible analysis pipelines.
- Decompiler and reverse-engineering capability track: [feature gap checklist](DECOMPILER_GAP_ROADMAP.md).

## 0.5 — Broader Targets

- ELF ingestion and Linux process workflows.
- PE/COFF ingestion and Windows-oriented static projects.
- Architecture abstraction beyond ARM64 where it adds real user demand.
- Import/export of common reversing artifacts rather than forced ecosystem lock-in.

## 0.6 — Team Research

- Shareable investigation bundles with content hashing.
- Comment/review state, findings, evidence links, and reproducible experiment recipes.
- Extension policy, signatures, allowlists, and organization settings.
- Optional on-prem execution node.

## 1.0 — Commercial Release

- Stable project schema and migrations.
- Stable SDK and target protocol.
- Crash recovery and resumable long jobs.
- Performance budgets with regression tests.
- Signed/notarized distribution and updater.
- Licensing tiers without crippling offline/local analysis.
- Documentation, guided labs, onboarding datasets that are legally redistributable, and enterprise deployment material.

## North-star engineering rule

Every release must either reduce the time from **question → evidence-backed answer**, increase reproducibility, or make advanced analysis understandable without making it less exact. Features that do none of those do not belong on the critical path.
