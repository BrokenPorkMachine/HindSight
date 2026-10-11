# HindSight 0.4.7 Release Audit

## Release identity

- Product/service/worker version: **0.4.7**
- Apple bundle build: **4107**
- Release channel: **stable**
- Protocol ABI: **1**
- Analysis report: exact **66-field** contract
- Analysis engine identity: **arm64-analysis-v11-schema66**
- Analysis cache envelope: **schema 2**
- Capability contract: **218 fields**
- Analysis continuity document: **schema 3** (reads schema 1/2/3)
- Project SQLite schema: unchanged

## Scope

0.4.7 turns investigation collections into explicit reviewable working sets. It does not change the Rust worker evidence schema or analysis semantics. The release adds exact-identity review checkpoints, bounded evidence-drift summaries, and pairwise collection comparison while preserving the responsive workbench and the 0.4.2+ cache line.

## Collection review changes

### Explicit review baselines

A collection can be marked reviewed. The project-local baseline records the collection ID, artifact record ID, captured-source SHA-256 when available, review timestamp, exact collection membership, and—when analysis is loaded—the subset present in the bounded current validated analysis corpus. Baselines are bounded to 256 documents globally and remain subject to the existing 64-findings-per-collection limit.

### Evidence drift

HindSight compares current collection membership against the last explicit review baseline and reports exact-identity transitions: findings added, findings removed, common findings that became current, and common findings that became preserved-only. Presence drift is computed only across findings that remain members of the collection, avoiding double-counting a removed finding as both removed and preserved.

The UI explicitly preserves the interpretability boundary: drift is continuity/provenance evidence, not vulnerability, severity, correctness, exploitability, runtime reachability, or semantic-importance scoring.

### Pairwise collection comparison

Analysis Triage adds a responsive **Review** mode. Two collections can be compared by stable preserved pin identity, producing bounded navigable sets for shared evidence, evidence only in collection A, and evidence only in collection B. Aggregate counts remain exact for the bounded collection contents while each rendered evidence section is capped at 24 cards to keep the review surface responsive.

### Responsive triage selector

The six Analysis Triage modes now use `ViewThatFits`: a segmented selector is used when sufficient horizontal space exists and a menu picker is used at narrower widths. This avoids reintroducing the control-density failure addressed in 0.4.2.

## Continuity migration and storage

The continuity document advances from schema 2 to schema 3 and continues to decode schema 1 and schema 2 state in place. Review baselines are sanitized against existing collections and bounded on load. The storage filename remains `.vectorglass/analysis-continuity-v1.json`; the 1 MiB budget, regular-file/symlink checks, and atomic writes remain unchanged.

Deleting a collection also removes its review baseline, while deleting/unpinning evidence continues to clean collection membership safely.

## Compatibility

The analysis engine identity, protocol ABI, exact 66-field `AnalysisReport`, project SQLite schema, and cache envelope schema remain unchanged. Compatible 0.4.2–0.4.6 schema-2 analysis caches remain eligible for reuse when source and engine identity checks succeed. New cache writes carry producer version 0.4.7.

## Validation performed in this environment

- `scripts/validate-source.py`: passed after deterministic Xcode regeneration.
- Python/portable pytest suite: **126 passed, 6 optional LLVM-oracle tests skipped, 63 subtests passed**.
- Native/source contract runner invoked by validation: **107 tests OK, 6 optional LLVM-oracle skips**.
- Focused 0.4.7 collection/review regression suite: **15 passed**.
- All **40** shipped Swift sources under App/Shared/PlatformServices parse with the available Swift compiler.
- Deterministic Xcode project/workspace/scheme regeneration: passed.
- Capability schema regeneration/check: passed at **218 fields**.
- Cargo lockfile structural preflight: passed.
- VectorEmu Rust 1.90 MSRV source preflight: passed.
- Portable Apple-Silicon Hypervisor API shim compile contract: passed.
- Shell syntax validation: passed.
- The aggregate portable build correctly fails fast because Cargo is not installed in this container.

## Release-host gates not claimed here

The following require the Apple-Silicon/macOS release host and are intentionally not represented as completed by this Linux environment:

1. Real Cargo build/test/clippy of the Rust workspace.
2. Xcode Debug and Release linking of the app and services.
3. Real Coordinator/XPC worker execution.
4. Real Hypervisor/Debugger/VectorEmu execution.
5. Visual qualification across the supported window-size range.
6. 8 GB memory-pressure and firmware-scale regression runs.
7. Developer ID signing, notarization, and stapling.

## Conclusion

Within the portable/source-verifiable surface available here, 0.4.7 is internally consistent and preserves the stable analysis/cache line while materially improving investigation review, collection comparison, and evidence-drift visibility. Public macOS distribution remains conditional on the release-host gates above.
