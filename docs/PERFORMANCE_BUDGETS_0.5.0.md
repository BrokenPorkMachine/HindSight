# HindSight 0.5.0 performance budgets

These are regression budgets and architectural ceilings, not promises about absolute timing on every Mac.

- Typed evidence index: maximum 50,000 records per selected artifact.
- Interactive generic evidence surface: 200 rows per reveal batch.
- Resident complete compatibility reports: maximum 8.
- Function Neighborhood detail: top 256 connected functions, max 12 incoming + 12 outgoing detail rows each.
- Launch-session persistence coalescing: 350 ms.
- Analysis report wire/cache contract: remains bounded by the existing XPC/cache transport limits.
- Heavy evidence indexing must execute outside SwiftUI body evaluation and publish an immutable snapshot to the main actor.

Release-host profiling should record initial evidence-index build time, first Triage paint, Search latency after indexing, peak RSS after 1/8/16 analyzed artifacts, and Function Neighborhoods interaction latency on both Intel and Apple Silicon.
