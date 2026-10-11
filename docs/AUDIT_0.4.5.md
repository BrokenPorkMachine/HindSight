# HindSight 0.4.5 Release Audit

## Release identity

- Product/service/worker version: **0.4.5**
- Apple bundle build: **4105**
- Release channel: **stable**
- Protocol ABI: **1**
- Analysis report schema: **66 fields (unchanged)**
- Analysis engine identity: **arm64-analysis-v11-schema66 (unchanged)**
- Capability schema: **210 fields**

## Scope

0.4.5 extends the 0.4.4 investigation-continuity layer with named evidence collections and explicit evidence-resume behavior. The release does not add speculative analysis semantics and does not alter the worker report schema, XPC ABI, project SQLite schema, or analysis-engine identity.

### Investigation collections

- Adds `VGInvestigationCollection` to project-local continuity metadata.
- Up to **32 collections per artifact**.
- Up to **64 findings per collection**.
- Collection membership is backed by stable pin identities rather than copied report text.
- Adding an unpinned finding to a collection first preserves it as a pin.
- Unpinning evidence removes stale membership from every collection.
- Stale pin references in a continuity file are sanitized on load rather than causing an otherwise-valid collection to be discarded.
- Collection names are normalized and bounded to 96 characters before duplicate-name comparison.

Collections are organization/navigation metadata only. They do not imply severity, exploitability, causality, semantic importance, or runtime reachability.

### Evidence resume

- Adds **Resume Latest Evidence** to the Workspace menu and Analysis Triage.
- Adds per-visit Resume controls to Recent evidence.
- Replay restores the recorded Source / Disassembly / CFG / Selected Symbol destination.
- Replay deliberately uses `recordVisit: false`, so navigating history does not manufacture a duplicate history entry.

### Continuity migration and bounds

The existing `.vectorglass/analysis-continuity-v1.json` storage path is retained. The document schema advances to **2** with a decoder that accepts schema 1 and treats absent collections as empty. The next save republishes schema 2.

Existing continuity protections remain:

- 1 MiB total metadata budget.
- 128 pins.
- 96 recent pivots.
- regular-file requirement.
- symlink rejection for metadata directory/file.
- atomic publication.
- analyst note bodies excluded.

## Compatibility

0.4.5 keeps `arm64-analysis-v11-schema66`, protocol ABI 1, and the exact 66-field `AnalysisReport`. Cache envelope schema 2 remains in use, so valid 0.4.2–0.4.4 caches remain eligible for reuse when captured-source and engine identities validate. New cache envelopes carry producer version 0.4.5.

## Validation completed in this environment

The frozen source tree passed:

- `scripts/validate-source.py` source/topology/protocol validation.
- **116 pytest tests passed**.
- **6 optional LLVM-oracle tests skipped** because the optional oracle is unavailable.
- **63 subtests passed**.
- New 0.4.5 investigation-collection/resume contracts passed.
- All shipped Swift sources under App / Shared / PlatformServices parse with `swiftc -parse`.
- Exact **66-field** Swift/Rust analysis protocol contract passed.
- Capability schema regenerated and matched the bridge at **210 fields**.
- Deterministic Xcode project/workspace regeneration passed.
- Hypervisor.framework Apple-Silicon API shim compile contract passed.
- Cargo lockfile structural preflight passed.
- VectorEmu Rust 1.90 MSRV source preflight passed.
- Shell syntax checks passed.
- Existing UI-density, analysis-scalability, staged-analysis, cache, framing, paging, native Mach-O, project-SQL, graph, binary-workspace, runtime-correlation, evidence-graph, and investigation-ledger regression suites remain green.

## Release-host gates not executed here

This container does not provide Cargo and is not macOS. `scripts/test-portable.sh` therefore correctly fails fast at its Cargo prerequisite rather than pretending the Rust build ran.

Before public distribution, run the release-host matrix on the target Apple-Silicon Mac:

- pinned Cargo build/tests/clippy and release compilation;
- Xcode Debug and Release app/service linking;
- real Coordinator/XPC/embedded-worker smoke tests;
- real firmware-scale analysis regression;
- Hypervisor / debugger / VectorEmu runtime tests;
- visual qualification from the supported compact workbench size through full screen;
- 8 GiB memory-pressure qualification;
- Developer ID signing, notarization, and stapling.

## Release conclusion

Within the portable/source-verifiable surface available here, 0.4.5 is internally consistent and preserves the stable analysis ABI while adding bounded, project-local investigation organization and history replay. Public macOS distribution remains conditional on the Apple-Silicon release-host gates above.
