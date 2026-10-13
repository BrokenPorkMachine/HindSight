# Validation — HindSight 0.5.10

Portable validation covers source topology, release identity, local dependency closure, Python/native contracts, plist/JSON/TOML parsing, shell syntax, deterministic Xcode project topology, Swift syntax parsing where available, Cargo lockfile structure, Rust 1.90 source admission, and compile checks of both the Apple-Silicon Hypervisor implementation and Intel fallback path. 0.5.10 validates the 71-field unified-symbol contract in addition to the 0.5.4 scalability budgets, indexed CFG/predecessor construction, selective Symbolication scanning, off-main analysis-result decoding, worker-native typed evidence sideband filtering, cache-v3/v2 compatibility, stage fingerprints, and Swift compatibility merging. Full Xcode/Rust linking still requires the native macOS release host.


## Universal macOS / Intel compatibility

Hotfix 5 adds regression coverage for `arm64 x86_64` Xcode settings, dual Rust targets, universal lipo packaging, x86_64 Swift type-checks, Intel native-debug stubs, service-side host gates, and the preserved cross-host ARM64 analysis/VectorEmu path. `scripts/test-beta-macos.sh` also verifies that the app executable, Coordinator service, `reworker`, and `vg-emu-server` each contain both architecture slices.

## Generated-target type-check coverage

`Config/project-topology.json` is the source of truth for Xcode target membership. `scripts/list-target-swift-sources.py` reads that generated topology, validates every Swift path remains inside the repository and exists, and feeds the source list to both `test-app-typecheck.sh` and `test-coordinator-typecheck.sh`.

This closes the gap that allowed `AnalysisNavigation.swift` and `AnalysisContinuity.swift` to be present in the Xcode app target while absent from the previous hand-maintained app type-check list. Portable regression coverage requires the helper output to exactly equal the generated target Swift list.

## 0.4.9 hotfix regressions

Validation explicitly protects the complete hotfix line:

- `VGTagPill` retains the optional `systemImage` initializer used by analysis-navigation pills.
- Collection-backed `ForEach` sites use explicit `id: \.id`, preventing SwiftUI from selecting the `Range<Int>` overload.
- `VGBoundedEvidenceList` incrementally exposes evidence rather than eagerly presenting thousands of interactive/context-menu rows.
- Function Neighborhoods keeps complete connectivity counts but bounds detailed materialization to the selected top functions/edges.
- App/Coordinator release-host type-checks are topology-derived, so newly generated target Swift sources cannot be omitted silently.

## Project-store trust boundaries

Project root, database, capture storage, staging, analysis-cache storage, and final staged/cache files are treated as filesystem trust boundaries. Source validation requires:

- `objects`, `staging`, and `analysis-cache` to be real directories rather than symbolic links;
- captured staging input to be a regular non-symlink file with the expected size;
- an existing cache destination to be a regular non-symlink file before atomic replacement;
- the existing final-file no-follow/open and source-identity verification paths to remain intact.

These checks complement the project-root/database symlink rejection and the native/Rust final-file `O_NOFOLLOW` admission path.

## Analysis/runtime invariants retained

- Product/service/worker identity is `0.5.10`, release channel `stable`, bundle build `5910`, protocol ABI 1.
- Swift/Rust agree on the exact 72-field `AnalysisReport`.
- Cache envelopes bind source SHA-256 and `arm64-analysis-v17-schema72`; only explicitly validated legacy cache paths are admitted.
- Staged worker progress remains bounded, monotonic pipeline telemetry and is filtered from ordinary diagnostics.
- Large-CFG dominators remain per-function/budgeted; Function Intelligence distinguishes internal/external calls; tail-call candidates remain exact-entry candidates.
- Continuity schema 5 remains bounded to 1 MiB and preserves pins, recent pivots, collections, review baselines/journal, and follow-ups without reclassifying workflow state as evidence.
- Responsive workspaces must preserve the 900×680 supported minimum without fixed-layout overlap.

## Native macOS gates

On either supported macOS architecture, `./scripts/test-macos.sh` runs the topology-derived dual-architecture Swift type-checks, portable suite, universal Rust bridge/worker build, native bridge contracts, full Debug app build, embedded XPC/worker/VectorEmu checks, universal-slice checks, and signing verification. `./scripts/test-beta-macos.sh` strengthens this with Release packaging. `./scripts/test-release-macos.sh` is the stable distribution gate and requires Developer ID signing, notarization, stapling, and stable bundle identity.

Manual release-host checks remain documented in [`MACOS_VALIDATION.md`](MACOS_VALIDATION.md) and [`RELEASE_READINESS.md`](RELEASE_READINESS.md), including real firmware analysis, Function Neighborhoods responsiveness, filesystem symlink-boundary substitution tests, dynamic debugger/Hypervisor/VectorEmu execution, 8 GiB memory pressure, and multi-size UI qualification.

## Evidence Core 0.5.0

Validate that `App/AnalysisEvidenceCore.swift` is in the generated app target, the compatibility transport remains exactly 66 fields, indexing runs through a utility-priority detached task, and Triage/Search/Related Evidence consume `currentEvidenceIndex`. Large reports should show an explicit indexing state and then navigate without repeatedly reparsing report text.

## Evidence Core 0.5.0

Validate `App/AnalysisEvidenceCore.swift` is in the generated app target, the wire transport remains exactly 66 fields, indexing uses a utility-priority detached task, and Triage/Search/Related Evidence consume `currentEvidenceIndex`. Large reports must show explicit indexing state rather than a false empty result.
