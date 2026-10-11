# Apple Silicon dispatch and performance audit

This audit records the current execution path and the correctness gaps found while reviewing task admission, queueing, resource control, and hardware routing. It is a baseline for measurable performance work; it does not claim parity with mature commercial disassemblers.

## Current execution path

- The app sends capture, page, and analysis requests to the Coordinator XPC service.
- `WorkerSupervisor` launches the Rust `reworker` process. The worker uses the native Mach-O C readers and Rust ARM64 analysis passes. The supervisor currently admits one worker per service, and a per-user file lock also serializes workers across service instances.
- Page requests and whole-file analysis therefore share one worker slot. The supervisor queue is bounded at 16 requests, ranks pages above analysis, and ages waiting tasks. Resource admission retries do not hold the worker slot.
- Plugin and scripting work runs in bounded XPC lanes. Firmware runtime work is kept serial because its host object owns mutable VM state.
- Hypervisor guest execution is routed through Apple's Hypervisor.framework. The guest SoC devices remain software models and must not be represented as Apple hardware acceleration.

## Findings and actions

### Fixed in this change

The page and analysis XPC entry points rejected work whenever any worker was active. That made the page-first priority queue unreachable for those request types. Both entry points now submit to the supervisor, so queue priority, status, and cancellation apply to real requests. Switching projects remains blocked while a worker is active or queued.

The scripting lane can run four requests at once, while plugin execution and plugin settings, trust, enablement, and installation share persistent per-plugin files. Concurrent operations on one plugin could read the same old state and overwrite each other's updates. A per-plugin lock now serializes those operations while allowing different plugins and standalone scripts to proceed concurrently.

### Remaining performance limits

The global worker lock and a governor configured for one worker cap static analysis throughput at one process. Queue priority cannot preempt a running analysis, so an interactive page request waits for the current worker to exit. Queue status now reports a live wait duration and position as priorities and queue membership change. The XPC request itself has a finite client deadline; long queue waits can end in cancellation when that deadline expires. The next scheduler milestone should define explicit queue-wait and execution deadlines and allow page reads to run in a separately budgeted lane when measured memory headroom permits.

The Swift resource gate estimates coordinator resident memory and checks thermal state. It does not observe system memory pressure, and its byte reservations are estimates rather than measured worker RSS. Before raising concurrency, add system pressure signals and worker peak-RSS telemetry, then size slots from observed workloads and a conservative memory budget.

The Rust `PriorityScheduler` tests a separate admission model, but the app's live scheduler is currently implemented in Swift; the Rust priority queue is not wired through the bridge. Keep one source of truth or connect and test the bridge before relying on that Rust queue in product behavior.

## Hardware and framework routing

| Work | Current or recommended backend | Reason |
| --- | --- | --- |
| Mach-O parsing, instruction decode, control-flow recovery, and dataflow | Native Rust/C on CPU | These passes contain irregular branches and pointer-heavy graph work. They need optimized code, bounded work, and parallel partitioning with deterministic merge. |
| File hashing | CommonCrypto on macOS; retain platform crypto implementations | Capture already uses Apple's CommonCrypto SHA-256 path on macOS. Hashing is streaming and can use platform-optimized CPU instructions. |
| Bulk arithmetic or regular vector transforms | Evaluate Accelerate only for identified hot loops | Accelerate dispatches vector work to CPU implementations optimized for the current processor. No current core disassembly pass is a clear vDSP/BLAS workload. |
| General disassembly and graph analysis | Do not route to Metal, ANE, or AMX by default | Instruction semantics and control-flow discovery are data-dependent. Moving them to an accelerator would add transfers, complexity, and evidence-validation risk without a demonstrated parallel kernel. |
| Guest CPU execution | Hypervisor.framework on supported Apple Silicon hosts | This is the system framework intended for hardware-assisted virtualization. Peripheral models remain explicit software emulation. |
| Plugin Python | Isolated subprocesses under bounded dispatch | Keeps arbitrary extension code outside the UI and coordinator logic; per-plugin mutation is serialized. |

Apple describes Accelerate as high-performance CPU vector computation, and `os_proc_available_memory()` as advisory app memory headroom that returns zero when the caller is not an app. The Coordinator runs as an XPC service, so that app-only API must not be used as its memory governor. A system memory-pressure dispatch source and measured child-worker RSS are better inputs for this process. [Accelerate](https://developer.apple.com/documentation/accelerate) · [Memory pressure dispatch source](https://developer.apple.com/documentation/dispatch/dispatch_source_type_memorypressure) · [Process available memory](https://developer.apple.com/documentation/os/os_proc_available_memory)

## Performance program and acceptance criteria

1. Add representative Apple Silicon benchmarks for small apps, large stripped binaries, shared caches, malformed inputs, and firmware images. Record cold/warm wall time, CPU time, peak RSS, queue wait, instructions per second, and output truncation.
2. Profile before changing kernels. Use Instruments Time Profiler and Allocations on the coordinator and worker separately; preserve benchmark fixtures and result hashes.
3. Partition independent per-section or per-function passes behind bounded work units. Keep parsing ownership explicit, bound aggregate memory, and merge results deterministically so reports and cache identities remain stable.
4. Add separate interactive and batch admission classes. Permit a page lane to overlap analysis only after memory and disk-I/O measurements demonstrate safe headroom; preserve cancellation and stale-result rejection across every lane.
5. Use adaptive concurrency from available CPU capacity, current memory pressure, worker RSS, and thermal state. Test sustained runs as well as peak throughput so performance does not rely on thermal throttling or system-wide memory pressure.
6. Require accuracy parity, deterministic output, bounded resource use, and documented benchmark improvement for each optimization. SIMD or Accelerate changes need scalar golden-vector comparisons; parallel passes need single-worker versus multi-worker byte-for-byte result checks.
