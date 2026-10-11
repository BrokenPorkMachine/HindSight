# HindSight 0.4.8 Release Audit

## Release identity

- Product/service/worker version: **0.4.8**
- Apple bundle build: **4108**
- Release channel: **stable**
- Protocol ABI: **1**
- Analysis report: exact **66-field** contract
- Analysis engine identity: **arm64-analysis-v11-schema66**
- Analysis cache envelope: **schema 2**
- Capability contract: **222 fields**
- Analysis continuity document: **schema 4** (reads schema 1/2/3/4)
- Project SQLite schema: unchanged

## Scope

0.4.8 turns the 0.4.7 collection-review surface into a durable review workflow. It adds bounded analyst-authored journal entries, explicit workflow state, and a local Markdown handoff while preserving the exact-identity/provenance boundary used by collection health, drift, and comparison. The Rust worker evidence schema and ARM64 analysis engine are unchanged.

## Review journal changes

### Explicit workflow state

Each investigation collection can carry review journal entries with one workflow state: **Open**, **In Review**, **Follow Up**, or **Reviewed**. These labels are analyst-authored organizational state only. They do not claim vulnerability, severity, exploitability, correctness, runtime reachability, semantic importance, or evidence validity.

### Bounded journal entries

Every journal entry records the collection ID, artifact record ID, timestamp, optional UTF-8 note, and the exact preserved pin identities that belonged to the collection when the entry was written. Notes are bounded to **2 KiB UTF-8**, journal storage is bounded to **256 entries globally** and **64 entries per collection**, and the existing **1 MiB project-local continuity budget** remains authoritative.

The Review UI renders only the newest 12 entries for a collection; older entries remain in continuity state within the configured limits. Deleting a collection removes its associated review journal and review baseline while preserving independently pinned evidence.

### Single-collection review workflow

The Review surface no longer requires two collections merely to be useful. One collection is enough to inspect health/drift, record review state and notes, refresh the exact review baseline, and generate a handoff. Pairwise exact-identity comparison remains available when two or more collections exist.

### Investigation handoff

HindSight can generate a bounded Markdown handoff and copy it to the clipboard on explicit analyst action. The handoff includes collection and artifact identity, captured source SHA-256 when available, workflow state, collection health, exact review drift, preserved evidence, and the newest 16 review entries.

Nothing is transmitted externally. The handoff footer explicitly states that health, drift, workflow state, and preserved membership are continuity/provenance signals rather than semantic or risk scoring.

## Continuity migration and storage

The continuity document advances from schema 3 to schema 4 and continues to decode schemas 1, 2, and 3 in place. Review entries are sanitized against existing collections, validated for bounded text/identity sizes, limited on load, and retained in the same `.vectorglass/analysis-continuity-v1.json` file. Existing regular-file/symlink checks and atomic writes are unchanged.

## Compatibility

The analysis engine identity, protocol ABI, exact 66-field `AnalysisReport`, project SQLite schema, and cache envelope schema remain unchanged. Compatible 0.4.2–0.4.7 schema-2 analysis caches remain eligible for reuse when existing source/engine identity checks succeed. New cache writes carry producer version 0.4.8.

## Validation performed in this environment

- `scripts/validate-source.py`: passed after deterministic Xcode regeneration.
- Python/portable pytest suite: **132 passed, 6 optional LLVM-oracle tests skipped, 63 subtests passed**.
- Native/source contract runner invoked by validation: **107 tests OK, 6 optional LLVM-oracle skips**.
- Focused 0.4.8 review-journal regression file: **6 passed**.
- All **40** shipped Swift sources under App/Shared/PlatformServices parse with the available Swift compiler.
- Deterministic Xcode project/workspace/scheme regeneration: passed with byte-identical `project.pbxproj`.
- Capability schema regeneration/check: passed at **222 fields**.
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

Within the portable/source-verifiable surface available here, 0.4.8 is internally consistent, preserves the stable analysis/cache line, and materially improves investigation review continuity and handoff without converting analyst workflow state into semantic evidence. Public macOS distribution remains conditional on the release-host gates above.
