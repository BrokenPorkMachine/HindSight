# HindSight 0.5.1 — Typed Worker Evidence

0.5.1 moves the Evidence Core across the worker boundary while preserving protocol ABI 1 and the exact 66-field `AnalysisReport` on worker stdout.

The Rust worker additionally emits bounded `VG_EVIDENCE` records over stderr. Coordinator treats these as structured sideband messages, not diagnostics. `VG_EVIDENCE_MANIFEST` carries fingerprints for the metadata, structure, linkage, and synthesis stages.

High-value navigation channels migrate first. Swift consumes those records directly and supplements only source channels absent from the native bundle with the 0.5.0 compatibility adapter, so Search/Triage coverage does not regress during migration.

Analysis-cache envelope schema 3 can persist validated typed evidence alongside the unchanged compatibility report. Schema 1 and schema 2 caches remain readable; older cache entries simply rebuild typed evidence through the compatibility adapter until a fresh worker analysis produces native records.

Stage fingerprints are provenance/cache-planning metadata in 0.5.1. Whole-report cache admission remains authoritative; later releases can use these fingerprints for stage-level reuse/invalidation.
