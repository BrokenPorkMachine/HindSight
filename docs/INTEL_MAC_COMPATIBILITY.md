# Intel Mac compatibility — 0.4.9 Hotfix 5

HindSight is built as a universal macOS application with `arm64` and `x86_64` slices. The host architecture controls only features whose implementation actually depends on Apple-Silicon AArch64 execution state.

## Available on both Apple Silicon and Intel

The following are cross-host and remain enabled on Intel Macs:

- project creation, source capture, notes, evidence, review journals and handoffs;
- ARM64 / ARM64e Mach-O parsing and static analysis, including CFG, SSA, xrefs, Function Intelligence, Function Neighborhoods, data references and analysis quality;
- Binary Workspace, Binary Diff, Graph, Trace/Evidence/Investigation workspaces that operate on stored analysis evidence;
- Python scripting, plugins/extensions and project automation;
- dyld cache/IPSW/public firmware-key tooling;
- deterministic **VectorEmu** software emulation, built as both `arm64` and `x86_64`;
- external backends when the selected executable itself supports the Intel host.

ARM64 analysis is analysis of the **target bytes**, not execution of those instructions on the host, so it remains cross-host.

## Closed out on Intel

The current implementations below are Apple-Silicon-only and are disabled when HindSight runs as an `x86_64` process:

1. **Hypervisor RE / SoC Virtualization execution.** The backend uses the Apple-Silicon AArch64 Hypervisor.framework vCPU/register API and guest model.
2. **Native ARM64 Debugger.** The current debugger decodes `ARM_THREAD_STATE64`, uses AArch64 register semantics, and installs ARM64 `BRK` software breakpoints.

The workspaces remain represented in the product so project/navigation state remains stable, but their controls are disabled/closed with an explicit host-requirement explanation. The Coordinator repeats the same check; UI gating is not the security/correctness boundary.

An `x86_64` HindSight process running under Rosetta is treated like Intel for these features. This is deliberate: feature availability follows the executing process slice, not merely the physical CPU underneath it.

## Universal build prerequisites

Both Rust standard-library targets must be installed on the build host:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

The checked-in Xcode project builds `arm64 x86_64`, and the Rust bridge, `reworker`, and `vg-emu-server` are lipo-combined into universal artifacts for Release builds.

## Qualification matrix

Before distribution, run the macOS release gates on both an Intel Mac and an Apple-Silicon Mac. On Intel, verify that the Apple-Silicon-only workspaces are visibly unavailable while static analysis and VectorEmu remain functional. On Apple Silicon, additionally execute the Hypervisor and native ARM64-debugger smoke tests.
