# HindSight 0.4.0-beta.13 — Analysis scalability / crash-resilience audit

## Trigger

A beta.12 debug run stopped during analysis while Xcode showed a libdispatch signal thread in `__sigsuspend_nocancel` / `_dispatch_sig_thread`. That thread is a normal signal-wait thread and is not sufficient to identify the crashing instruction. The triggered-thread backtrace was not available, so the exact runtime fault cannot be proven from that stack alone.

Static audit did, however, identify a concrete high-severity analysis defect capable of producing the observed failure class on large firmware: the dominator implementation allocated an all-block `BTreeSet` for every basic block in the admitted CFG (`vec![all.clone(); n]`). Memory therefore scaled quadratically with whole-program basic-block count. Because analysis can scan up to 4 MiB of ARM64 code, a branch-heavy image could construct a CFG large enough to cause severe memory pressure on an 8 GB host.

## Corrections

- Replaced whole-program dominator allocation with per-function dominator solving.
- Added an explicit 1024-block per-function detail budget. Over-budget functions retain CFG/edge evidence while dominator/IDOM/frontier detail is withheld with an explicit explanation.
- Dominance frontiers are produced only for resolved immediate-dominator chains.
- Removed the unused successor-set copy from the structural-analysis pass.
- Indexed symbol names once in Function Intelligence instead of linearly searching the complete symbol table for every discovered function.
- CoordinatorService now distinguishes worker termination by Unix signal and reports it as an isolated worker failure.
- Added three portable regression contracts for scalability and signal-failure behavior.

## Validation completed in this environment

- `scripts/validate-source.py`: PASS.
- Python/native portable suite: **94 passed, 6 skipped**. The skips are the optional LLVM-oracle differential checks when that oracle is unavailable.
- Swift parse and portable Swift contracts: PASS.
- Analysis report contract: 63 exact fields.
- Capability schema: **189 fields**, matching the bridge payload.
- Portable Hypervisor API-stub compile contract: PASS.
- Cargo lockfile structural preflight: PASS.
- Rust 1.90 MSRV source preflight: PASS.
- Shell syntax checks: PASS.

## Unverified release-host gates

This container does not provide Cargo/Rust or macOS/Xcode. Therefore beta.13 still requires an Apple Silicon host to run the actual Rust workspace build/test/clippy, Xcode Debug and Release builds, the original firmware analysis reproduction, 8 GB memory-pressure observation, real Hypervisor/Debugger/VectorEmu tests, signing, and notarization.

The source-level defect above is real and corrected, but without the original triggered-thread stack or a macOS reproduction this audit does not claim that it was the only possible contributor to the reported stop.
