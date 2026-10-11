# HindSight 0.4.9 Hotfix 5 — Universal macOS / Intel compatibility

Hotfix 5 removes the previous Apple-Silicon build lock while retaining truthful host-dependent feature boundaries.

## Architecture model

HindSight now ships one universal macOS product containing `arm64` and `x86_64` slices. `VGHostPlatform` resolves the executing process architecture and exposes feature-specific availability. An x86_64 process under Rosetta is intentionally treated as x86_64 because the unavailable backends depend on the process ABI/API surface, not merely the physical CPU.

## Cross-host functionality

ARM64 target parsing/decoding/analysis is preserved on Intel. The analysis engine consumes target bytes and does not execute them on the host. Project management, evidence/review continuity, automation/plugins, firmware tooling, and VectorEmu remain enabled on both architectures.

## Apple-Silicon-only functionality

The current Native Debugger and Hypervisor RE/SoC execution implementations remain AArch64-host specific. The UI marks those workspaces unavailable on x86_64 across every route: Workspace menu, sidebar/tool cards, restored workspace state, selected-artifact rendering, and no-artifact rendering. Coordinator/native code repeats the same rejection so direct XPC/plugin calls cannot bypass the host boundary.

## Build changes

- Xcode project `ARCHS`: `arm64 x86_64`.
- Rust bridge/worker: build both `aarch64-apple-darwin` and `x86_64-apple-darwin`, then `lipo` them.
- VectorEmu: build both macOS targets and `lipo` `vg-emu-server`.
- App and Coordinator Swift type-check gates run for both architectures.
- Candidate/release packaging validates universal slices.

## Qualification still required

This Linux environment cannot perform Xcode linking or run either macOS slice. Before distribution, run the universal release gate on macOS and complete the Intel + Apple-Silicon host matrix in `RELEASE_READINESS.md`.
