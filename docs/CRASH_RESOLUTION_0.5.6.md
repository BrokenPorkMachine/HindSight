# HindSight 0.5.6 — Crash address reconciliation

New standalone `scripts/crash-resolve.py` resolves **text `.crash`** frames against existing unified-symbol function ranges. Requires an explicit Mach-O LC_UUID and **static image base**; checks the runtime image UUID in the crash Binary Images section and composes the ASLR slide. Bounded input (16 MiB), images (4096) and symbol ranges (4096). Overlapping evidence and missing UUIDs fail closed.

Usage: `python3 scripts/crash-resolve.py report.crash --symbols analysis.json --uuid AABB... --static-base 0x100000000 > resolved.crash`

The JSON must contain the 71-field report's `symbol_resolver` string. Alternatively, `--symbol-text resolver.txt`. This tool does not handle Apple `.ips` JSON, DWARF lines, indirect stubs without function ranges, dSYM lookup, or GUI invocation. A matching UUID is required but does not independently authenticate report provenance; verify the analysis report was produced from the same binary.

The app's native analysis protocol remains at 71 fields and cache identity remains unchanged.
