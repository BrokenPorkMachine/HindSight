# HindSight 0.4.4 — Investigation Continuity

0.4.4 preserves analyst context without changing the ARM64 evidence protocol. The exact 66-field `AnalysisReport`, protocol ABI 1, cache envelope schema 2, and `arm64-analysis-v11-schema66` engine identity remain unchanged.

## Project-local continuity state

Analysis pins, successful evidence pivots, and analysis-channel comparison metadata live in `.vectorglass/analysis-continuity-v1.json` beneath the project. The file is metadata, not authoritative evidence: the captured object and validated `AnalysisReport` remain authoritative.

The continuity document is capped at 1 MiB. Pins are capped at 128 entries and recent pivots at 96. State paths/files are rejected when symbolic links are detected, and writes use atomic replacement. Evidence snippets are bounded; analyst note contents are not copied into continuity state.

## Triage modes

**Focus** keeps the source-prioritized 0.4.3 queue. **Pinned** contains analyst-selected evidence for the current artifact. **Recent** records successful Source/Code/CFG/Symbol pivots and de-duplicates repeated destinations. **Changes** compares the current complete report to the previous snapshot for the same record using per-channel FNV-1a fingerprints and line counts.

The change ledger is intentionally conservative. A changed fingerprint means bytes in the rendered evidence channel changed; it does not mean the change is security-relevant, semantically important, reachable, or causal. The fingerprint is for change detection only and is not a cryptographic integrity primitive.

## Relaunch continuity

The launch-session record advances to schema 2 while accepting schema 1. In addition to project, selected artifact, workspace, and layout, schema 2 preserves bounded address text, CFG focus, analysis search text, and selected symbol. This restores navigation context only after Coordinator successfully reopens the project.

## Compatibility

0.4.4 does not change project SQLite schema, XPC ABI, worker framing, or the analysis report schema. Existing 0.4.2/0.4.3 schema-2 analysis caches remain eligible for reuse when source and engine identity validate.
