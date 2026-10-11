# HindSight 0.5.3 — Symbolication

Adds static symbolication evidence without loading Apple private frameworks. The worker records LC_UUID build identity, LC_FUNCTION_STARTS presence, direct nlist symbols, bounded function ranges, and debug/unwind section inventory. A separate Apple Symbolication view identifies load commands, imports, and bounded strings that reference Symbolication/CoreSymbolication-style APIs.

File/line symbolication is intentionally not claimed without matching DWARF/dSYM information. A future dSYM ingestion layer should match LC_UUID exactly before contributing source locations.

The analysis report is now 70 fields and engine identity `arm64-analysis-v13-schema70`.
