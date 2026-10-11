# HindSight 0.4.0-beta.13 — Analysis scalability hardening

## Failure addressed

During large firmware analysis, the previous structural-analysis pass initialized one complete dominator set for every basic block in the entire admitted CFG. That representation is quadratic in the global block count. It is acceptable for tiny fixtures but unsafe for branch-heavy firmware on an 8 GB host.

The libdispatch signal thread (`_dispatch_sig_thread` / `__sigsuspend_nocancel`) is not itself a crash location. A worker terminated by memory pressure or a signal can cause Xcode to stop the debug process tree while that harmless signal-wait thread is visible. Beta.13 makes worker signal termination explicit in diagnostics.

## New structural-analysis contract

- Dominator solving is partitioned by discovered function.
- At most 1024 basic blocks per function are admitted to full dominator/IDOM/frontier detail.
- A function above that limit is not partially interpreted: HindSight states that dominator detail was withheld by the memory budget while keeping CFG and edge evidence available.
- Dominance frontiers are emitted only for blocks with a resolved immediate-dominator chain.
- Structural phi-site discovery remains bounded and evidence-based.
- The obsolete whole-program successor-set copy is removed from the dominator stage.

This converts the dangerous memory term from a quadratic function of the whole-program CFG to a bounded per-function working set.

## Function Intelligence scaling

Function Intelligence now indexes symbol names once and performs keyed lookups while ranking functions. The previous implementation performed a linear symbol-table search for every discovered function, which becomes expensive on symbol-heavy binaries.

## Worker failure isolation

CoordinatorService continues to run heavy analysis in the external `reworker` process. If that process exits because of an uncaught Unix signal, beta.13 reports the signal-specific failure instead of presenting a generic worker status. A worker fault is not promoted into an application crash.

## Release-host verification

On the Apple Silicon release host, analyze the same firmware that exposed the beta.12 failure while watching Memory in Activity Monitor / Instruments. Confirm that CoordinatorService and HindSight remain responsive, the worker either completes or fails with a bounded diagnostic, and no whole-system memory-pressure termination occurs.
