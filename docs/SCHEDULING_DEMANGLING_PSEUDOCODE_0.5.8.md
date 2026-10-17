# HindSight 0.5.8 — Scheduling, demangling, and pseudocode

## Scheduling behavior

- Analysis, page, and capture work enters the queue-confined `WorkerSupervisor`. It has one active worker, a 16-request pending bound, page-first priority, FIFO order within equal aged priority, and a two-second aging step capped after three steps.
- Queue admission retries for memory, worker leases, and the cross-process lock keep retry delays out of the active worker slot. Queued requests can be cancelled; active workers are terminated and their revoked leases prevent publication of stale results.
- Queue order is calculated by one comparator for dispatch and status. A status read reports the current queue position, readiness state, and elapsed wait based on current aging and retry eligibility.
- Scripting, firmware, and REA use separate bounded coordinator lanes. Firmware remains serial because its runtime host owns mutable state. Scripting and REA have their own concurrency and reservation limits.
- The Rust `PriorityScheduler` remains a tested policy model and is not presented as the live coordinator scheduler. Swift `WorkerSupervisor` owns worker dispatch; the Rust governor C ABI owns worker admission and publication leases.

## Pseudocode and symbol tools

- The IR workspace includes a bounded structural preview; it does not claim full decompilation.
- The ARM64 worker emits evidence-bounded C-like function bodies. Objective-C hidden parameters and selector parameters, C/C++ prototype register bindings, and straight-line parameter-name propagation are supported. Names stop at register writes and calls that may clobber them; uncertain control-flow joins preserve machine registers.
- `scripts/vg-symbol-tools.swift` uses Apple's `swift-demangle` through `xcrun`, without invoking a shell, and can render a structural preview from analysis JSON.

## Boundaries and remaining work

- One heavy worker and the cross-process file lock serialize analysis and page requests. Priority cannot interrupt a running analysis, so an interactive page can wait for that analysis to finish.
- The coordinator reservation ledger uses conservative estimates, not measured worker peak RSS. It checks coordinator resident memory and thermal state but does not subscribe to system memory-pressure events. Do not increase concurrency until pressure signals, worker RSS, disk I/O, and sustained-run behavior are measured.
- Queue and session cancellation remain subsystem-specific. A single cross-subsystem generation and resource budget is not implemented.
- Full SSA variable recovery, general control-flow structuring, source-level type propagation, x86-64 decompilation, and a unified Swift symbol index remain roadmap work.
- Run native Xcode linking and the release-host stress matrix on supported Apple hardware before distribution.

## Verification

The coordinator queue policy and its status contract are covered by the Python source contract suite. Rust scheduling policy, cancellation, memory leases, and priority FIFO behavior are covered by `cargo test --workspace`. Coordinator Swift type checking and source validation are provided by `scripts/test-coordinator-typecheck.sh` and `scripts/validate-source.py`.
