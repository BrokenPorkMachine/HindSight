# HindSight 0.4.4 — Correctness, Completeness, and Release Audit

## Release identity

- Product/service/worker version: **0.4.4**
- Apple bundle build: **4104**
- Release channel: **stable**
- Coordinator/XPC protocol ABI: **1**
- Static analysis report schema: **66 exact fields** (unchanged)
- Analysis cache envelope: **schema 2** (unchanged)
- Analysis engine identity: **`arm64-analysis-v11-schema66`** (unchanged)
- Capability schema: **207 fields**

0.4.4 is intentionally an application/workflow release. It does not change the ARM64 evidence engine or project SQLite schema, so a valid 0.4.2/0.4.3 schema-2 analysis cache remains eligible for reuse when the captured SHA-256 and analysis-engine identity validate.

## What changed

### Project-local analysis continuity

0.4.4 adds `App/AnalysisContinuity.swift` and stores non-authoritative analyst-navigation metadata in:

`<project>/.vectorglass/analysis-continuity-v1.json`

The state contains bounded pins, bounded recent evidence pivots, per-artifact analysis-channel snapshots, and the most recent structural channel comparison. It does **not** copy analyst note contents. Captured objects and validated `AnalysisReport` data remain the evidence authority.

Continuity storage is capped at **1 MiB**, pins at **128**, and recent pivots at **96**. Persisted evidence snippets and symbols are truncated to bounded display/navigation sizes. Project roots and `.vectorglass` directories must be real directories; the state file, when present, must be a regular non-symlink file. JSON is published atomically. ISO-8601 date decoding explicitly matches the encoder.

### Analysis Triage continuity modes

Analysis Triage now provides four modes:

- **Focus** — the existing source-prioritized, bounded navigation queue.
- **Pinned** — analyst-selected evidence preserved with the project.
- **Recent** — successful Source / Code / CFG / Symbol pivots, de-duplicated and bounded.
- **Changes** — the latest structural comparison against the previous complete analysis snapshot for the same artifact.

Pinned, Recent, and Changes are available immediately after reopening a project even if the current `AnalysisReport` has not yet been loaded into the UI. The live Focus queue continues to require a loaded/reused report.

### Analysis change ledger

For each analysis channel, HindSight records a bounded line count plus a deterministic 64-bit FNV-1a content fingerprint. On the next complete analysis it reports which channels changed and their line-count transition.

This fingerprint is **not cryptographic** and is never used for source integrity, cache admission, trust, or security decisions. Source identity remains the captured SHA-256. The change ledger is a continuity signal only; it is not semantic diffing, severity, exploitability, reachability, or importance scoring.

### Relaunch context schema 2

The launch-session record advances to schema 2 and restores:

- project path;
- selected artifact;
- workspace and layout preset;
- address text;
- CFG focus;
- analysis search query;
- selected symbol.

Schema-1 launch records remain accepted and migrate naturally on the next save. Each newly persisted navigation field has a strict byte/character bound and the overall launch record retains its 32 KiB ceiling.

## Correctness and compatibility findings

The 0.4.3 analysis-navigation model remains intact. 0.4.4 does not add fields to `AnalysisReport`, change framing, broaden evidence claims, or invalidate the patch-compatible project analysis cache.

During the implementation audit, continuity decoding was hardened to use the same ISO-8601 strategy as encoding, and state-file admission was tightened from symlink-only checking to **regular-file + non-symlink** validation. Restored pins use their stored stable identity when toggled so a bounded/truncated display snippet can still be unpinned correctly.

The Xcode generator itself was updated to include `App/AnalysisContinuity.swift`; deterministic project regeneration therefore cannot silently drop the new source from the application target.

## Validation performed in this environment

The frozen source tree passed:

- `scripts/validate-source.py` source/topology/metadata/contracts;
- **111 pytest tests passed**;
- **6 optional LLVM-oracle tests skipped** because the oracle is not installed;
- **63 parameterized/subtests passed** within the suite;
- exact **66-field** Swift/Rust `AnalysisReport` protocol parity;
- **207-field** capability-schema parity;
- Swift syntax parsing for every shipped App, Shared, and CoordinatorService Swift source;
- deterministic Xcode project/workspace regeneration;
- Cargo lockfile structural preflight;
- VectorEmu Rust **1.90** MSRV source-admission preflight;
- portable Apple-Silicon Hypervisor API shim compilation;
- shell syntax validation across shipped shell scripts;
- the 0.4.4 analysis-continuity regression contract;
- the existing responsive/UI-density, scalability, analysis-quality, analysis-navigation, paging, framing, native Mach-O, project-SQL, and cache regression suites.

`scripts/test-portable.sh` was also invoked to check its release prerequisite behavior. It correctly fails immediately in this container because **Cargo is not installed**. That is an environment limitation, not reported as a passing Rust build.

## Release-host gates not executed here

This container cannot truthfully certify the macOS distribution matrix. Before public distribution, run the stable release-host gate on the Apple Silicon target and verify:

- Cargo build/test/clippy for all Rust workspace crates;
- Xcode Debug and Release app + XPC service linking;
- real Coordinator/worker execution and 0.4.3 cache reuse;
- 0.4.4 project-local pin/recent persistence across quit/relaunch;
- schema-1 launch-session migration to schema 2;
- continuity fail-closed behavior with a symlink/special state file;
- analysis Changes reporting on cached/fresh repeated analysis;
- the existing real Hypervisor/Debugger/VectorEmu matrix;
- visual qualification from the 900×680 minimum through full-screen layouts;
- the 8 GiB memory-pressure workload;
- Developer ID signing, hardened runtime verification, notarization, and stapling.

## Packaging integrity

The final release tree contains **287 files total**: **286 payload files** plus `SHA256SUMS`. The manifest covers every payload file, including this audit. Packaging verification must be repeated after fresh extraction before the outer ZIP checksum is published.

## Release assessment

Within the portable/source validation available here, **0.4.4 is internally consistent and ready for Apple Silicon release-host qualification**. No protocol or project-schema migration is required. The main remaining uncertainty is platform qualification, not a known source-contract failure.
