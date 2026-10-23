# Intel Mac compatibility — experimental

HindSight officially supports macOS on Apple Silicon (`arm64`). Intel (`x86_64`) support is experimental, provided for research and development only, and is not a release or support commitment. The host architecture controls features whose implementation depends on Apple-Silicon AArch64 execution state.

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

## Experimental Intel build

Both Rust standard-library targets must be installed on the build host:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

Official builds target `arm64` only. To produce an experimental Intel/universal development build, set `HINDSIGHT_EXPERIMENTAL_INTEL=1`; the required Intel Rust target must be installed as well.

## Qualification matrix

Before distribution, run the release gates on an Apple-Silicon Mac only. Intel smoke tests may be run as experimental research checks, but Intel results do not qualify or block an official release.
