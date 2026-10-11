# Runtime admission v0.5.9

Before spawning a heavyweight analysis worker, the coordinator now checks its own resident memory and macOS thermal state. Serious/critical thermal pressure or coordinator RSS above one-third of physical RAM rejects new heavy work with an actionable error. This is deliberately conservative for 8 GB machines.

**Not a global scheduler:** other processes' resident memory, compressed memory, total system pressure, active worker preemption, and cross-service leases are not yet accounted for. Firmware and scripting retain their existing bounded lanes. Worker lock and Rust lease admission remain in force.

**Build requirement:** the admission helper is embedded in `WorkerSupervisor.swift` to preserve Xcode target membership. Native macOS build and pressure tests were not run in this environment.
