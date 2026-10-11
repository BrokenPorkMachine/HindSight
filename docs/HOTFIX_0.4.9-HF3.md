# HindSight 0.4.9 Hotfix 3 — Function Neighborhoods Responsiveness

## Symptom

Opening **Function Neighborhoods** could make the app appear hung on large firmware analyses.

## Root causes addressed

1. The generic clickable-evidence SwiftUI surface could instantiate up to 4,000 interactive `Button` rows, each with a context menu. Function Neighborhoods frequently produces enough short rows to hit that worst-case presentation path.
2. The worker retained every incoming/outgoing neighborhood edge in per-function vectors even though the report only emits a bounded subset. That created avoidable transient memory pressure on large call graphs.

## Fix

- `VGBoundedEvidenceList` initially presents 200 rows and reveals evidence in explicit 200-row batches.
- Selection, Disassembly navigation, CFG focus, and context menus are preserved.
- Function-neighborhood generation keeps complete incoming/outgoing counts, selects the top 256 connected functions, and materializes at most 12 incoming plus 12 outgoing detail edges per selected function.
- Omitted detail is reported explicitly while full edge counts remain visible.
- Direct-call records no longer repeat a binary search for the caller function because the recovered call tuple already contains that admitted function entry.

## Compatibility

Product identity remains **0.4.9 / build 4109**. Protocol ABI 1, the exact 66-field analysis report, `arm64-analysis-v11-schema66`, project schema, and cache envelope schema 2 are unchanged. Existing compatible caches remain reusable.

## Validation

The portable regression suite covers the bounded SwiftUI renderer and bounded neighborhood materialization. Full Rust/Xcode linking remains a macOS release-host gate.
