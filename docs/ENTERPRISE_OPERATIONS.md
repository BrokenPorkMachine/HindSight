# Enterprise Operations Contract

This document records the local deployment, release-trust, retention, and diagnostics boundaries for HindSight 0.5.7. It is an operator contract, not a claim that Developer ID signing, notarization, or managed-device enrollment has been completed in this source environment.

## Managed defaults

The application preference domain is `ca.zensoftware.VectorGlass`. A managed deployment may provide these keys through macOS configuration profiles:

| Key | Type | Meaning | Safe default |
| --- | --- | --- | --- |
| `HindSight.preferred-layout.v1` | string | Initial layout preset | `Explore` |
| `HindSight.enterprise.read-only.v1` | boolean | Reserved deployment control; unsupported values must be ignored | absent/false |
| `HindSight.extensions.mode.v1` | string | Extension policy control: `allow`, `verified-only`, or `disabled` | `allow` |
| `HindSight.diagnostics.opt-in.v1` | boolean | Permits future automated diagnostic collection; it does not upload data by itself | false |

Unknown keys, malformed values, and unsupported policy values fail closed to the local default. The Coordinator enforces `HindSight.extensions.mode.v1`: `disabled` blocks every extension, while `verified-only` permits only extensions with a valid Ed25519 package signature. A managed preference must never silently grant an extension capability, debugger entitlement, target access, or external network authority. The Extension Manager exposes the resulting local policy status for each discovered extension.

The current application persists startup/layout state in bounded `UserDefaults` records and does not transmit preferences or diagnostics. `HindSight.preferred-layout.v1` is validated against the finite workspace-layout list before use; launch-session data is bounded to 32 KiB and recent projects to eight entries.

## Signed release manifest

Stable distribution requires a detached release manifest next to the notarized artifact. The manifest should contain, at minimum:

```json
{
  "schema": 1,
  "product": "HindSight",
  "version": "0.5.7",
  "build": "5700",
  "release_channel": "stable",
  "artifacts": [{"name": "HindSight-0.5.7.dmg", "sha256": "…", "bytes": 0}],
  "minimum_macos": "14.0",
  "required_architectures": ["arm64", "x86_64"],
  "analysis_engine": "arm64-analysis-v15-schema71",
  "protocol_abi": 1,
  "signature": {"algorithm": "Ed25519", "key_id": "…", "value_base64": "…"}
}
```

Release operators must verify the manifest signature, exact product/version/build/channel, artifact digest and size, minimum OS, both universal slices, analysis engine, and protocol ABI before publishing. A missing, malformed, mismatched, or unverifiable manifest is a release failure; it must not be downgraded to a warning. The existing `scripts/test-release-macos.sh` remains the credentialed build/signing/notarization gate, while `docs/RELEASE_READINESS.md` remains the manual real-host matrix.

## Retention and locality

HindSight is local-first. There is no background crash upload, analytics stream, or automatic case transmission.

- Operation errors retain at most 64 bounded records in memory; diagnostic export includes at most the newest 32.
- Worker/request status retains only the bounded recent terminal/request window defined by the Coordinator protocol.
- Analysis continuity is project-local and capped at 1 MiB; pins, recents, collections, review entries, follow-ups, annotations, and saved searches have explicit caps in `App/AnalysisContinuity.swift`.
- Plugin settings/state/annotations/custom data are kept under the application-support plugin store and each JSON store is capped at 64 KiB.
- Case exports are explicitly initiated by the analyst and bounded to 32 MiB; case integrity manifests are verified on import.

Operators should delete project directories, exported cases, and diagnostic files according to their organization’s retention schedule. HindSight does not infer a legal retention period and does not delete analyst-owned project data automatically.

## Opt-in crash and support diagnostics

The supported diagnostic path is explicit user action: **Support & Readiness → Copy diagnostics** or **Export diagnostics…**. The export is JSON, support-safe, and contains build identity, host/capability state, Coordinator/session state, bounded operation errors, and current workspace counts. It intentionally omits full project paths, target bytes, analysis report contents, credentials, and REA target paths are redacted in operation errors.

`HindSight.diagnostics.opt-in.v1` is reserved for a future organization-managed opt-in crash channel. It must not be interpreted as consent to upload until a separately documented transport, destination, payload schema, retention period, and disable mechanism exist. Until then, crash reports and diagnostics remain manual, local exports under the analyst’s control.

## Qualification boundary

Source-side identity, bounded diagnostics, crash parsing, and release preflight checks can be validated in the portable source environment. Developer ID/notarization credentials, signed-manifest publication, real-target behavior, memory-pressure behavior, and crash-diagnostics qualification remain release-host/manual gates listed in [`RELEASE_READINESS.md`](RELEASE_READINESS.md).
