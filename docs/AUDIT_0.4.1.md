# HindSight 0.4.1 — Stable-line consolidation audit

## Scope

0.4.1 promotes the beta.14 source line to the stable product identity while preserving protocol ABI 1 and the 66-field analysis result schema. The release is intentionally conservative: the responsive workbench, Coordinator/session recovery, firmware-scale analysis bounds, evidence-quality semantics, and beta.14 call-neighborhood model are retained. The principal new runtime feature is project-local reuse of valid analysis results.

## Release identity

- Product/protocol version: `0.4.1`
- Release channel: `stable`
- Protocol ABI: `1`
- Apple marketing version: `0.4.1`
- Apple bundle build: `4101`
- Capability fields: `195`

App and Coordinator still require an exact version/channel/ABI/capability handshake. Mixed beta/stable components are intentionally rejected.

## Analysis cache correctness

0.4.1 adds project-local analysis caching without migrating the SQLite schema.

- Cache storage lives under the project's `analysis-cache/` directory rather than `project.sqlite`.
- The cache key is the captured object's validated lowercase SHA-256 digest.
- Each cache envelope records its schema, exact product version, source digest, and bounded `AnalysisReport`.
- Reuse requires the exact `0.4.1` product version, matching source digest, exact report-field set, ordinary `AnalysisReport.matches` validation, and a new request UUID rebound through `AnalysisReport.forRequest(_:)`.
- Cache files must be regular, non-symlink files and remain within the 12 MiB analysis-XPC ceiling.
- Writes use atomic publication and the resulting file type/size is verified.
- The captured object's file metadata is stamped before cache lookup and checked again before a cached result is returned.
- Cache miss, stale version, malformed cache, or cache-write failure never blocks analysis; Coordinator falls back to the isolated worker path.

This provides cross-session reuse for unchanged project artifacts while preventing cached evidence from becoming authoritative over the captured source identity.

## Stable release-host gate

`scripts/test-release-macos.sh` is the stable distribution gate. It requires native Apple Silicon macOS plus explicit Developer ID Application identity, development team, and notarization profile. It runs the existing Debug/Release candidate matrix, verifies the embedded Coordinator/worker/VectorEmu binaries, requires notarization/stapling, and checks the final app's `0.4.1` / `4101` bundle identity.

The remaining real-target and 8 GiB pressure matrix is documented in `docs/RELEASE_READINESS.md`.

## Validation completed in this environment

- `scripts/validate-source.py`: PASS.
- Python/native unittest suite: **98 passed, 6 skipped**. The six skips are optional LLVM-oracle differential checks unavailable in this container.
- Portable Swift contracts: PASS, including Target Model, 66-field Analysis Protocol/rebinding, Analysis Status, Disassembly, CFG, Binary Workspace, Binary Diff, Runtime Correlation, Trace Explorer, Evidence Graph, and Investigation Ledger.
- Full Swift syntax parsing of shipped Swift sources: PASS.
- Portable Hypervisor API-stub compile contract: PASS.
- Cargo lockfile structural preflight: PASS.
- VectorEmu Rust 1.90 MSRV source preflight: PASS.
- Capability schema generation/check: PASS at **195 fields**.
- Shell syntax/executable validation: PASS.
- Release metadata/version consistency: PASS.

## Release-host gates not claimed here

This Linux container does not provide Cargo/Rust or macOS/Xcode. The following are therefore not represented as passed:

1. Rust workspace build/test/clippy and VectorEmu Cargo build/test.
2. Xcode Debug and Release app/XPC builds on Apple Silicon.
3. Re-analysis of the large firmware image that exposed beta.12 memory pressure, including peak-RSS observation on the 8 GiB M2 baseline.
4. Real debugger, Hypervisor.framework, and VectorEmu execution tests.
5. Real cross-session analysis-cache hit/miss smoke against a packaged macOS app.
6. Developer ID signing, notarization, stapling, and Gatekeeper assessment.

The source archive uses the stable identity because that is the requested 0.4.1 line; public distribution should occur only after the release-host matrix passes.
