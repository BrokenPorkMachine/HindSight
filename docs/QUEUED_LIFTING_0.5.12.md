# 0.5.12 — queued service admission and IR sketch correctness

Scripting and firmware service lanes now retry resource admission asynchronously (up to 20 attempts at 250 ms spacing), retaining priority ordering and cancellation generation checks. Retries do not sleep on the control queue. Requests still reject when the lane capacity is full. Analysis-worker admission remains fail-fast; it is not queued by this patch.

The SwiftUI structural pseudocode renderer now parses the actual ARM64 worker IR `cbranch`, `branch`, `term`, `op`, `data`, `opaque` classifications rather than looking for prose keywords. It avoids claiming a return for arbitrary terminal transfers. It does not implement x86-64 decoding, ObjC ABI lifting, or faithful source reconstruction.

Native Xcode and Rust builds remain required before release deployment.
