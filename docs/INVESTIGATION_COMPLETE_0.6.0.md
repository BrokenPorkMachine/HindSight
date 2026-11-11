# HindSight 0.6.0 — Investigation Complete

Status: **proposed product-completion milestone**; acceptance **NOT YET QUALIFIED**.
Basis: the 0.5.15 source baseline. This document defines completion scope and
release gates; it does not attest that any untested path works.

## Product decision

HindSight is the investigation studio. Warhead is an **optional**, independently
permissioned agent integrated into the studio, never an authority for raw evidence.

**Definition of done:** A new user on a supported Apple-Silicon Mac can install or
build HindSight, import an authorized sample, analyze it, navigate observed and
derived evidence, record a falsifiable hypothesis, inspect or safely test it,
save/reopen the project, and export an independently reproducible case—without
Warhead, private provider credentials, unexplained blank states, or manual
workarounds.

Do not expand the release scope to universal decompilation, full-system Apple
hardware emulation, arbitrary ECU support, fleet operations, or production
multi-user remote access. Those remain distinct, explicitly experimental tracks.

## Mandatory release workflows

### W1 — Fresh investigation (P0)

- [ ] Clean install/source build on Apple Silicon using the supported toolchain.
- [ ] Create a project; import the open synthetic ARM64 fixture
      `Tests/Fixtures/arm64-integer-sample.macho`.
- [ ] Record source SHA-256, format, architecture, base mapping and provenance.
- [ ] Run analysis; show progress, exact outcome and non-fatal incomplete stages.
- [ ] Navigate Binary Workspace → disassembly → function → CFG → xrefs →
      analysis quality without re-entering addresses.
- [ ] Show unsupported instructions as unknown, not reconstructed source.
- [ ] Save a note tied to exact evidence, close/reopen, and export case/Markdown/JSON.
- [ ] Import the case into a fresh project and verify artifact identity and evidence
      references are preserved or explicitly flagged as not portable.

Gate: one reproducible end-to-end fixture transcript and export manifest, with
negative cases (missing input, malformed file, invalid mapping, partial analysis).

### W2 — Real-world corpus and reconstruction (P0/P1)

- [ ] Use a legal corpus including stripped and symbol-rich ARM64 Mach-O, ELF and
      PE, plus a moderate large binary on the 8 GiB baseline.
- [ ] Classify each recovered value as observed, decoded, inferred, unresolved
      or analyst-authored; do not promote partial SSA/ABI evidence to source truth.
- [ ] Validate direct calls, indirect calls, stack variables and typed signatures
      against instruction-level provenance and known fixtures.
- [ ] Check reconstructed C only with explicit supported harnesses and report
      differences; do not claim general binary/source equivalence.
- [ ] Make incomplete format support and unsupported lifts visible and exportable.

Gate: deterministic corpus results, quantified coverage and bounded memory/time,
and no fabricated targets or declarations.

### W3 — Runtime and machine lab (P1)

- [ ] On a local authorized ARM64 test target, verify native debugger launch,
      stop, registers, breakpoint, memory, trace and detach lifecycle.
- [ ] Exercise VectorEmu step/reset/trace/breakpoint behavior against modeled
      fixtures and verify deterministic snapshots.
- [ ] Mark Hypervisor, SoC, SEP and similar hardware models as experimental where
      parity is not proven.
- [ ] Confirm failure, timeout, cancellation and service restart never leave
      indefinite active state, unbounded subprocesses, or silent data loss.

Gate: independently replayable observed traces and accurate capability notices.

### W4 — Investigation UX and accessibility (P0)

- [ ] Test at 900×680, 1180×760, 1440×900 and full screen on an 8 GiB Mac.
- [ ] Keep one investigation identity across all workspaces and the inspector.
- [ ] Validate keyboard navigation, command palette, focus, VoiceOver labels,
      contrast, reduced motion and no-overlap layouts.
- [ ] Add an obvious first-run journey (Create / Open / Import / Analyze / Review /
      Export) with context-specific empty, failed and unavailable states.
