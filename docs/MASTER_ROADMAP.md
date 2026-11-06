# HindSight Master Roadmap

Status: active execution authority. Updated 2026-11-06.

This is the one roadmap used to choose work. It consolidates the former product,
Chimera completion, decompiler-gap, reconstruction-first, development, ECU, and
enterprise plans. Historical versions live in `docs/archive/roadmaps/`; release
audits and architecture/specification documents remain evidence/reference material,
not competing work queues.

## Operating rules

1. Do not claim a capability from a decoder, UI, fixture, or plan alone. It needs
   bounded implementation, a compatible transport/UI path where user-facing, and
   an acceptance gate.
2. Preserve captured inputs. Firmware emulation, patch staging, and exports must
   remain explicit and non-flashing.
3. Treat unknown effects as unknown. A missing memory model, type proof, runtime
   provider, or hardware qualification blocks that claim rather than being filled
   with synthetic success.
4. Keep source-backed static evidence, observed runtime evidence, derived results,
   and analyst assertions distinct.
5. Work in dependency order unless a lower item unblocks the current one.

## Current baseline

- macOS/Apple Silicon is the official host; Intel remains experimental.
- `0.5.15` / protocol ABI 1 is the current source baseline. See [STATUS](STATUS.md)
  and [release readiness](RELEASE_READINESS.md) for exact identity and distribution
  gates.
- Static Mach-O, ELF, PE, ECU/ME7, XDF relocation, project/case evidence, REA, and
  bounded execution surfaces exist. Their individual boundaries are described in
  [binary formats](BINARY_FORMATS_AND_ANALYSIS.md), [ECU support](ECU_FIRMWARE_SUPPORT_PLAN.md),
  [target model](TARGET_MODEL.md), and [extension trust](EXTENSION_TRUST.md).

## Ordered delivery tracks

### 0. Release integrity and operational qualification — P0

- [ ] Run the Apple-Silicon release-host matrix: signed/notarized distribution,
  clean install, physical-target smoke, memory-pressure, crash recovery, and visual
  layout checks.
- [ ] Keep release manifests, support diagnostics, policy/trust decisions, and case
  integrity verification reproducible and bounded.
- [ ] Qualify live optional integrations (REA provider, third-party reversing tools,
  Linux native attach) only on their actual hosts; retain a clear unavailable state
  elsewhere.

**Gate:** `RELEASE_READINESS.md` is complete with attached host evidence. Source-only
builds never substitute for this gate.

### 1. Evidence-backed reconstruction — P1

- [ ] Recover source-level variables and stack locals only from complete SSA/effect
  evidence; preserve ambiguity, aliases, and destructive calls as blockers.
- [ ] Expand type recovery through validated ABI, SDK/project layouts, Objective-C,
  C++, and Swift evidence without inventing declarations or implementations.
- [ ] Structure broader proven control flow and indirect dispatch/switch patterns.
- [ ] Extend architecture-neutral IR and add x86-64 Mach-O lifting when the shared
  model and user demand justify it.
- [ ] Qualify generated C with bounded harnesses and differential behavior checks;
  do not call generated code original-source recovery or general equivalence.

**Gate:** each inference has source instruction/evidence provenance, bounded failure
behavior, corpus coverage, and an explicitly scoped validation result.

### 2. ECU firmware analysis and bounded C167 execution — P1

- [x] Import and structurally analyze ME7-class images; verify checksum descriptors,
  stage/checksum repair/export, import XDF, and relocate maps with evidence states.
- [x] Provide bounded C167 decoding/emulation, DPP/EXTP, compare-adjust loops,
  near/segmented calls, CSP, tracing, breakpoints, watchpoints, and private RAM seed
  overlays.
- [ ] Add descriptor-driven runtime-state construction for verified checksum-routine
  qualification. It must seed documented RAM state separately from flash and compare
  the emulated result with the static checksum verifier across the corpus.
- [ ] Model remaining routine-critical C167 forms (including bit/system control where
  admitted), segmented/interrupt behavior only where needed and proven, and retain
  unsupported-instruction stops elsewhere.
- [ ] Extend ECU support only with real corpus evidence and explicit variant mapping;
  do not generalize Volvo/ME7 facts to arbitrary ECUs.

**Gate:** an image's own checksum routine reaches a documented terminal result from
  explicit state, matches the independent static verifier across qualified corpus
members, and reports all unsupported/missing-state boundaries.

### 3. Formats, navigation, and interoperability — P2

- [ ] Broaden qualified format coverage and data-reference reconstruction while
  preserving validated mappings and bounds.
- [ ] Add reversible, hash-bound interchange for types, comments, signatures, and
  supported external tool projects; complete live adapter qualification separately.
- [ ] Keep target/address pivots, evidence graph, collections, handoff, and case
  exports coherent across static, native, emulated, and guest targets.

**Gate:** import/export is revision-bound, conflict-aware, transactional, and tested
against malformed/changed inputs.

### 4. Runtime and experimental execution — P2

- [ ] Improve qualified native-debugger correlation and stop-context workflows.
- [ ] Continue VectorEmu/hypervisor/virtualization research only behind capability
  gates, with host/device models disclosed as models rather than hardware proofs.
- [ ] Add no execution path that bypasses coordinator leases, input identity, bounded
  resources, or explicit analyst initiation.

**Gate:** each backend has a truthful target model, lifecycle/error tests, resource
admission, and a reproducible trace/snapshot boundary.

### 5. Enterprise/local-first operations — P2

- [ ] Complete support bundle/redaction, retention/export controls, append-only
  auditable event export, and remaining recovery/health-history work.
- [ ] Evolve local policy, extension governance, and organization controls without
  implying a fleet/SSO service that does not exist.
- [ ] Complete resumable jobs/checkpointing and schema migration proof before making
  commercial stability claims.

**Gate:** operators can identify an exact build/input/policy state, reproduce a
failure without target-data exfiltration, and recover safely.

## Supporting references

| Area | Reference |
| --- | --- |
| Current shipped/unreleased state | [STATUS](STATUS.md), [CHANGELOG](../CHANGELOG.md) |
| Release gates | [RELEASE_READINESS](RELEASE_READINESS.md), [VALIDATION](VALIDATION.md) |
| ECU design and evidence | [ECU_FIRMWARE_SUPPORT_PLAN](ECU_FIRMWARE_SUPPORT_PLAN.md) |
| Product/target architecture | [PRODUCT_VISION](PRODUCT_VISION.md), [TARGET_MODEL](TARGET_MODEL.md), [ARCHITECTURE_HINDSIGHT](ARCHITECTURE_HINDSIGHT.md) |
| Enterprise controls | [ENTERPRISE_READINESS](ENTERPRISE_READINESS.md), [ENTERPRISE_OPERATIONS](ENTERPRISE_OPERATIONS.md) |
| Original plans | [archive/roadmaps](archive/roadmaps/) |

## Retired planning documents

The compatibility pages `ROADMAP.md`, `CHIMERA_COMPLETION_ROADMAP.md`,
`DECOMPILER_GAP_ROADMAP.md`, `RECONSTRUCTION_FIRST_ROADMAP.md`, and
`DEVELOPMENT_PLAN.md` deliberately contain only redirects. Their prior contents are
archived rather than silently deleted. New work must update this document and the
appropriate detailed design/reference, not revive a parallel queue.
