# HindSight 0.4.9 Release Audit

## Release identity

- Product/service/worker version: **0.4.9**
- Apple bundle build: **4109**
- Release channel: **stable**
- Protocol ABI: **1**
- Analysis report: exact **66-field** contract
- Analysis engine identity: **arm64-analysis-v11-schema66**
- Analysis cache envelope: **schema 2**
- Capability contract: **226 fields**
- Analysis continuity document: **schema 5** (reads schemas 1/2/3/4/5)
- Project SQLite schema: unchanged

## Scope

0.4.9 extends the 0.4.8 review journal into an explicit next-action workflow. Investigation collections can now carry bounded follow-up tasks with Open/Completed state and a small exact-identity evidence context snapshot. The feature is entirely in the project-local continuity/UI layer: the Rust worker evidence schema and ARM64 analysis engine are unchanged.

## Review Actions & Follow-Up Queue

### Separate workflow object

A follow-up is intentionally not an analysis finding. It records a collection ID, artifact record ID, bounded title, optional bounded context note, Open/Completed state, creation/completion timestamps, and up to 16 exact evidence identities captured from the collection at creation time. The UI and handoff state explicitly that follow-ups are analyst-authored tasks rather than vulnerability, severity, exploitability, correctness, reachability, or semantic-importance conclusions.

### Bounded persistence

The existing 1 MiB `.vectorglass/analysis-continuity-v1.json` ceiling remains authoritative. Additional 0.4.9 limits are:

- **128** follow-ups globally.
- **32** follow-ups per collection.
- **256 UTF-8 bytes** per title.
- **1 KiB UTF-8** per optional note.
- **16** related evidence identities per task.

A full follow-up budget rejects a new task rather than silently evicting an existing analyst action. Duplicate open titles inside one collection are rejected. A persisted follow-up must bind to the same artifact record as its owning collection. Deleting a collection deletes its scoped follow-ups; unpinning evidence does not silently delete a task or its bounded historical identity context.

### Review surface

The responsive Review desk now contains a dedicated Follow-Up Queue beneath the journal. It displays open/completed counts, supports explicit create/complete/reopen/remove actions, and continues to use wrapping controls rather than fixed-width action rows. Completed tasks remain visible as workflow history within the configured bounds.

### Handoff continuity

Generated Investigation Handoff Markdown now includes a collection review summary plus a Follow-Up Queue section. Open actions are emitted as unchecked tasks; completed actions are summarized separately. The footer distinguishes workflow actions from evidence and retains the existing provenance/interpretability disclaimer.

## Continuity migration and storage

The continuity document advances from schema 4 to schema 5 and continues to decode schemas 1–4 in place. Follow-ups are sanitized against existing collections, record identity, bounded strings, state/completion consistency, and related-identity size limits. Existing regular-file/symlink checks and atomic writes remain unchanged.

## Compatibility

The analysis engine identity, protocol ABI, exact 66-field `AnalysisReport`, project SQLite schema, and cache envelope schema remain unchanged. Compatible 0.4.2–0.4.8 schema-2 analysis caches remain eligible for reuse when existing source/engine identity checks succeed. New cache writes carry producer version 0.4.9.

## Validation performed in this environment

- Deterministic Xcode project/workspace/scheme regeneration: passed.
- `scripts/validate-source.py`: passed on the frozen source tree.
- Full portable pytest suite: **138 passed, 6 optional LLVM-oracle tests skipped, 63 subtests passed**.
- Focused collection/review/follow-up migration suite: **23 passed**.
- Native/source contract runner invoked by validation: **107 tests OK, 6 optional LLVM-oracle skips**.
- All **40** shipped Swift sources under App/Shared/PlatformServices parse with the available Swift 6.2.1 parser.
- Exact Swift/Rust analysis protocol remains **66 fields**.
- Capability schema regeneration/check: passed at **226 fields**.
- Cargo lockfile structural preflight: passed.
- VectorEmu Rust 1.90 MSRV source preflight: passed.
- Portable Apple-Silicon Hypervisor API shim compile contract: passed.
- Shell syntax validation: passed.
- The aggregate portable build correctly fails fast because Cargo is not installed in this container.

## Release-host gates not claimed here

The following still require the Apple-Silicon/macOS release host:

1. Real Cargo build/test/clippy of the Rust workspace and embedded worker smoke test.
2. Xcode Debug and Release linking of app/services.
3. Real Coordinator/XPC execution and schema-4 → schema-5 project migration smoke.
4. Review/Follow-Up Queue visual qualification at the supported compact/medium/fullscreen widths.
5. Real Hypervisor/Debugger/VectorEmu execution.
6. 8 GB memory-pressure and firmware-scale regression runs.
7. Developer ID signing, notarization, stapling, and Gatekeeper assessment.

## Conclusion

Within the portable/source-verifiable surface available here, 0.4.9 is internally consistent, keeps the stable analysis/cache line intact, and turns collection review into a bounded actionable workflow without reclassifying analyst intent as analysis evidence. Public macOS distribution remains conditional on the release-host gates above.