- [ ] Verify long-running work never stalls pointer/keyboard input or freezes
      the main actor.

Gate: recorded manual UX matrix and a non-blocking deterministic UI smoke set.

### W5 — Projects, integrity, privacy and recovery (P0)

- [ ] Verify SQLite schema 1/2 → 3 migration on copies; never silently mutate
      the only retained source. Verify quick_check and crash/restart recovery.
- [ ] Verify SHA-bound analysis caches, atomic writes, exact evidence references,
      stale-cache rejection, path/symlink defenses and no accidental exfiltration.
- [ ] Distinguish project/target loss, worker crash, cancellation and auth failure
      in visible recovery guidance.
- [ ] Export a portable, independently verifiable evidence report with tool/version
      identity, input hashes, unknowns and analyst assertions separated.

Gate: independent corrupt/malformed/symlink/race cases and restore evidence.

### W6 — Warhead Community (optional, separately gated)

- [ ] Keep private `BrokenPorkMachine/warhead` untouched and never auto-import it.
- [ ] Treat `feature/warhead-community-guardrails` / draft PR #3 as a separate
      reviewable track, not a dependency for the non-agent W1/W5 release.
- [ ] Before claiming a Community edition is safe: audit the bundled code and
      history, settle licensing, generate Community-only artifacts, confine actual
      processes at OS level, and independently verify every execution entrypoint.
- [ ] With the agent disabled, all mandatory W1/W5 features still work.
- [ ] When enabled, findings cite exact HindSight evidence and require permission
      for every mutating operation; never trust model verification claims.

Gate: an explicit independent security/release review. A JavaScript allowlist
alone is not a sandbox.

## Priority and sequencing

| Sequence | Milestone | Entry/exit condition |
| --- | --- | --- |
| C0 | Baseline and contradictions | Clean version/schema/engine/platform documentation; run existing source gates; publish failing cases |
| C1 | Golden investigation path | W1 fully repeatable with export/reopen and negative cases |
| C2 | UX and recoverability | W4 and W5 pass; zero P0/P1 data-loss, hang or mislabeling defects |
| C3 | Evidence quality | W2 corpus and source-backed provenance gates pass |
| C4 | Controlled runtime | W3 real ARM64 host plus deterministic lab gates pass |
| C5 | Candidate packaging | Clean checkout Debug/Release on Apple Silicon, reproducible fixture results, signed/notarized only when credentials and host qualification exist |
| C6 | Optional agent release | W6 independently passes; never silently bundled into a cleared release |

Do not increment the shipping identity to 0.6.0 until relevant feature and
compatibility changes actually land, the gates are attached, and release notes
accurately describe the state. Design docs alone are not a version upgrade.

## Completion evidence

For each gate keep a small record with:
`scenario_id`, host OS/arch/RAM, Git SHA, app/worker/engine/schema versions,
fixture SHA-256, exact command/actions, exit or UI outcome, observed result,
expected result, logs/artifact links with credentials redacted, assessor, and
status **PASS / FAIL / BLOCKED / NOT RUN**. Never convert NOT RUN into PASS.

## Boundaries

- Official host is Apple Silicon macOS 14+; Intel is experimental.
- Linux workers may be qualified separately, but Linux is not the native app release.
- Model-backed Warhead is optional; offline deterministic analysis must continue.
- Patching remains staged, reversible and explicitly approved; no target flashing.
- Source authenticity and deterministic reproduction outrank visual plausibility.

## First work increment

1. Align code/document release identities and add an automated drift check.
2. Run `python3 scripts/check-product-consistency.py`, and add it to the
   mandatory source validation gate.
3. On the supported Mac, run `./scripts/test-macos.sh` and
   `./scripts/qualify-release.sh --candidate`. Preserve logs.
4. Reproduce W1 with the bundled fixture and record a bug for each missing step.
5. Fix blockers in dependency order, **then** improve layout, then revisit advanced
   reconstruction gaps. No speculative feature proliferation.

This plan supplements the single authoritative
`docs/MASTER_ROADMAP.md` rather than creating a competing research backlog.
