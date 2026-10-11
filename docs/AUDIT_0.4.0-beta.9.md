# HindSight 0.4.0-beta.9 — Correctness, Completeness, and Recovery Audit

## Scope

This audit treats `0.4.0-beta.8` as the baseline and focuses the next beta on a correctness gap at the macOS XPC service boundary: recovering from a CoordinatorService interruption while a project remains open in the application UI. The pass also reviews connection lifecycle, project-operation admission, capability compatibility, diagnostics, release identity, source contracts, portable Hypervisor admission, lockfile/MSRV checks, and release documentation.

The audit was performed in a Linux validation environment with Swift 6.2.1 and Python 3.13.5. Cargo/Rust tooling and macOS frameworks are not installed on this host. Rust compilation/clippy, Xcode builds, real Hypervisor.framework/debugger/VectorEmu execution, memory-pressure qualification, signing, and notarization therefore remain release-host gates and are not represented as having passed here.

## Result

The beta.9 portable source/contract suite is green. All **87 runnable Python/native tests pass**; **6 optional LLVM-oracle differential tests are skipped** because that oracle is unavailable. Swift source parsing succeeds. The generated capability schema matches the bridge payload and now covers **177 capability fields**. Cargo lockfile structure, Rust 1.90 MSRV source admission, shell syntax, and the portable Apple-Silicon Hypervisor API-stub compile contract pass.

The aggregate portable script correctly fails fast on this host because Cargo is absent. That is an environmental qualification gap, not a passing Rust build and not a source failure being hidden by this audit.

The package remains **beta**, not RC. Real Apple Silicon/macOS qualification is still required before promotion.

## Correctness findings fixed in beta.9

### 1. XPC reconnection did not imply Coordinator project-state recovery

In beta.8 the application could successfully reconnect to CoordinatorService after the service was interrupted or relaunched and mark the Coordinator online. CoordinatorService's in-memory project store, however, belongs to the service process and is lost on restart. The UI could therefore still hold a valid-looking client-side `ProjectSnapshot` while the newly launched Coordinator had no corresponding project open.

**Fix:** transport reachability and project-session readiness are now separate states. After a successful service handshake, HindSight automatically reopens the currently displayed project in CoordinatorService before project-bound controls become available. If rehydration fails, the Coordinator remains reported as reachable while project operations remain locked and an explicit Restore Project action is offered.

### 2. Explicit reconnect could reuse an unhealthy XPC generation

The previous reconnect path could call through the existing `NSXPCConnection` lifecycle rather than guaranteeing a new connection generation.

**Fix:** `CoordinatorClient.resetConnection()` now invalidates the old connection, cancels outstanding deadlines/callbacks, advances the generation token, and forces the next probe to create a fresh XPC connection.

### 3. Capability replies were treated as reachability rather than compatibility

A successful transport reply was enough to mark the service available even if the response was malformed, represented a bridge-unavailable fallback, or came from an app/service version combination that should not operate together.

**Fix:** startup/reconnect now decodes and validates a Coordinator handshake. Product version and release channel must match, ABI must be `1`, and the bridge must advertise the core Rust/analysis/execution capabilities before the session is admitted. Compatibility failures are surfaced as actionable status instead of being treated as a healthy Coordinator.

### 4. Project-bound actions needed a single readiness contract

Checking only `connected` was no longer sufficient once service reachability and project rehydration became distinct states.

**Fix:** project operations are gated through `projectServiceReady`. Analysis, instruction paging, note persistence, binary diff preparation, imports, and corresponding workbench controls remain disabled until both the Coordinator and its project session are ready. Successful project snapshots re-establish readiness atomically.

### 5. Beta support diagnostics were insufficient for service-recovery bugs

Recovery bugs are difficult to reproduce from a screenshot alone, while exposing arbitrary filesystem paths or project content in diagnostics would be unnecessarily invasive.

