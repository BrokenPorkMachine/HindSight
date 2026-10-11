# HindSight 0.4.3 Release Audit

## Release identity

- Product: **HindSight 0.4.3**
- Release channel: **stable**
- Apple marketing version: **0.4.3**
- Apple bundle build: **4103**
- Coordinator protocol ABI: **1**
- Analysis report contract: **66 exact fields (unchanged)**
- Analysis engine identity: **arm64-analysis-v11-schema66 (unchanged)**
- Project analysis-cache envelope: **schema 2 (unchanged)**
- Capability schema: **203 fields**

## 0.4.3 scope

0.4.3 is an analysis-consumption and navigation release. It does not add new unproven worker heuristics or expand the analysis transport. The goal is to reduce the manual copy/search/jump loop that had accumulated as HindSight gained more evidence surfaces.

### Analysis Triage

A new **Analysis Triage** workspace assembles a bounded focus queue from validated report channels such as Analysis Insights, Analysis Quality, Function Intelligence, Function Neighborhoods, Symbol Callers, Data References, Indirect Calls, Tail-Call Candidates, and linkage evidence.

The queue is source-prioritized for navigation efficiency only. It is explicitly not a vulnerability, severity, exploitability, semantic-importance, or runtime-reachability score. Every row retains the original evidence text and source workspace.

### Structured Search & Navigate

Navigator now presents source-labelled evidence cards instead of an anonymous flattened text result. Where the evidence itself supplies the required identity, a result can:

- return to the owning source workspace;
- jump to Disassembly using the parsed virtual address;
- focus that address in CFG; or
- open Selected Symbol using an explicit symbol identity.

Destinations are not invented for text-only evidence.

### Related Evidence

The contextual inspector now correlates the selected evidence row across analysis channels by exact parsed virtual-address equality or explicit selected-symbol identity. The resulting cards remain source-labelled and independently navigable. Shared identity is treated as a navigation correlation only, not a new semantic edge.

### Compact report navigation

Existing high-volume clickable evidence surfaces retain their compact rows. Address-bearing rows gain a context menu for Disassembly/CFG routing instead of adding permanent buttons to thousands of lines.

## Bounds and safety properties

The new presentation layer is bounded independently of the worker transport:

- maximum admitted evidence-line size: **16 KiB**;
- structured search result limit: **500** (defensive hard ceiling 1000);
- Analysis Triage limit: **180** (defensive hard ceiling 300);
- related-evidence limit: **120** (defensive hard ceiling 300).

The report remains immutable/atomic after Coordinator validation. 0.4.3 does not publish partial analysis data, change the worker framing channel, or mutate evidence while navigating it.

## Cache continuity

Because neither the 66-field `AnalysisReport` nor `arm64-analysis-v11-schema66` changes, schema-2 caches produced by 0.4.2 remain eligible for reuse when the captured source SHA-256, file metadata, and engine identity validate. The producer identity is updated to 0.4.3 for newly written cache envelopes.

Schema-1 compatibility remains explicitly limited to the validated 0.4.1 legacy format.

## UI integration

The new navigation layer is part of the 0.4.2 responsive workbench contract:

- Analysis Triage is registered in the workspace catalog and sidebar;
- the layout preset system can open Analysis Triage directly;
- the Workspace menu exposes Analysis Triage;
- result cards use the existing wrapping `VGFlowLayout` controls;
- no new fixed-width multi-column requirement was introduced;
- the new source file is included in deterministic Xcode topology generation.

## Validation performed in this environment

The frozen source tree passed the available portable gates:

- `PYTHONPATH=python python3 -m pytest -q python/tests`: **106 passed, 6 skipped**; the skipped cases are the optional LLVM-oracle checks;
- all **5 new 0.4.3 analysis-navigation regression tests** passed;
- `scripts/validate-source.py` passed, including topology, release identity, capability parity, native fixtures, project SQL, analysis protocol/status contracts, UI-density guards, and portable Swift contract execution;
- exact Swift/Rust analysis protocol remains **66 fields**;
- all shipped Swift files parse with the available Swift compiler;
- deterministic Xcode project regeneration includes `App/AnalysisNavigation.swift`;
- Hypervisor Apple-Silicon portable API-stub compile contract passed;
- Cargo lockfile structural preflight passed;
- VectorEmu Rust 1.90 MSRV source preflight passed;
- shell syntax validation passed;
- generated capability schema matches the bridge and contains **203 fields**.

## Gates not executable here

The full `scripts/test-portable.sh` release aggregate intentionally stops because this Linux container has no Cargo installation. Therefore this audit does **not** claim:

- real Cargo test/clippy/release linking;
- Xcode Debug/Release typecheck or application linking;
- real macOS XPC execution;
- Apple Silicon Hypervisor runtime behavior;
- the 8 GiB memory-pressure matrix;
- visual qualification at all supported window sizes;
- Developer ID signing, notarization, stapling, or Gatekeeper assessment.

Those remain mandatory release-host gates in `docs/RELEASE_READINESS.md` and `scripts/test-release-macos.sh`.

## Assessment

Within the portable/source-verifiable boundary, 0.4.3 preserves the stable 0.4.2 analysis and cache contracts while making the analysis corpus substantially easier to traverse. The release introduces no new worker evidence claims: its new correlations and triage ordering are presentation/navigation constructs with explicit bounds and provenance.
