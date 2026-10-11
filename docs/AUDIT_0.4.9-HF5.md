# HindSight 0.4.9 Hotfix 5 — Universal macOS / Intel compatibility audit

## Scope

This audit starts from the 0.4.9 Hotfix 4 audit-closure baseline and evaluates whether HindSight can ship as a universal macOS product without weakening the existing ARM64 analysis, project-security, analysis-bounding, or review-workflow contracts.

Internal product identity intentionally remains **0.4.9 stable / build 4109 / protocol ABI 1**. The analysis engine remains **`arm64-analysis-v11-schema66`** with the exact **66-field `AnalysisReport`**, so compatible analysis caches are not invalidated by this host-portability hotfix.

## Result

**Portable/source audit: PASS.** The product is now architected for `arm64` and `x86_64` macOS slices. Static ARM64/ARM64e analysis remains cross-host. Features tied to the current AArch64 host implementation fail closed on Intel at both presentation and service/native boundaries.

Actual Xcode linking/running on real Intel and Apple-Silicon Macs remains a release-host gate and is not claimed by this Linux environment.

## Host feature matrix

| Capability | Apple Silicon arm64 process | Intel/x86_64 process |
| --- | --- | --- |
| Projects, capture, notes, continuity/review | Available | Available |
| ARM64/ARM64e Mach-O static analysis | Available | Available |
| Binary Workspace/Diff, CFG, evidence, triage | Available | Available |
| Python/plugins/extensions/automation | Available | Available |
| dyld/IPSW/firmware tooling | Available | Available |
| VectorEmu software execution | Available | Available |
| Native ARM64 debugger | Available when macOS policy permits | **Closed out** |
| Hypervisor RE / SoC AArch64 execution | Available on supported Apple Silicon | **Closed out** |

The debugger/Hypervisor closeout is based on the **executing process architecture**. An x86_64 slice running under Rosetta follows the x86_64 behavior and must not enter the arm64-only host backends.

## Correctness changes

### 1. Universal application and embedded artifacts

- Generated Xcode settings use `ARCHS = "arm64 x86_64"`.
- `build-app.sh` builds the generic macOS destination with both slices and `ONLY_ACTIVE_ARCH=NO`.
- Rust bridge and `reworker` build for both `aarch64-apple-darwin` and `x86_64-apple-darwin`; universal artifacts are produced with `lipo`.
- VectorEmu builds the same two Darwin targets and lipo-combines `vg-emu-server`.
- macOS qualification checks `HindSight`, `CoordinatorService`, `reworker`, and `vg-emu-server` with `lipo -archs` and requires both slices.
- `doctor.sh` admits either Intel or Apple-Silicon Macs and requires both Rust targets.

### 2. Shared runtime host capability boundary

`Shared/HostPlatform.swift` is included in both the app and Coordinator generated targets. It distinguishes `arm64`, `x86_64`, and unknown process architectures and publishes explicit availability for:

- Apple-Silicon Hypervisor execution;
- ARM64 native debugging;
- cross-host VectorEmu.

Diagnostics report the process architecture and these host-dependent feature states.

### 3. Intel closeout is defense in depth

The current Native Debugger and Hypervisor RE/SoC execution implementations are not emulated or falsely reported as available on Intel.

UI closeout covers:

- Workspace menu;
- sidebar/tool cards;
- selected-artifact workspace routing;
- no-artifact workspace routing;
- restored/deep-linked workspace state;
- firmware-tool shortcut into Hypervisor RE.

The Coordinator repeats the host checks in `DebugServer` and `VirtualizationHost`. Native C code also compiles an explicit non-arm64 failure implementation instead of importing ARM thread-state or AArch64 Hypervisor APIs on x86_64.

This means plugin/XPC/direct model calls cannot bypass the visual disabled state.

### 4. Cross-host ARM64 analysis remains enabled

The analyzer consumes captured target bytes. ARM64/ARM64e decoding, CFG, SSA, xrefs, Function Intelligence, Function Neighborhoods, data references, quality/triage, Binary Diff and evidence workflows are not gated by host architecture. This is intentionally separate from executing an ARM64 process natively.

VectorEmu also remains enabled because it is a software machine model and now has an x86_64 macOS build path.

## Build and validation coverage

The release-host type-check gates now run the full topology-derived Swift source sets for both:

- `arm64-apple-macos14.0`
- `x86_64-apple-macos14.0`

Current generated source sets contain **33 Swift sources in the app target** and **15 Swift sources in CoordinatorService**; `HostPlatform.swift` is included in both. There are **41 shipped Swift source files** under App/Shared/PlatformServices.

Portable validation after the final host-gating pass:

- `pytest`: **152 passed, 6 optional LLVM-oracle tests skipped, 63 subtests passed**;
- source/native contract runner: **115 passed, 6 optional LLVM-oracle tests skipped**;
- deterministic Xcode project regeneration: **passed byte-for-byte**;
- exact Swift/Rust analysis protocol: **66 fields, passed**;
- capability schema: **231 fields, passed**;
- Hypervisor C compile contracts: Apple-Silicon implementation + Intel fallback **passed**;
- native debugger x86 fallback syntax compile with warnings-as-errors: **passed**;
- Cargo lockfile structural preflight: **passed**;
- VectorEmu Rust 1.90 source-admission preflight: **passed**;
- modified shell script syntax: **passed**.

The aggregate Rust build cannot run in this container because Cargo is unavailable. That is an environment limitation, not marked as a passing build.

## Release-host gates still required

Before public distribution, run the stable release gate and manual matrix on **both** host families.

### Intel Mac

- build/run the native x86_64 slice;
- prove ARM64 static analysis and VectorEmu work end to end;
- prove debugger/Hypervisor/SoC workspaces are closed from every navigation path;
- prove direct Coordinator calls to those host-bound backends fail closed;
- execute the existing project symlink-boundary, cache, Function Neighborhoods, responsive UI and continuity tests.

### Apple Silicon Mac

- repeat the common matrix;
- additionally exercise Native ARM64 Debugger and Hypervisor RE/SoC execution;
- run the 8 GiB firmware pressure baseline.

### Universal artifact

On either release host, inspect all distributed executables with `lipo -archs` and require both `arm64` and `x86_64`, then complete Developer ID signing, notarization, stapling and Gatekeeper assessment.

## Conclusion

No analysis ABI, project database migration, or cache invalidation is required for Intel support. Hotfix 5 makes Intel a supported host for the portable HindSight workbench while keeping Apple-Silicon-only execution features visibly and programmatically closed when the x86_64 process slice is running.
