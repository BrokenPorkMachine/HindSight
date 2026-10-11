# HindSight 0.5.0 — Evidence Core

0.5.0 introduces the first typed evidence layer without breaking the proven worker protocol. The Coordinator still receives the exact ABI-1, 66-field `AnalysisReport`, preserving project/cache compatibility. The app converts that report once into an immutable `VGAnalysisEvidenceIndex` outside the SwiftUI render path.

Each admitted evidence line becomes a `VGEvidenceRecord` with a stable semantic identity, source/workspace/kind provenance, normalized search text, explicit address/symbol fields when present in the validated report, and original text for presentation/export.

The index is bounded to 50,000 records and contains address and symbol indices, stable focus ordering, source ranges, and source-specific focus limits. Search, Analysis Triage, Related Evidence, and current collection-health membership consume the indexed representation instead of repeatedly splitting and reparsing all report strings during view recomputation.

Index construction runs at utility priority and publishes one immutable snapshot back to the main actor. While indexing, evidence surfaces show an explicit progress state rather than incorrectly reporting no findings.

Compatibility remains: product 0.5.0 stable / build 5000; protocol ABI 1; exact 66-field report; analysis engine/cache identity `arm64-analysis-v11-schema66`; project SQLite schema unchanged; cache envelope schema 2; universal Intel/Apple Silicon behavior retained.

## Memory and persistence

The app retains at most eight complete compatibility reports at once. The selected artifact has one typed evidence index; older reports can be evicted and transparently reloaded/reanalyzed through the existing project cache path when Binary Diff needs them. Launch-session writes are coalesced with a 350 ms debounce so rapidly changing search/address state does not cause synchronous preference churn.