**Fix:** a Support menu now includes Copy Diagnostics. The report is intentionally bounded to product/build context, OS/hardware summary, Coordinator version/ABI/reachability, project-session readiness, project basename, artifact/selection counts, current workspace, activity state, and debugger/VectorEmu state. Full project paths and project contents are not copied.

## Completeness checks

- Product/protocol identity is **`0.4.0-beta.9`**.
- Apple short bundle version remains **`0.4.0`** and numeric bundle build is **`4009`**.
- Capability schema and Rust bridge payload agree on **177 fields**, including `coordinator_handshake_validation`, `coordinator_session_rehydration`, and `beta_diagnostics`.
- Project-session restore preserves the client project snapshot until the service confirms a refreshed snapshot; a failed restore does not falsely advertise project readiness.
- Explicit reconnect creates a fresh Coordinator connection generation.
- New/Open commands require a validated Coordinator handshake; project-bound operations require a restored project session.
- The empty-project/status-bar/menu surfaces expose connecting, restoring, offline, reachable-but-unrestored, and ready states distinctly.
- Existing beta.8 first-run/import/search/large-project/status-bar contracts remain under source validation.
- Workspace/task-layout, analysis lifecycle, project persistence/recovery, binary diff, runtime correlation, trace explorer, evidence graph, and investigation-ledger contracts remain covered by the portable validator.
- Experimental virtualization/device-model surfaces remain labelled experimental.

## Validation performed

Passed in this environment:

- `python3 scripts/validate-source.py`
- **87** runnable Python/native tests
- capability-schema regeneration/consistency checks (**177 fields**)
- `swiftc -frontend -parse` across the edited app/shared Swift sources
- `./scripts/test-hypervisor-shim-compile.sh` against the portable constrained Apple-Silicon API stub
- `python3 scripts/check-lockfiles.py`
- `python3 scripts/check-rust-msrv.py`
- `bash -n scripts/*.sh`
- release/version/source-contract checks for beta.9 recovery behavior

Unavailable or intentionally skipped here:

- **6 LLVM differential-oracle tests** because the optional oracle is not installed
- Cargo test/clippy/build because Cargo is not installed in this container
- Xcode app/service Debug and Release builds because this is not macOS
- real Hypervisor.framework, native debugger, and VectorEmu execution on Apple Silicon
- 8 GiB memory-pressure qualification
- code signing, notarization, stapling, and installed-app launch verification

The full `./scripts/test-portable.sh` entry point was also exercised and correctly stopped immediately with its explicit “Cargo not found” prerequisite error. This audit does not reinterpret that run as a successful Rust gate.

## Remaining release-host gates

Before RC promotion, run the clean Apple Silicon matrix in `docs/BETA_READINESS.md`, including:

1. Pinned Rust 1.90 Cargo test/clippy/release build and embedded-worker verification.
2. Xcode Debug and Release app/service builds and launch tests.
3. Terminate/relaunch CoordinatorService with an open project and unsaved workbench state; verify handshake, automatic project rehydration, preserved selection/drafts where designed, locked controls during restoration, and explicit recovery behavior on a deliberately inaccessible project.
4. Version/ABI mismatch injection to confirm incompatible service admission fails closed with useful status.
5. Native debugger attach/launch/step/breakpoint/register/memory/module/backtrace workflows.
6. VectorEmu and Hypervisor create/run/step/reset/trace/breakpoint workflows on supported Apple Silicon.
7. Real stripped and symbol-rich ARM64 Mach-O analysis and Binary Diff workflows.
8. 8 GiB memory-pressure testing across analysis, CFG, diff, trace, evidence, ledger, and long-lived reconnect/recovery cycles.
9. Release signing, notarization, stapling, Gatekeeper verification, and final installed-app smoke test.

## Release disposition

`0.4.0-beta.9` is suitable as the next broader-testing beta **subject to the macOS/Rust release-host matrix above**. Its main correctness improvement is that HindSight now recovers the service-side project session rather than merely reconnecting the transport, and it fails closed when app/service compatibility or project rehydration cannot be established.
