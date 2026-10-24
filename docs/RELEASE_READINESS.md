# HindSight 0.5.14 — Release readiness

HindSight 0.5.14 officially supports macOS on Apple Silicon (`arm64`). Intel (`x86_64`) remains experimental and is not part of the official release gate. Credentialed distribution and manual host qualification are intentionally deferred until further notice; this document retains the future release procedure.

## Source invariants already enforced

- Product/service/worker identity is exactly `0.5.14`; release channel is `stable`; bundle build is `5914`.
- App and Coordinator require the same version, release channel, ABI, and required capabilities before project actions are enabled.
- Dense workspaces use adaptive wide/medium/compact topology rather than fixed horizontal packing; compact action groups wrap/restack and Targets no longer uses fixed metadata columns inside a collapsible pane.
- Large-CFG dominator analysis is per-function and budgeted; worker termination is isolated and reported.
- Analysis Quality distinguishes coverage from negative evidence; candidate control-flow remains labelled candidate evidence.
- Project-local cached analysis is SHA-256- and analysis-engine-bound using envelope schema 3, bounded to the analysis transport ceiling, atomically published, symlink-rejected, and decoded through the same report limits before reuse. Schema-2 cache reads remain supported; engine identity `arm64-analysis-v104-schema130` prevents reuse of results from earlier semantic lifts.
- Existing schema-v1 projects require no database migration for the cache feature.
- Worker progress records must remain separate from framed stdout and real stderr diagnostics; final evidence publication remains atomic.
- App and Coordinator macOS Swift type-check source sets are derived directly from `Config/project-topology.json`; `AnalysisNavigation.swift`, `AnalysisContinuity.swift`, and future generated-target Swift files cannot be omitted by stale hand-maintained shell lists.
- Project `objects`, `staging`, and `analysis-cache` path components must be real non-symlink directories; staged capture and existing cache destinations must be regular non-symlink files before publication/admission.
- Function Neighborhoods presentation is incrementally revealed (200 interactive rows per batch) and worker-side detailed neighborhood materialization is bounded while complete connectivity counts remain available.

- The 0.5.9 Evidence Core builds a maximum 50,000-record typed index off the SwiftUI render path; Search/Triage/Related Evidence must consume that index rather than reparsing report strings.
- Resident full compatibility reports are bounded to an 8-entry LRU; the selected artifact owns at most one typed evidence index.
- Launch-session persistence is coalesced with a 350 ms debounce.
- The optional REA bridge has a deterministic MCP fixture contract covering handshake, capability gating, progress retention, and fail-closed unavailable-tool behavior; real provider qualification remains manual and local-first.

## Deferred Apple-Silicon macOS release-host gate

Credentialed signing, notarization, Gatekeeper assessment, real-target qualification, memory-pressure qualification, and crash-diagnostics qualification are intentionally deferred until further notice. The procedures below remain retained for a future release decision and are not current completion blockers.

Before supplying credentials, run the non-secret preflight to identify missing
toolchain, keychain, notarization-profile, or universal-bundle prerequisites:

```bash
./scripts/release-preflight.sh
```

Run:

```bash
VECTORGLASS_SIGN_IDENTITY="Developer ID Application: …" \\
VECTORGLASS_DEVELOPMENT_TEAM="TEAMID" \\
VECTORGLASS_NOTARIZE_PROFILE="notary-profile" \\
./scripts/test-release-macos.sh
```

This gate must complete Debug/Release builds, embedded worker/VectorEmu checks, code-sign verification, notarization, stapling, and stable bundle identity verification.

For a single repeatable operator command that runs the non-secret preflight, invokes
the candidate or stable gate, captures a timestamped transcript, and writes the
remaining manual checklist, use:

```bash
./scripts/qualify-release.sh --candidate
```

After Developer ID and notarization credentials are configured, run the stable path:

```bash
VECTORGLASS_SIGN_IDENTITY="Developer ID Application: …" \
VECTORGLASS_DEVELOPMENT_TEAM="TEAMID" \
VECTORGLASS_NOTARIZE_PROFILE="notary-profile" \
./scripts/qualify-release.sh --stable --open
```

Reports are written below `build/release-reports/<timestamp>-<mode>/`. The command
does not claim the real-target or memory-pressure matrix passed; it creates a
checklist for those host-specific steps.

For supplied IPSW/IMG3/IMG4/SEP fixtures, run the worker-backed container gate and
retain its JSON report with the release evidence:

```bash
python3 scripts/qualify-firmware-fixtures.py \
  /absolute/path/to/reworker \
  /absolute/path/to/firmware-fixtures \
  --report build/release-reports/firmware-qualification.json
```

The gate verifies bounded capture, source SHA-256 preservation, container structure,
and worker status for every fixture. It does not promote an unsigned or encrypted
artifact to authenticated plaintext without the required Apple credentials.
For legacy ZipCrypto fixtures only, provide `HINDSIGHT_ZIP_PASSWORD` in the worker
environment; passwords are never written to the report. AES-encrypted ZIP fixtures
remain a deliberate failure until a vetted AES implementation and key-handling
contract are supplied.

## Manual distribution matrix

Before distributing 0.5.14, complete the official matrix on an Apple-Silicon Mac using the 8 GiB baseline:

