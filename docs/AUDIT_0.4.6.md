# HindSight 0.4.6 Release Audit

## Release identity

- Product/service/worker version: **0.4.6**
- Apple bundle build: **4106**
- Release channel: **stable**
- Protocol ABI: **1**
- Analysis report: exact **66-field** contract
- Analysis engine identity: **arm64-analysis-v11-schema66**
- Analysis cache envelope: **schema 2**
- Capability contract: **214 fields**
- Project SQLite schema: unchanged

## Scope

0.4.6 extends the project-local investigation workflow introduced in 0.4.4–0.4.5. It does not add new analysis semantics or change the Rust worker evidence schema. The release makes named collections operational at scale and turns the bounded recent-evidence list into traversable investigation history.

## Investigation workflow changes

### Collection search

The Collections surface can filter across collection names and their preserved pin text, evidence-source labels, and explicit symbol identities. Filtering is UI-only and does not mutate persisted evidence.

### Collection rename

Collections can be renamed in place with the existing 96-character bound and per-artifact case-insensitive uniqueness rule. Rename changes only the organizational label and updated timestamp. Stable pin identities and evidence provenance remain unchanged.

### Bounded collection health

When a validated report is loaded, HindSight derives stable pin identities from a bounded corpus of up to 25,000 structured analysis rows. Each collection reports the number of preserved pins found in that corpus. Pins not present are labelled **preserved**, not invalid or semantically stale. When no analysis is loaded the UI reports that fact instead of guessing health.

### Previous/Next evidence traversal

Recent Source/Code/CFG/Symbol visits now support Previous and Next traversal. History replay uses the existing visit without recording a duplicate. Opening new evidence resets the transient traversal cursor. Workspace menu commands expose the same actions.

## Compatibility

The analysis engine identity, protocol ABI, exact 66-field `AnalysisReport`, project SQLite schema, and cache envelope schema remain unchanged. Compatible 0.4.2–0.4.5 schema-2 analysis caches remain eligible for reuse when source and engine identity checks succeed. New cache writes carry producer version 0.4.6.

## Validation performed in this environment

- `scripts/validate-source.py`: passed.
- Python/portable pytest suite: **120 passed, 6 optional LLVM-oracle tests skipped, 63 subtests passed**.
- Native/source contract runner invoked by validation: **107 tests OK, 6 optional LLVM-oracle skips**.
- All shipped Swift sources under App/Shared/PlatformServices parse with the available Swift compiler.
- Deterministic Xcode project/workspace/scheme regeneration: passed.
- Capability schema regeneration/check: passed at **214 fields**.
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

Within the portable/source-verifiable surface available here, 0.4.6 is internally consistent and preserves the stable analysis/cache line while materially improving investigation-set management and evidence-history navigation. Public macOS distribution remains conditional on the release-host gates above.
