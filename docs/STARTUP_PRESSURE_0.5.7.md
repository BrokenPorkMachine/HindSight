# Pressure-aware cold start

- First frame renders without activating the coordinator XPC connection.
- Coordinator connection starts after a 900 ms asynchronous grace period.
- Serious/critical macOS thermal state delays nonessential activation in 1.5 s increments, capped at 12 s.
- Explicit user actions calling `connect()` bypass the grace period.
- Plugin discovery no longer runs automatically after a fresh coordinator handshake; plugin views request it on demand.
- Project restoration still occurs on coordinator connection to preserve existing session behavior.

## Boundaries
This is startup deferral, not a complete system-wide memory-pressure scheduler. Thermal state is not a substitute for available-memory telemetry. XPC service activation, saved-project restoration, and view-specific `.onAppear` work require profiling on macOS. No full native Xcode build was run here.