1. Re-run the large firmware image that exposed the beta.12 pressure failure; record peak RSS and confirm bounded dominator behavior.
2. Analyze several real arm64 Mach-O inputs: stripped, symbol-rich, data-in-code, and encrypted-metadata cases.
3. Open a project containing a valid 0.4.1 cache and confirm schema-1 migration; repeat and confirm schema-2 reuse. Then reuse a valid 0.4.2 schema-2 cache and confirm the unchanged engine identity avoids recomputation. Modify/replace source identity and confirm the cache is not reused.
4. Run a fresh large analysis and confirm stage percentages are monotonic, cancellation reports the last stage, and `VG_PROGRESS` records never appear in normal worker failure diagnostics.
5. Exercise Binary Diff across two artifacts with cached and uncached sides.
6. Exercise the responsive UX at 900×680, 1180×760, 1440×900, and full-screen. Verify Targets, VectorEmu, IPSW Catalog, Binary Diff, Trace Explorer, Runtime Correlation, and the global inspector at every size. On Intel, verify closed-out Debugger/Hypervisor/SoC surfaces show the host-requirement view with no overlapping or active execution controls; on Apple Silicon, also exercise those live surfaces.
7. Exercise VectorEmu start/step/run/reset/trace/breakpoint flows on both host architectures.
8. Kill/restart Coordinator with an open project; verify project rehydration and project-action gating.
9. Quit/relaunch with a saved project/workspace/layout; verify continuity and stale-path fallback.
10. Verify no P0/P1 defects remain and that the final `.app` passes Gatekeeper assessment after notarization.
11. Run the topology-derived app/service type-check gates and verify `AnalysisNavigation.swift`, `AnalysisContinuity.swift`, and `HostPlatform.swift` are included in the generated target source sets; then complete clean Xcode Debug and Release builds.
12. Open Function Neighborhoods on the firmware-scale stress artifact that previously caused a hang; confirm first paint remains responsive, evidence expands only in bounded batches, Code/CFG pivots work, and memory stays bounded enough for the 8 GiB baseline.
13. On a disposable project copy, substitute symbolic links for `objects`, `staging`, and `analysis-cache`, plus a staged capture/cache file. Each case must fail closed without writing through the link or special file.
14. Inspect `HindSight`, `CoordinatorService`, `reworker`, and `vg-emu-server` with `lipo -archs`; every distributed executable must contain both `arm64` and `x86_64`.
15. If REA is enabled for the distribution, run a local provider qualification against an approved target and confirm `binary_session` availability, progress, cancellation, bounded response retention, structured Evidence identity, and `.vgcase` restore. The automated deterministic fixture runs during release preflight.

### Experimental Intel qualification (optional)

16. If an experimental Intel build is produced, confirm ARM64/ARM64e static analysis, projects, Binary Diff, evidence/review workflows, scripting/plugins, firmware tooling, and VectorEmu work in the x86_64 slice. Failures do not block the official release.
17. If an experimental Intel build is produced, confirm Debugger, Hypervisor RE, and SoC Virtualization are visibly closed out with explicit Apple-Silicon requirements from every navigation route. Direct Coordinator calls must fail closed.

### Official Apple-Silicon qualification

18. Exercise authorized native debugger attach/launch/step/breakpoint/memory/module/backtrace against a local ARM64 target.
19. Exercise Hypervisor/SoC execution create/destroy, RAM access, vCPU registers/run exits, reset, trace, breakpoints, and firmware-load paths.

Until that matrix is completed on both release hosts, treat the source archive as a release-candidate source package even though its internal channel is `stable`.

## 0.4.3 analysis-navigation qualification

On the release host, reuse a valid 0.4.2 schema-2 analysis cache and confirm 0.4.3 does not rerun the worker when source SHA-256 and `arm64-analysis-v104-schema130` still match. Exercise Analysis Triage and Navigator with address-bearing, symbol-bearing, and text-only rows; verify Source, Code, CFG, and Symbol routing never invents a destination when the required identity is absent. Select an address/symbol row and verify Related Evidence is bounded and source-labelled.

## 0.4.4 investigation-continuity qualification

On the macOS release host, open an existing 0.4.3 project and confirm its schema-1 launch record still restores. Pin evidence, pivot several findings through Source/Code/CFG/Symbol, quit/relaunch, and confirm project-local pins/recents survive without copying note contents. Re-run analysis from a valid schema-2 cache and confirm the Changes view records an unchanged channel comparison. Verify `.vectorglass/analysis-continuity-v1.json` remains below 1 MiB and that replacing the state file with a symlink causes continuity persistence to fail closed without blocking ordinary project analysis.

## 0.4.9 review-action qualification

Open a project last used by 0.4.8 and confirm continuity schemas 1–4 migrate to schema 5 without losing pins, recent pivots, collections, review baselines, or journal entries. Create several follow-ups in one collection, including items with and without context notes; confirm new tasks capture no more than 16 exact collection evidence identities and that completing/reopening an item preserves its creation identity while updating completion state. Unpin evidence used by a follow-up and confirm the task remains present as workflow context. Delete the collection and confirm its scoped follow-ups are removed.

Quit/relaunch and verify the queue persists. Generate a handoff and confirm open follow-ups appear as unchecked actions, completed actions are summarized, and the footer states that follow-ups are analyst-authored tasks rather than findings. Exercise the Review surface at 900×680 and 1180×760; the Follow-Up Queue must wrap/restack without overlapping the journal or pairwise comparison controls. Replace `.vectorglass/analysis-continuity-v1.json` with a symlink/special file and confirm persistence still fails closed without blocking ordinary analysis.

