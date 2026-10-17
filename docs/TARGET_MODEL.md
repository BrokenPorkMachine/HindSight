# Unified Target Model

HindSight 0.2 introduces a common target identity across four execution/analysis domains:

- **Static artifact** — captured file bytes, validated Mach-O structure, preferred VM addresses, symbols, and semantic analysis.
- **Native process** — a Coordinator-hosted Mach debugger session with runtime VM state, registers, modules, execution control, and software breakpoints.
- **VectorEmu machine** — deterministic ARM64 machine state and explicit modeled bus/device regions.
- **Hypervisor guest** — isolated guest physical state exposed through the Hypervisor-assisted research backend.

The target model is deliberately capability-driven. A target advertises what it can do (`read.bytes`, `decode`, `registers`, `trace`, `snapshots`, and so on). UI and automation should test those capabilities rather than infer behavior from a backend string.

## Session header

Every target exposes a `VGSessionHeader` with:

- schema version;
- HindSight product version;
- stable target ID for the current workbench context;
- target fingerprint;
- engine fingerprint;
- model fingerprint;
- source fingerprint when the backend has a verified content digest.
- trace fingerprint when a trace view has been captured.

Session schema 2 uses SHA-256 for canonical identity tuples. Engine fingerprints hash the embedded `reworker`, `CoordinatorService`, or `vg-emu-server` executable bytes; a missing bundled executable is reported as `unavailable`. Model fingerprints hash a length-framed model description. Trace fingerprints cover the selected VectorEmu or Hypervisor trace text and target identity. Captured artifacts retain their verified source SHA-256. Runtime image paths are not presented as content hashes, and native debugger trace identity is not yet available.

## Address pivot doctrine

An address translation is shown only when HindSight can derive it from loaded evidence:

- Mach-O `__text` preferred VM ↔ captured file offset uses the validated section mapping.
- Native preferred VM ↔ runtime VM uses a matching dyld-module basename and its measured slide, then joins back to the validated static section when available. Until the running image itself is content-hashed, this join is labeled **correlated**, not cryptographically proven identity.
- VectorEmu addresses resolve only inside regions returned by the local `/regions` debugger command.
- Hypervisor guest addresses resolve only inside regions declared by the active virtualization profile.

If those relationships are absent or ambiguous, the result is **unresolved**. HindSight does not turn a plausible address guess into evidence.

## Automation context

Python/extensions receive these additional selection keys when a target is active:

- `target_id`
- `target_kind`
- `target_backend`
- `target_capabilities`

This is the beginning of the Target API. Later releases should replace backend-specific automation entry points with target capability calls while retaining explicit access to specialized backend functionality.

## Export

The Targets workspace can copy or export a versioned JSON target-session record containing the selected target identity, executable and model hashes, an available trace hash, and the most recent address pivot. This is intentionally small and inspectable so it can become a stable building block for future evidence bundles and team handoff.
