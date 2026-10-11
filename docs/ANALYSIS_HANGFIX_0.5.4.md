# HindSight 0.5.4 — Analysis Hang Fix

## Symptom

Running **Analyze binary** could make HindSight appear frozen, particularly on large frameworks, firmware-scale Mach-O images, or branch-dense code. The failure had two independent classes: worker-side analysis could spend disproportionate time in derived graph/value-flow passes, and successful multi-megabyte analysis replies could then be decoded and fingerprinted synchronously on the SwiftUI main actor.

## Confirmed root causes

1. CFG presentation rescanned the complete edge corpus for every displayed block.
2. Predecessor value-flow performed the same full-edge rescan pattern and repeatedly rescanned predecessor instructions for each register.
3. Iterative value propagation scaled its iteration budget with CFG size and did not cap predecessor fan-in or reaching-definition cardinality.
4. Dominator analysis had a per-function memory limit, but lacked a strict total solved-block and pass budget.
5. Phi operand recovery repeatedly decoded predecessor blocks rather than reusing local-definition summaries.
6. Annotated disassembly and indirect-call resolution linearly searched the relocation corpus in hot loops.
7. Apple Symbolication evidence rebuilt/materialized a broad Mach-O symbol-name table; linkage surfaces also recovered indirect-symbol state more than once.
8. `AnalysisReply` JSON decoding, XPC UTF-8 reply-size accounting, and continuity fingerprint generation could perform multi-megabyte CPU work on the main actor.
9. Worker progress previously had a long low-observability interval during structural/linkage/value-flow work, making expensive bounded work look indistinguishable from a deadlock.

## Fixes

- CFG and predecessor evidence now build bounded indexes in single passes over edges.
- Structural analysis is globally capped at 20,000 CFG blocks; detailed dominator solving is capped at 512 blocks per function, 8,192 solved blocks total, and 64 passes per function.
- Structural phi detail is omitted above 8,192 blocks; phi-operand recovery is omitted above 20,000 blocks and reuses per-block definition summaries.
- Iterative inter-block value propagation is limited to 2,048 blocks, 32 passes, 64 predecessors per block, and 16 reaching definitions per argument register.
- Relocation lookups in annotated disassembly and indirect-call resolution use indexed address lookup.
- Symbolication framework scanning materializes only matching Symbolication/CoreSymbolication-style symbol names. Static symbolication caches `LC_FUNCTION_STARTS` presence for the pass.
- Indirect-symbol recovery is performed once and reused for provenance, linkage, caller, and GOT/auth-GOT surfaces.
- Large analysis reply decoding and Binary Diff decoding run in detached tasks; XPC reply byte counting and continuity fingerprints are also moved off-main.
- Progress now reports explicit `structure`, `runtime`, `value-flow`, `linkage`, `synthesis`, `evidence`, and `finalize` stages.
- Cache identity advances to `arm64-analysis-v14-schema70`; report field count remains exactly 70.

## Correctness policy under budgets

The new limits apply to **derived** analysis. Directly decoded metadata, bounded instruction evidence, CFG edges already recovered, symbols, relocations, dyld data, and other validated sources remain available. When a derived pass exceeds its work budget, its output explicitly states that detail was omitted or stopped at a bounded fixed point. HindSight does not silently label incomplete derived results as complete.

## Remaining review / native validation

- Compile and run the Rust worker with the pinned toolchain on the macOS release host; this container does not provide Cargo/rustc.
- Stress-test a branch-dense 4 MiB executable region and a symbol-heavy framework on the 8 GB M2 target while recording worker RSS/CPU and UI frame responsiveness.
- Repeat the same analysis on an Intel Mac host to confirm identical static evidence gating and acceptable worker resource use.
- Profile linkage/dyld passes with unusually large chained-import/export tries. They are bounded, but several evidence surfaces still independently decode some shared dyld structures and could be consolidated further.
- Consider moving continuity-state JSON encoding/writing off-main if profiling shows the bounded continuity document itself becoming large enough to cause visible latency.
- Add an optional user-visible “analysis detail budget” profile only if real-world corpora show that the fixed conservative budgets omit too much useful derived evidence.

## Build hotfix 1

The initial 0.5.4 archive contained three Rust compile errors at top-level `symbol_name_at` call sites. The helper accepts `&[(u64, String)]`, while the analysis entry point owns a `Vec<(u64, String)>`; free-function arguments require an explicit borrow. All three sites now pass `&symbols`. The unrelated `read_sleb` warning was also removed by declaring `byte` inside the decode loop. No report schema, engine identity, cache format, or analysis semantics changed.
