# HindSight 0.4.0-beta.10 — Correctness, Completeness, and Session-Continuity Audit

## Disposition

`0.4.0-beta.10` is a beta-hardening release built on the beta.9 Coordinator recovery contract. The main correctness change is full-app launch continuity: HindSight may reopen the last project only after a fresh Coordinator capability handshake, and it restores selected-artifact/workspace/layout context only after CoordinatorService has accepted and returned the project snapshot.

This audit does **not** claim release-candidate qualification. The current environment is Linux and has no Cargo installation, so Apple Silicon Xcode builds, the pinned Rust build/clippy matrix, real Hypervisor/debugger/VectorEmu execution, memory-pressure qualification, signing, notarization, stapling, and installed-app launch verification remain release-host gates.

## Beta.10 correctness findings and fixes

### 1. Full app relaunch lost the active investigation context

Beta.9 correctly recovered an open project when CoordinatorService restarted while the app remained alive, but terminating HindSight still discarded which project/artifact/workspace/layout the analyst had been using.

Beta.10 adds one versioned launch-session record containing only:

- absolute standardized project path
- optional selected project-record ID
- workspace
- layout preset
- update timestamp

The encoded record is capped at **32 KiB**. Project paths are capped at **4096 UTF-8 bytes** and must already be standardized absolute paths. Selected-record IDs are capped at **256 UTF-8 bytes**. Workspace and layout values must still exist in the current product catalogs.

### 2. Saved UI state must never outrank project state

Launch metadata is not treated as an authoritative project snapshot. Startup order is now:

1. establish a fresh Coordinator connection;
2. validate exact product version, release channel, ABI, Rust bridge, analysis, and execution capabilities;
3. ask CoordinatorService to reopen the saved project;
4. accept the returned project snapshot;
5. restore selected artifact/workspace/layout only if still valid.

Project-bound actions remain gated through `projectServiceReady` while this is happening.

### 3. Stale launch sessions could otherwise trap first run

A saved project path that no longer exists or is no longer a directory is removed and HindSight returns to the normal create/open flow with the Coordinator usable. A project that still exists but is rejected by CoordinatorService remains an explicit recoverable failure rather than being silently trusted.

The Support menu now exposes **Reopen Last Project** and **Forget Saved Project** when appropriate.

### 4. Privacy boundary for continuity data

Beta.10 intentionally does **not** duplicate unsaved note bodies, analysis reports, debugger state, runtime memory observations, Python source/output, firmware paths, or emulator/virtual-machine state into preferences.

Unsaved note drafts continue to survive a CoordinatorService restart while the HindSight app process remains alive, as in beta.9. They are **not** claimed to survive full app termination in beta.10.

Diagnostics disclose whether a saved launch session exists and its project basename, but do not expose the saved full project path.

### 5. Recovery completion remains usable

The missing-path and failed-reopen branches explicitly return `projectSessionReady` to a usable no-project state and refresh plugin discovery, avoiding a partial-recovery state in which the Coordinator is healthy but ordinary workbench setup remains stale.

## Release identity and capability contract

- Product/protocol identity: **`0.4.0-beta.10`**
- Apple short bundle version: **`0.4.0`**
- Apple bundle build: **`4010`**
- Capability schema: **180 fields**
- New capabilities:
  - `launch_session_continuity`
  - `automatic_last_project_reopen`
  - `stale_launch_session_recovery`

The bridge capability payload and generated JSON schema match exactly.

## Portable validation completed

The following completed successfully in this environment:

- `python3 scripts/validate-source.py`
- **87/87 runnable Python/native tests passed**
- **6 optional LLVM-oracle differential tests skipped** because the oracle is unavailable
- complete Swift source parse sweep when `swiftc` is available
- executable portable Swift contracts for Target Model, Analysis Protocol, Analysis Status, Disassembly, Graph Model, Binary Workspace, Binary Diff, Runtime Correlation, Trace Explorer, Evidence Graph, and Investigation Ledger
- `scripts/test-hypervisor-shim-compile.sh` against the constrained portable Apple-Silicon Hypervisor API stub
- `python3 scripts/check-lockfiles.py`
- `python3 scripts/check-rust-msrv.py`
- capability-schema regeneration and consistency check (**180 fields**)
- shell syntax validation for all release scripts
- generated Xcode topology/hash consistency
- **95 unique workspaces** and **40 task-layout presets** remain present and routed

The aggregate `scripts/test-portable.sh` entry point was also exercised and correctly failed immediately at its explicit prerequisite because Cargo is not installed in this container. That failure is environmental and is **not** counted as a passed Rust gate.

## Release-host gates still required

Before RC promotion, run the clean Apple Silicon matrix in `docs/BETA_READINESS.md`, including:

1. Rust 1.90 Cargo test/clippy/release build and embedded-worker verification.
2. Xcode Debug and Release app/service builds and installed launch tests.
3. Full-app beta.10 continuity smoke: select a non-default artifact/workspace/layout, quit, relaunch, confirm handshake → project reopen → context restoration ordering.
4. Repeat continuity smoke with the project moved/deleted and confirm clean fallback; repeat with a service-rejected project and confirm retry/forget behavior.
5. CoordinatorService termination/relaunch with an open project and unsaved note draft; confirm beta.9 in-process recovery still behaves correctly.
6. App/service version and ABI mismatch injection to verify fail-closed handshake behavior.
7. Native debugger attach/launch/step/breakpoint/register/memory/module/backtrace workflows.
8. VectorEmu and Hypervisor create/run/step/reset/trace/breakpoint workflows on supported Apple Silicon.
9. Real stripped and symbol-rich ARM64 Mach-O analysis and Binary Diff workflows.
10. 8 GiB memory-pressure testing across analysis, CFG, diff, trace, evidence, ledger, repeated relaunch/reconnect cycles, and long-running sessions.
11. Release signing, notarization, stapling, Gatekeeper verification, and final installed-app smoke test.

## Release recommendation

`0.4.0-beta.10` is suitable for the next broader beta test **subject to the macOS/Rust release-host gates above**. It closes the gap between service-level recovery and full-app continuity without pretending to persist runtime state or silently copying unsaved analyst content into a second storage location.
