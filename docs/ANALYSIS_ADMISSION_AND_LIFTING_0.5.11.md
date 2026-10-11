# v0.5.11 Analysis Admission / Lifting Plan

## Integrated
- Analysis worker now reserves 512 MiB in the same process-wide ledger as firmware (192 MiB) and scripting (64 MiB).
- Lease lasts through report decoding and validation and is released by `defer` on both success and failure.
- Existing worker global lock, Rust governor, and thermal/RSS admission remain intact.

## Not yet implemented
- Queued worker admission: worker reservations currently reject immediately. Avoid blocking the serial coordinator queue waiting for memory.
- True cross-process accounting: the ledger measures reservations, not actual RSS of reworker or total system pressure.
- Symbolication/pseudocode work still runs inside the worker analysis envelope, not separate independently metered stages.
- Expanded ARM/x86 instruction decoding, C/Objective-C type recovery, and high-fidelity pseudocode require a tested lifting layer. Do not claim 1:1 source recovery: compiler optimizations erase original source constructs.

## Next design
1. Coordinator actor/serial scheduler with bounded pending queues, deadlines, per-job cancellation, priority aging, and callback-based wakeups (no busy waiting).
2. Monitor worker RSS using `proc_pid_rusage` on macOS and compare to actual system pressure; adjust estimates, stop new admissions, and terminate only opted-in background jobs on sustained critical pressure.
3. Add instruction architecture dispatch for ARMv7/Thumb2, AArch64/arm64e, x86 and x86-64. Decode to typed instruction semantics with explicit unsupported markers.
4. Lift to SSA with ABI-specific argument recovery: AAPCS64 and SysV x86-64; detect ObjC `objc_msgSend` callsite conventions and emit `-[Class selector:]` only with grounded receiver/selector evidence.
5. Emit typed C/Objective-C pseudocode with confidence/provenance per expression; retain assembly for unsupported operations and compare differential fixtures to Clang-generated binaries.
6. Keep Swift demangling distinct from source reconstruction: demangled symbol names do not prove source types or original control flow.

## Verification
Build both macOS targets, run concurrent dyld-cache/firmware/script stress tests under 8 GiB, and verify cancellation/release accounting. Not executed in this environment.
