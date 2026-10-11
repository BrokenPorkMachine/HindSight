# HindSight Architecture

## Architectural goal

Keep the UI responsive, keep untrusted/expensive parsing out of the app process, preserve evidence provenance, and support multiple execution backends without turning the workbench into a monolith.

## Process topology

```text
HindSight.app (SwiftUI)
        |
        | typed XPC requests
        v
CoordinatorService.xpc
   |        |          |             |               |
   |        |          |             |               +--> Python extension subprocesses
   |        |          |             +------------------> Hypervisor.framework lab
   |        |          +--------------------------------> Native debugger bridge
   |        +-------------------------------------------> vg-emu-server (loopback only)
   +----------------------------------------------------> reworkbench-worker (Rust analysis)
```

The app owns presentation and investigation state. The coordinator owns bounded execution, subprocess lifecycle, trust decisions, and OS services. Heavy parsers and engines stay outside the SwiftUI process.

## Major subsystems

### Investigation shell
SwiftUI surfaces task-oriented layouts, navigation history, selected evidence, debugger state, emulator state, notes, extension panels, and project persistence.

### Static analysis engine
The Rust worker parses Mach-O and reconstructs functions, CFG, dominance/SSA/data flow, relocations, jump tables, Objective-C/Swift metadata, dyld chained fixups, imports/exports, pointer-auth structures, linkage, and provenance. The protocol returns bounded serialized records to the coordinator.

### Native debugger
A native Mach/ptrace bridge provides process launch/attach, thread state, memory, software breakpoints, loaded images, stack context, and live ARM64 disassembly. Debug state is represented independently of static analysis so observations can be correlated rather than conflated.

### VectorEmu
`vg-emu-server` is a standalone deterministic ARM64 research target. It is launched directly by the coordinator, accepts a bounded user-supplied ROM, binds only `127.0.0.1`, begins paused, and exposes newline-delimited JSON debugger messages.

The first engine extraction reuses the iPhemeral `iphem-core`, `iphem-arm`, `iphem-devices`, and `iphem-soc` crates. Specialized restore/signing/jailbreak/key/network machinery is deliberately excluded.

### Hypervisor laboratory
Hypervisor.framework supplies a separate high-performance execution backend for targets that benefit from hardware virtualization. It is not treated as equivalent to VectorEmu: each backend records its observation source.

### Extensions
Python runs in subprocesses, never inside the SwiftUI process. Manifests declare read and mutation capabilities. Returned actions are bounded and revalidated. `.vgplugin` bundles may be Ed25519-signed; unsigned code requires explicit local trust. Legacy `.rewplugin` bundles remain a migration format.

## Target abstraction roadmap

Today, the backends have separate protocol surfaces. The next architectural milestone is a common target identity layer:

```text
Target
 ├─ StaticArtifactTarget
 ├─ NativeProcessTarget
 ├─ EmulatedMachineTarget
 └─ HypervisorGuestTarget

AddressSpace
 ├─ module identity
 ├─ mapping identity
 ├─ architecture
 ├─ translation / slide
 └─ evidence source
```

Common operations should include read, decode, register snapshot, mapping enumeration, execution state, trace query, breakpoint intent, snapshot identity, and address translation. Backend-specific features remain available through capability discovery rather than a lowest-common-denominator API.

## Evidence model

Every material observation should be able to carry:

- `source_kind`: static / native / emulated / virtualized / analyst / extension;
- target and snapshot identity;
- address-space identity;
- source range or trace interval;
- timestamp or deterministic step counter where applicable;
- derivation algorithm/version;
- confidence class for inferred facts;
- parent evidence IDs.

The UI can then distinguish **fact**, **derived fact**, **hypothesis**, and **model behavior** without relying on color or prose alone.

## Resource boundaries

The existing worker governor and scripting limits remain central. HindSight should add the same discipline everywhere:

- bounded parser payloads and worker concurrency;
- bounded subprocess stdout/stderr;
- emulator ROM size ceiling and explicit run-step budgets;
- trace ring limits and paginated history;
- cancellable long jobs;
- snapshot storage quotas;
- no network listener exposed by VectorEmu;
- no shell interpolation for engine lifecycle.

## Security/trust boundaries

1. Treat analyzed input as hostile bytes.
2. Keep parsers and scripts outside the app process.
3. Do not automatically trust project extensions.
4. Keep engine control on loopback and authenticate/handshake before future remote support.
5. Separate “can inspect” from “can mutate/control.”
6. Never silently download proprietary firmware or secret material.
7. Preserve raw evidence so transformed views can be audited.

## Why iPhemeral remains separate

Full iPhemeral has a legitimate specialized purpose: high-fidelity Apple device research. Pulling all of it into HindSight would couple a general reverse-engineering product to restore flows, firmware catalogs, signing behavior, device identity, and other Apple-specific machinery. HindSight therefore consumes the reusable execution core and supports full iPhemeral as an explicit external backend when an authorized lab needs it.
