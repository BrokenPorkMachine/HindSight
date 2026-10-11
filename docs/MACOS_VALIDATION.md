# Required macOS checks — HindSight 0.5.0

For ordinary development validation run:

```bash
./scripts/doctor.sh
./scripts/test-macos.sh
```

For an ordinary candidate build, run the stronger package gate:

```bash
./scripts/test-beta-macos.sh
```

That gate repeats the development matrix and also builds/verifies the Release app and its embedded worker. For a distributable 0.5.0 build, run `./scripts/test-release-macos.sh` instead; it requires Developer ID signing credentials and a notarization profile, and validates the stapled app plus the stable `0.5.0` / build `5000` identity.

Confirm the worker self-test reports `0.5.0`. The native Hypervisor shim must pass both arm64 implementation and x86_64 fallback syntax/type checks against the installed macOS SDK before the full Xcode build begins. App and Coordinator Swift type-checks run for both macOS architectures.


## Intel host smoke

On an Intel Mac, verify the app launches natively as `x86_64`; ARM64 static analysis and VectorEmu remain operational; Native ARM64 Debugger, Hypervisor RE, and SoC Virtualization controls are unavailable with explicit host-requirement text. The Coordinator must reject direct calls to those Apple-Silicon-only backends as well.

Use `lipo -archs` on the app executable, Coordinator service, `reworker`, and `vg-emu-server`; each Release artifact must contain `arm64 x86_64`.

## 0.4.9 hotfix/audit-closure smoke

Run `./scripts/test-app-typecheck.sh` and confirm the reported topology-derived app source count includes both `App/AnalysisNavigation.swift` and `App/AnalysisContinuity.swift`. Run `./scripts/test-coordinator-typecheck.sh` and confirm its source list is likewise derived from `Config/project-topology.json`. These are release gates: adding a Swift file to a generated target without type-check coverage is a failure.

Build and launch the app, then open **Function Neighborhoods** on the same firmware-scale artifact that previously wedged the UI. Initial presentation must remain responsive, show no more than the first 200 interactive evidence rows, and expose additional detail only through explicit 200-row batches. Scroll, select evidence, open context menus, and pivot to Code/CFG while monitoring memory pressure; the worker report must retain complete connectivity counts while detailed per-function neighborhoods remain bounded.

On a disposable copy of a project, replace each of `objects`, `staging`, and `analysis-cache` in turn with a symbolic link to a temporary directory outside the project. Reopen/import/analyze as appropriate and verify HindSight fails closed with a precise storage-boundary error and does not create or replace files in the linked destination. Restore real directories between cases. Also replace an existing analysis-cache JSON path and a staged capture path with symlinks/special files and confirm publication/admission is rejected.

On both Intel and Apple Silicon, launch HindSight and import two captured arm64 builds. Run **Binary Diff → Analyze Pair** and verify changed/added/removed functions, side-by-side disassembly, symbol/fingerprint match evidence, and architecture mismatch refusal.

### Apple-Silicon-only dynamic execution

On Apple Silicon only, validate against a locally built arm64 target with development debugging permissions. Verify launch/attach; thread selection; register refresh; step/continue/interrupt; stop reason and PC; Follow PC; PC → Annotated Code; PC → CFG; bounded memory reads; BRK set/remove; VM map; loaded images with plausible preferred `__TEXT` and slide; module-aware backtrace; live disassembly; exit/detach.

For a Hypervisor guest, verify create/destroy; guest RAM map/read/write; PC set/read; active stack-pointer inspection; run/exit reporting; and reset. Apple-silicon VM creation must use the config-pointer API (`hv_vm_create(NULL)` for the default configuration), and stack-pointer access must use the system-register API (`SP_EL0` / `SP_EL1`).

Protected system/third-party processes may be rejected by macOS security policy and must surface a precise error. Hardware debug-register breakpoints/watchpoints are not claimed in this release.


## v0.4.9 review actions & follow-up smoke

Open a 0.4.8 project with existing collection review state. Confirm the continuity document migrates to schema 5 in place and that existing journal entries and review baselines remain visible. Add follow-ups from a collection with more than 16 pins and confirm each task reports at most 16 bounded evidence identities. Complete and reopen tasks, relaunch HindSight, and verify status persists. Unpin source evidence and confirm the task is retained; delete the collection and confirm collection-scoped tasks are removed.

Generate an Investigation Handoff and verify unresolved tasks appear under **Follow-Up Queue** while completed tasks are summarized rather than presented as evidence. At 900×680 and 1180×760, verify the queue, review journal, and comparison controls remain reachable with no overlap.


## v0.4.2 analysis execution/cache smoke

