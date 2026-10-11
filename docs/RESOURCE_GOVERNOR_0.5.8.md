# HindSight 0.5.8 — pressure-aware launch policy

## Implemented
- UI shell remains available before coordinator activation.
- Deferred XPC connection while macOS reports thermal or recent memory pressure.
- Removes previous forced startup after 40 seconds under sustained pressure.
- Explicit manual Connect bypasses the startup deferral.
- Plugin discovery remains workspace-driven.
- Existing worker/scripting/firmware bounded lanes are preserved.

## Correctness
- Startup task is cancellation-aware and does not block the main actor.
- Pressure cooldown is based on the latest notification, not the first one.
- If pressure remains high indefinitely, automatic startup remains deferred. Manual connection is available.

## Remaining integration
A *global* scheduler with per-process resident-memory accounting, reservation leases, queued cancellation across XPC services, and preemption is not yet implemented. Do not interpret this release as end-to-end resource arbitration. Full native macOS compilation and pressure injection tests are required.
