# Memory-pressure-aware startup governor

The UI renders before coordinator XPC activation. Automatic activation begins after a 900 ms asynchronous grace period. macOS DispatchSource memory pressure warnings/critical events hold automatic startup for 8/20 seconds; serious/critical thermal state also defers startup, with a 40-second retry bound. Manual reconnect bypasses the delay. Plugin enumeration is on-demand, including after project restoration. `startupPhase` and `startupPressure` publish observable startup states.

This is not full system-wide memory accounting or preemption. Native Xcode compilation and pressure injection testing on macOS are outstanding.
