# HindSight enterprise readiness

This roadmap defines enterprise-grade polish as trust, operability, governance,
reproducibility, and deployment safety around the existing local-first analysis
engine. It does not imply that a local macOS application can provide SSO, fleet
management, or shared collaboration without an enterprise service boundary.

## Release principles

- **Evidence remains local-first.** Target files and analysis contents are not
  sent anywhere unless an analyst explicitly exports or shares a case.
- **Every conclusion is inspectable.** Observations, derived analysis,
  correlations, and analyst assertions remain distinct in the case layer.
- **Failure is supportable.** Errors identify the failed boundary, preserve
  diagnostics, and provide a bounded recovery action.
- **Policy is explicit.** Host capability, extension trust, runtime permission,
  and release-channel boundaries are visible before an operation is attempted.
- **Automation is reproducible.** Builds, qualification, case exports, and
  support reports have stable formats and machine-readable summaries.

## Workstreams

### 1. Trust and supportability

- [x] Surface product version, channel, host capability, coordinator health, and
  diagnostics actions in Settings.
- [ ] Add structured operation/error taxonomy with recovery guidance.
- [x] Add support-safe diagnostics export with explicit user-selected destination.
- [ ] Add full support-bundle export with redaction preview and explicit consent.
- [x] Add a backward-compatible SHA-256 case integrity manifest and verify it on import.
- [ ] Add startup, memory-pressure, crash, and worker-health history to support
  diagnostics without collecting target-file contents.

### 2. Governance and security boundaries

- [ ] Add a local policy document with schema/version, read-only policy view,
  and fail-closed handling for malformed policy.
- [ ] Add extension allowlist, signature/trust status, capability disclosure,
  and per-extension enable/disable controls.
- [ ] Add explicit data-retention and export destination controls.
- [x] Record extension trust and enable/disable decisions in the durable case ledger.
- [ ] Add an append-only, exportable audit stream for all analyst and policy events.

### 3. Team investigation workflow

- [x] Add explicit Approved/Rejected states to the append-only review journal and
  case exports.
- [ ] Add review assignment and multi-user authorization to review decisions.
- [ ] Add conflict-safe case merge/diff for independently authored evidence.
- [ ] Add deterministic case manifest/version compatibility checks.
- [ ] Add a documented isolated/on-prem handoff boundary for organizations that
  cannot use a hosted service.

### 4. Deployment and operations

- [x] Automate universal candidate/stable qualification and report capture.
- [ ] Complete Developer ID/notarization and dual-host qualification.
- [ ] Add managed-defaults documentation and deployment verification.
- [ ] Add opt-in crash diagnostics and a support-safe redaction contract.
- [ ] Add signed release manifest/checksum verification to the distribution flow.

## Current slice acceptance

An operator can open Settings and answer: which HindSight build is running, what
host capabilities are available, whether the Coordinator is healthy, what project
is active, and how to copy diagnostics for support. The surface must not expose
target contents or imply that an unsupported native backend is available.
