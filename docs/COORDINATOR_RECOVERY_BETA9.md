# Coordinator recovery — HindSight 0.4.0-beta.9

## Problem closed by beta.9

CoordinatorService owns the live `ProjectStore`. When the XPC service process is interrupted or relaunched, the SwiftUI app can still hold a perfectly valid `ProjectSnapshot`, but the new service process starts with no project open. In beta.8, a successful capabilities probe could therefore make the UI look online while subsequent project requests failed with “Create or open a project first.”

Beta.9 makes that state impossible to represent as ready.

## State model

The workbench now tracks two independent facts:

- **Coordinator reachable** — an XPC request completed and the capability handshake is compatible with the app.
- **Project session ready** — either no project is open in the app, or the Coordinator has successfully opened the same project currently displayed by the app.

Project-bound actions use the conjunction `connected && projectSessionReady`. Import additionally requires a project and no active operation.

Visible states are:

1. **Connecting** — establishing XPC and validating the capability handshake.
2. **Restoring project** — Coordinator is reachable; the displayed project is being reopened in the service.
3. **Coordinator online** — handshake is valid and the required project session is ready.
4. **Project session unavailable** — Coordinator is reachable but reopening the displayed project failed.
5. **Coordinator offline** — transport/handshake is unavailable.

## Handshake requirements

The capabilities payload must decode as JSON and match:

- `version == VGProduct.version`
- `release_channel == VGProduct.releaseChannel`
- `abi == 1`
- `rust_bridge == true`
- `analysis == true`
- `execution == true`

This catches partial app/service upgrades and bridge initialization responses before project work is attempted.

## Reconnect behavior

**Reconnect Coordinator** explicitly invalidates the old connection generation, cancels any pending deadlines/callbacks on that generation, and creates a new XPC connection. After the handshake succeeds, an existing project is automatically reopened with `create:false`.

A successful rehydration refreshes the project snapshot but does not clear unsaved note drafts. The current selection is preserved when its record still exists; otherwise normal snapshot application selects a valid record. The Investigation Ledger is not reloaded merely because the same project was rehydrated.

If restore fails, the app retains the client-side project snapshot for context but disables project-bound service actions and offers **Restore Project**. The user can also open a different project from the app menu once the Coordinator itself is reachable.

## Diagnostics

The Support menu includes **Copy Diagnostics**. The report includes product version/channel, OS, physical memory, processor count, Coordinator state/version/ABI, project-session state, project basename, artifact count, selected artifact, workspace, active analysis phase, debugger/VectorEmu flags, and the current status message. The full project path is intentionally excluded.

## Qualification smoke test

On the Apple Silicon release host:

1. Open a project with at least one captured artifact.
2. Select a non-default artifact and create an unsaved note draft.
3. Terminate CoordinatorService while the app remains open.
4. Confirm project-bound controls disable and the app reports the disconnect.
5. Reconnect. Confirm the status passes through Connecting → Restoring project → Coordinator online.
6. Confirm the same project and selected artifact are restored and the unsaved draft is still present.
7. Repeat with the project folder temporarily unavailable and confirm the UI reports Project session unavailable and does not enable Import/Analyze/Page/Save Note.
8. Restore filesystem access and use Restore Project; confirm actions become available only after the reopen succeeds.
