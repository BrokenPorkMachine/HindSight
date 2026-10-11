# HindSight 0.4.0-beta.14 — Beta readiness

> Historical record: 0.4.2 release qualification is tracked in [`RELEASE_READINESS.md`](RELEASE_READINESS.md).

HindSight is promoted from alpha to **beta** only because the source tree now treats expected failure and recovery as product behavior rather than development accidents. This is still pre-release software.

## Alpha-exit work completed

- CoordinatorService no longer contains a production `fatalError`; failure to initialize the Rust admission governor degrades analysis requests with an actionable error instead of crashing the service. Heavy Rust workers are also serialized across independently forced app instances through a per-user lock.
- Project open/create performs SQLite `quick_check` before accepting writes and removes only UUID-shaped abandoned `.partial` staging files left by an interrupted capture.
- The Investigation Ledger maintains a last-known-good backup. A corrupt primary ledger can be recovered without silently discarding case history, and the UI reports the recovery.
- Hypervisor/SoC profiles with incomplete device models are explicitly marked **Experimental** in the service model and UI. Experimental models are not represented as beta-qualified hardware equivalence.
- Debug and Release macOS builds are separate qualification gates. The beta gate verifies signatures and the embedded Rust worker in both development and Release packaging paths.
- Apple bundle versioning is now separated from product SemVer: `CFBundleShortVersionString=0.4.0`, numeric build `4014`, product/protocol version `0.4.0-beta.14`.
- The source validator rejects production Swift crash primitives (`fatalError`, `preconditionFailure`, `assertionFailure`, `try!`, forced casts) outside Tests and enforces the beta recovery/experimental-label contracts.
- The published capability schema is generated from the bridge capability payload and must match it exactly; stale or contradictory API metadata is a release failure.
- VectorEmu declares Rust **1.90** as its MSRV and the repository pins Rust 1.90.0 for qualification. The beta.2 source gate rejects post-1.90 integer APIs that previously escaped into `iphem-core`.
- Beta.4 adds full-app and full-CoordinatorService Swift type-check gates, cross-target XPC bundle-layout verification, symlink-resistant project/ledger/plugin boundaries, bounded plugin installation, transactional captured-object rollback, and continuously drained helper-process output.
- The notarization path now requires an explicit Developer ID Application identity and development team, builds with hardened runtime, verifies the signed app identity, then submits and staples the result.
- Beta.6 makes analysis lifecycle state observable through a bounded 10-phase status contract, extends only the analysis deadline to 240 seconds, and makes status-poll timeouts non-fatal to the primary XPC generation.
- Beta.8 hardens the first-run and recovery UX: CoordinatorService is probed on presentation, offline/interrupted state has an explicit reconnect path, Import Binary is enabled only when its model prerequisites are satisfied, and bounded sidebar/search surfaces disclose empty/overflow states rather than failing silently.
- Beta.9 hardens service restart recovery: the app validates Coordinator version/channel/ABI/capabilities, establishes a fresh XPC generation on reconnect, reopens the displayed project before project actions become available, and exposes restore failure separately from transport availability.
- Beta.10 extends continuity across full app relaunches with bounded validated launch metadata, automatic last-project reopen, stale-session fallback, and explicit reopen/forget controls without persisting unsaved note contents.
- Beta.11 re-architects dense workbench composition around adaptive wide/medium/compact topology, responsive global inspector placement, semantic multi-row toolbars, and overflow-safe status/metric bands so capabilities remain reachable at practical window sizes.
- Beta.12 adds evidence-synthesis surfaces; beta.13 bounds dominator memory per function and distinguishes worker signal failure; beta.14 adds an analysis-quality ledger, one-hop function neighborhoods, conservative exact-entry tail-call candidates, and external direct-BL accounting.

## What beta means here

Beta means the advertised stable workbench paths are feature-complete enough for broader real-world testing and have bounded failure/recovery behavior. It does **not** mean every research backend is stable. Experimental machine profiles remain clearly labelled and may be incomplete.

## RC exit gates

The following are intentionally **not claimed as completed by this package**, because they require the Apple Silicon release host and real targets:

1. `./scripts/test-beta-macos.sh` passes from a clean checkout with both Debug and Release builds.
2. Static-analysis smoke matrix on multiple real arm64 Mach-O binaries, including stripped and symbol-rich pairs.
3. Native debugger attach/launch/step/breakpoint/memory/module/backtrace matrix on an authorized local target.
4. VectorEmu start/step/run/reset/trace/breakpoint test against a legally obtained research image.
5. Hypervisor create/map/read/write/reset/exit test on supported Apple Silicon hardware.
6. Binary Diff smoke test across real version/layout churn, plus a large-function-set responsiveness run.
7. Project crash-recovery test: interrupted capture leaves no live staging object after reopen; ledger primary corruption recovers from the backup.
8. Memory-pressure run on the 8 GiB Apple Silicon baseline while analysis, graph, diff, and trace surfaces are exercised.
9. Release signing verification succeeds. Notarization is submitted and accepted for an RC candidate.
10. Coordinator restart smoke test: with an open project and unsaved note draft, terminate/restart the XPC service; confirm the app reconnects, reopens the project, preserves the draft/selection, and does not enable project-bound actions before restore completes.
11. Full-app relaunch smoke test: open a project, select a non-default artifact/workspace/layout, quit and relaunch HindSight, and confirm the project/context restore only after a valid Coordinator handshake; repeat with the project moved/deleted and confirm clean fallback.
12. Responsive UX matrix: exercise the migrated dense surfaces at 900 × 680, 1180 × 760, 1440 × 900, and a large/full-screen width; confirm no primary action, pane selector, status, or evidence surface becomes unreachable.
13. Analysis-quality smoke: run the same large firmware that exposed beta.12 pressure; verify Quality reports bounded coverage, no external call is attributed as an internal incoming edge, and tail-call candidates remain explicitly labelled candidates.
14. No new P0/P1 defects from the beta test matrix.

Until those pass, the product should remain beta rather than RC.
