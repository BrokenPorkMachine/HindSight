# HindSight 0.4.0-beta.8 — Correctness, Completeness, and Polish Audit

## Scope

This audit treats the uploaded `0.4.0-beta.7` tree as a beta release candidate and reviews the product from source admission through the macOS workbench shell, service boundary, protocol/version identity, analysis contracts, project persistence, debugger/emulation surfaces, build scripts, validation, and release documentation.

The audit was performed in a Linux validation environment with Swift 6.2.1 and Python 3.13.5. Cargo/Rust tooling and macOS frameworks are not installed on this host, so the Rust compile/clippy gate and the real Xcode/macOS execution matrix remain release-host gates rather than claims made by this audit.

## Result

No portable P0/P1 defect remains known after this pass. The source/contract validator is green, all 87 runnable Python/native tests pass, six optional LLVM-oracle differential tests are skipped because that oracle is unavailable, Swift source parsing succeeds, and the Hypervisor shim contract compiles against the constrained portable Apple-Silicon API stub.

The package remains **beta**, not RC. Real Apple Silicon Debug/Release builds, Rust build/clippy with the pinned toolchain, native debugger execution, VectorEmu/Hypervisor real-target testing, 8 GiB memory-pressure validation, signing, and notarization still need to pass on the intended macOS release host.

## Correctness findings fixed in beta.8

### 1. Coordinator state could be falsely reported offline

`WorkbenchModel.connected` previously became true only after a successful project snapshot was applied. The main view did not probe CoordinatorService when first presented, so a healthy service could appear offline until the user completed a project operation.

**Fix:** the workbench now performs an idempotent first-presentation capabilities probe, exposes an explicit connecting state, marks the service online on a successful reply, records interruption/disconnect recovery state, and offers an explicit reconnect action.

### 2. Import Binary could be visibly enabled while guaranteed to no-op

The model requires an open project, a live Coordinator, and no conflicting operation before import. Some UI entry points checked only the service state, and the app menu checked none of those prerequisites. Direct model invocation also returned silently when a prerequisite was absent.

**Fix:** every primary Import Binary entry point now reflects the same prerequisite set. Direct model calls produce an actionable status message for busy/offline/no-project cases instead of silently returning.

### 3. Empty-state flow mixed two different product states

A brand-new launch and an already-open empty project shared the same generic “Start an investigation” screen. This made “Import Binary” appear conceptually available before the required project existed and obscured the natural two-step flow.

**Fix:** first-run now focuses on creating/opening an investigation; an open empty project gets a separate “Add your first artifact” state with Import Binary as the primary action.

### 4. Bounded UI surfaces hid useful state without disclosure

The sidebar intentionally shows only the first 12 project artifacts, but previously gave no indication that more existed. Command-palette and sidebar searches could also become blank with no explanation.

**Fix:** the sidebar now shows the exact overflow artifact count with a one-click route to Targets, while command and sidebar searches present explicit no-result states.

### 5. Persistent status text could be truncated without a recovery path

Long service/error messages are correctly bounded to one status-bar line, but the clipped text had no discoverable way to inspect the complete message.

**Fix:** the status text now exposes the full value through help/hover and text selection while keeping the compact status-bar layout.

## Completeness checks

- Workspace catalog: **95 unique workspaces** are declared; every descriptor is routed either through the main workspace switch or the intentional Overview/Targets top-level handlers. No orphan catalog entry was found.
- Task layouts: **40 presets** are declared; every non-default preset maps to a workspace and no extra switch case exists outside the catalog.
- Release identity: product/protocol identity is `0.4.0-beta.8`; Apple short bundle version remains `0.4.0`; numeric bundle build is `4008` across app/service metadata.
- Capability schema: generated schema matches the Rust bridge capability payload.
- Project/Xcode topology: deterministic project hash/reference checks pass.
- Crash-primitives policy: production Swift sources remain guarded against `fatalError`, `preconditionFailure`, `assertionFailure`, forced `try!`, and forced casts outside the allowed test boundary.
- Project persistence/recovery contracts, analysis lifecycle, binary diff, runtime correlation, trace explorer, evidence graph, and investigation ledger contracts all pass in portable validation.
- Experimental virtualization/device-model surfaces remain explicitly labelled experimental rather than being represented as hardware-equivalent.

## Portable validation run

Passed in this audit environment:

- `python3 scripts/validate-source.py`
- JSON/plist/TOML/XML parsing and source topology checks
- Cargo lockfile structural consistency checks
- generated capability-schema consistency check
- Swift parser/contracts available on the host
- portable Hypervisor API-stub compile contract
- 87 Python/native tests covering cache, decoder, framing, integer decoder, native Mach-O handling, paging, and project SQL/recovery behavior

Skipped by design/environment:

- 6 optional LLVM differential-oracle tests (oracle unavailable)
- Cargo test/clippy/build (Cargo not installed in this audit container)
- Xcode app/service type-check/build and real Hypervisor.framework compilation (requires macOS)
- Release signing/notarization and real-target/memory-pressure matrix

## Remaining RC gates

The remaining work is environmental qualification rather than a hidden source TODO. Before RC promotion, run the clean Apple Silicon matrix in `docs/BETA_READINESS.md`, with special attention to:

1. `./scripts/test-beta-macos.sh` on the pinned Rust 1.90 toolchain.
2. Real stripped and symbol-rich ARM64 Mach-O analysis.
3. Native debugger attach/launch/step/breakpoint/memory/module/backtrace behavior.
4. VectorEmu and Hypervisor create/run/step/reset/trace/breakpoint behavior on supported hardware.
5. 8 GiB memory-pressure behavior across analysis, CFG, diff, trace, evidence, and ledger workflows.
6. Release signing, notarization, stapling, and final embedded-worker/VectorEmu verification.

## Release disposition

`0.4.0-beta.8` is suitable as the next broader-testing beta **subject to the macOS/Rust release-host gates above**. The source tree is more internally consistent and the workbench now behaves truthfully across startup, interruption, empty-project, import, search, and large-project states.
