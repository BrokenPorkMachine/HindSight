# v0.5.6 Dispatch Hardening (source patch)

This is a compatibility-preserving update of v0.5.6, **not** a completed end-to-end scheduler rewrite.

## Changes
- Add `BoundedServiceLane`: serial DispatchQueue, utility QoS, lock-protected admission and outstanding-job accounting.
- Limit plugin/scripting operations to four outstanding tasks including the running task.
- Limit firmware runtime operations to two outstanding tasks including the running task.
- Return explicit service failure when saturated rather than accumulate requests indefinitely.
- Preserve the existing coordinator serial queue, worker governor, and 71-field analysis protocol.
- Avoid holding lane lock while executing commands or invoking XPC replies.

## Tested
- Linux swiftc executable test: bounded admission, saturation rejection, drain and reservation reuse all pass.
- Python source contract check: all seven scripting and one firmware call sites are routed via bounded lanes.

## Pending release gates
- macOS arm64 and x86_64 `scripts/test-coordinator-typecheck.sh`.
- Native Cargo worker/Xcode build and runtime stress test on M2 8 GiB.
- Job priority, cancellation of queued non-worker tasks, stale-generation drop, and performance telemetry remain needed for a genuinely unified scheduler.

## Local build
Run the repository's `scripts/build-rust.sh` and `scripts/build-app.sh` with the usual Xcode environment; run `scripts/test-coordinator-typecheck.sh` before shipping.
