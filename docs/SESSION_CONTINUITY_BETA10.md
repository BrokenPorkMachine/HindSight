# Launch/session continuity — HindSight 0.4.0-beta.10

## Purpose

Beta.9 could recover an already-running app after CoordinatorService restarted, but quitting HindSight still lost the open-project/workbench context. Beta.10 extends recovery across a full app relaunch while keeping CoordinatorService/project storage authoritative.

## Persisted metadata

HindSight stores one bounded launch-session record in its application preferences. It contains only:

- schema version (`1`)
- absolute project path (maximum 4096 UTF-8 bytes)
- optional selected project-record ID (maximum 256 UTF-8 bytes)
- workspace name (must exist in `VGWorkspaceCatalog`)
- layout preset (must exist in `VGWorkspaceCatalog.layouts`)
- update timestamp

The encoded record is capped at **32 KiB**. Invalid, oversized, malformed, or catalog-incompatible records are discarded.

## Recovery order

1. HindSight starts with no project trusted in memory.
2. The app connects to CoordinatorService and validates exact product version, release channel, ABI, Rust bridge, analysis, and execution capabilities.
3. If a valid saved launch session exists, the app asks CoordinatorService to `openProject(..., create:false)`.
4. Only a successful project reply becomes the active snapshot.
5. The selected record, workspace, and layout are then restored if they still exist.
6. Project-bound actions remain gated by `projectServiceReady` throughout recovery.

A saved path that no longer exists is cleared automatically and the normal create/open workflow becomes available. A path that exists but is rejected by CoordinatorService remains visible as a recoverable failure so the user can retry or choose **Support → Forget Saved Project**.

## Privacy boundary

The launch-session record does **not** contain unsaved analyst-note text, analysis reports, debugger registers/memory, runtime observations, Python source/output, firmware paths, or machine state. Beta.9 still preserves unsaved note drafts across a Coordinator restart because the app process remains alive; beta.10 does not claim draft persistence across app termination.

Diagnostics expose only whether a saved launch session exists and its project **basename**, never the full saved project path.

## User controls

- **Support → Reopen Last Project** retries the saved project when no project is currently open.
- **Support → Forget Saved Project** removes the saved launch record when no project is open.
- The first-run/empty-investigation surface offers **Reopen Last Project** when a recoverable entry exists.

## Non-claims

This is workbench continuity, not deterministic runtime replay. Native debugger state, Hypervisor guest state, VectorEmu state, long-running jobs, and unsaved note content are not resumed across full app termination in beta.10.
