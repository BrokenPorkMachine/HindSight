# Dispatch and Scheduling Hardening (v0.5.7 candidate)

## Current architecture

- The Coordinator control queue remains serial for ProjectStore and WorkerSupervisor state.
- WorkerSupervisor already uses a single out-of-process worker, a 128 MiB reservation governor, cancellation and execution deadlines. Preserve these invariants.
- Scripting and firmware work are dispatched on separate serial utility-QoS lanes. Each lane now has an atomic finite admission budget including running and queued tasks.
- Scripting lane: 4 outstanding tasks maximum; firmware lane: 2. Saturated lanes reject with a service failure instead of accumulating jobs.
- No lane lock is held while executing operations or invoking XPC callbacks.

## Remaining engineering gates

- Native macOS arm64/x86_64 Swift type-check and Rust/Cargo build (not available on this host).
- Add unified priority classes and cancellation tokens for queued jobs, before claiming full end-to-end priority scheduling.
- Move cache lookups and JSON validation off the coordinator serial control queue when their state can be safely isolated.
- Provide per-stage runtime budgets and progress telemetry, then profile real large dyld caches on 8 GiB hosts.
- Add stress tests for concurrent XPC callbacks, plugin cancellation, firmware stop, and termination during queue saturation.