Analyze a captured artifact once and confirm a normal worker analysis completes. Run Analyze again without changing the project or captured object; the second result must report **Cached analysis reused** and must preserve the same source SHA-256 while using the new request UUID. Confirm an `analysis-cache/<sha256>.json` file exists inside the project and remains bounded.

Then move the project to another path and reopen it: cache reuse should still work because identity is content-based, not absolute-path-based. Replace/import a different captured object and confirm its analysis does not reuse the old digest cache. A malformed, oversized, symlinked, or wrong-analysis-engine cache file must be ignored or rejected and ordinary worker analysis must remain available.


## v0.4.0-beta.14 analysis synthesis smoke

Build the app first to confirm the VectorEmu **Run steps** control compiles with the `UInt64` model value and an `Int` Stepper stride. Analyze at least one symbol-rich ARM64 Mach-O and one stripped/low-symbol fixture. Confirm **Analysis Insights**, **Function Intelligence**, and **Data References** populate without widening the responsive workbench beyond its beta.11 minimum.

For Function Intelligence, manually verify several rows against the direct call graph and CFG. The displayed attention score must match its published formula and must not be labeled as severity, vulnerability, reachability, or semantic importance. For Data References, spot-check ADR and adjacent ADRP+ADD sites against disassembly/section mappings. Every emitted target must fall inside a parsed Mach-O section; string annotations must point into a validated string-bearing section. Non-adjacent ADRP chains must not be promoted.

## Analysis observability smoke

Run analysis on a captured arm64 Mach-O large enough to remain active for several seconds. Confirm the status bar advances through real bounded phases (at minimum launching/running and a terminal outcome), elapsed time increases, Cancel transitions to cancellation without publishing a late report, and a delayed/failed status poll does not disconnect CoordinatorService or abort the primary analysis request.

## Modern Mach-O admission smoke test

Use at least one ordinary Apple-silicon executable that contains `LC_FUNCTION_STARTS` and run **Analyze**. It must progress beyond capture/preview into the analysis lifecycle; the presence of `LC_FUNCTION_STARTS` or `LC_DATA_IN_CODE` must not produce an unsupported-preview refusal. If `LC_DATA_IN_CODE` is present, declared data words must appear as data barriers rather than decoded instructions. For a fixture with nonzero `cryptid`, verify that capture succeeds with `metadata-only`, full analysis returns bounded metadata/linkage evidence, and instruction-page requests fail explicitly rather than disassembling ciphertext.


## v0.4.0-beta.10 full-app continuity smoke

Open a real project, select a non-first artifact, switch to a non-default workspace/layout, quit HindSight completely, and relaunch it. The app must first validate the Coordinator capability handshake, then reopen the prior project, then restore the selected artifact/workspace/layout. Project-bound actions must remain unavailable during the reopen window.

Repeat after moving or deleting the saved project folder. HindSight must clear a missing saved path and return to the ordinary create/open flow without reconnect loops or a permanently unavailable project session. For a project that still exists but fails Coordinator validation, the saved entry may remain retryable and **Support → Forget Saved Project** must dismiss it. Confirm diagnostics contain only the saved project basename, never its full path. Unsaved note text is not part of full-app continuity and must not appear in preferences/diagnostics.


## v0.4.0-beta.11 responsive-workbench smoke

Exercise Binary Workspace, CFG, Trace Explorer, Runtime Correlation, Evidence Graph, Investigation Ledger, Binary Diff, Debugger, Targets, and VectorEmu at **900 × 680**, **1180 × 760**, **1440 × 900**, and a large/full-screen width. At each size, verify that the primary canvas remains usable, auxiliary navigator/inspector regions change topology rather than collapsing below useful width, toolbar actions remain reachable, and metric/status bands do not force the canvas wider.

At medium widths, the three-pane workspaces must expose navigator/inspector content through the single auxiliary dock. At compact workspace widths, that auxiliary dock must move below the primary canvas. The global contextual inspector must similarly move below the workspace once the detail region is narrower than its trailing-inspector threshold. Focus Mode must still restore the prior inspector visibility when exited.

Confirm a fresh analysis shows monotonic stage/percentage progress, cancel during a mid-pipeline stage, and verify the cancellation status retains the last stage without `VG_PROGRESS` text leaking into diagnostics. Confirm a valid 0.4.1 cache is reused/migrated once and subsequent runs use schema 2.

## 0.5.0 Evidence Core smoke

Analyze the firmware-scale Function Neighborhoods fixture, then repeatedly switch among Analysis Triage, Search, Related Evidence, Collections and Function Neighborhoods while typing/changing the search field. Confirm the UI remains responsive while the initial typed index is constructed, shows an explicit indexing state, and does not reintroduce the previous main-thread hang. Inspect memory after analyzing more than eight distinct artifacts and confirm resident full reports remain bounded.
