# HindSight 0.5.7 — Scheduling / demangling / structural pseudocode

## Delivered

- Swift coordinator scripting and firmware work now passes through bounded, serialized, priority-aware service lanes. Pending requests are ordered by priority (FIFO among peers); saturation returns a meaningful failure; `cancelQueued()` invalidates previous generations, and running results are rejected when invalidated. No lock is held while executing work or invoking XPC replies.
- Existing worker admission remains memory-bounded; Rust `PriorityScheduler` adds an independent tested policy model: reserved-memory leases, bounded pending count, stable priority order, stale-generation cancellation, and publication gating.
- A conservative *structural pseudocode* preview is available inside the IR workspace. It only renders branches, terminal markers, and opaque instructions with an explicit evidence warning and size cap. This is **not full decompilation**.
- `scripts/vg-symbol-tools.swift` provides Swift ABI demangling through Apple's `swift-demangle` via `xcrun` (without a shell) and standalone structural pseudocode from a 71-field analysis JSON report. The script is portable with a Swift toolchain fallback.

## Unfinished integration / correctness limits

- The Rust priority scheduler policy is not yet the coordinator's shared operational scheduler. The worker supervisor retains its existing single-worker memory governor, and coordinator lanes remain independently bounded. There is **no global budget enforcement across worker, scripting, firmware, cache, and debug jobs** yet.
- Swift queued-job invalidation is exposed at the lane level but is not automatically triggered for every project/session mutation; worker process cancellation remains separate.
- The CLI demangler is on-demand and not yet projected into every function/symbol row in the unified resolver. A full-product Swift symbol index and symbol-name search require additional integration.
- The pseudocode preview is a structural rendering of the existing IR; no higher-level expressions, variables, function parameters, types, SSA folding, or source reconstruction are claimed. A validated ARM64 lifting and SSA transformation pipeline is required for those.
- This release does not run Rust/Cargo compilation or macOS app linking in the portable environment. Run the native release build and stress tests on both Apple Silicon and Intel hosts before distribution.

## Practical next gates

1. Connect every async subsystem to one scheduler admission/cancellation protocol, with per-project generation IDs and a main-actor publication guard.
2. Collect queue wait, runnable time, resident-byte high water marks, truncation, late-reply drops, and UI responsiveness telemetry; tune against the M2 8-GB host.
3. Add `swift-demangle` batch mode with caching in the symbol resolver and mark malformed/non-Swift symbols unchanged.
4. Implement typed lifting to a decompiler IR before offering full C-like pseudocode.

## Local CLI usage

```sh
swift scripts/vg-symbol-tools.swift demangle '$sSSN'
swift scripts/vg-symbol-tools.swift pseudocode /path/to/analysis-report.json
python3 scripts/validate-source.py
```
