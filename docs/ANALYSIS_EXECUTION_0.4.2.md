# HindSight 0.4.2 — Analysis execution model

## Goal

0.4.2 makes long-running firmware analysis observable without weakening HindSight's evidence boundary. The final analysis report is still one bounded, identity-checked publication. Progress is operational telemetry, not partial evidence.

## Progress transport

The bundled Rust worker reserves stdout for its existing length-framed result. During `--analyze`, it may emit ASCII progress records on stderr:

```text
VG_PROGRESS\t<0-100>\t<stage>\t<detail>
```

Coordinator accepts a progress record only when the percentage is in 0…100 and the stage/detail fields are non-empty and bounded. Accepted progress lines update `InspectionStatus`; they are not appended to the diagnostic buffer. Any other stderr remains bounded diagnostic output and is attached to a real worker failure when useful.

Current milestones are deliberately coarse: source verification, Mach-O mapping/metadata, instruction decode, CFG construction, structural/evidence synthesis, evidence formatting, final identity check/serialization, result validation, and publication. Percentages are monotonic pipeline position. They are **not an ETA** and do not claim equal work between stages.

## Cancellation and publication

Cancellation continues to revoke the worker lease, terminate the isolated process, and escalate to `SIGKILL` if necessary. The last reported stage remains visible in the terminal status so a cancelled or failed run says where it stopped.

HindSight intentionally does not stream half-built `AnalysisReport` sections into the evidence UI. A report becomes authoritative only after the complete bounded frame passes exact-field, request-ID, source-SHA-256, size, and report-limit validation. If a prior valid report is already displayed, starting another operation does not mutate that report until the replacement validates.

## Cache envelope v2

0.4.1 keyed cached reports to the complete product version. That was safe but caused unnecessary recomputation across patch releases even when the analysis engine and report schema were unchanged. 0.4.2 introduces cache schema 2:

- `engineVersion` — semantic identity of the analysis implementation/report contract (`arm64-analysis-v11-schema66` in 0.4.2).
- `producerVersion` — HindSight version that wrote the envelope.
- `sourceSHA256` — captured-object identity.
- `report` — the same bounded, exact-field `AnalysisReport`.

0.4.2 recognizes a schema-1 cache produced by 0.4.1 as a one-time compatible legacy source because the analysis engine/report contract is unchanged. The report is rebound to the new request UUID only after validation and is then rewritten as schema 2 when possible. Future releases must explicitly opt into compatibility; producer version alone never authorizes reuse.

Cache read/decoding errors are non-fatal. They fall through to a fresh isolated analysis. Source metadata is stamped before cache lookup and checked again before any cached result is published.

## Security and trust boundaries

- No progress record changes the final analysis payload or report identity.
- Progress parsing is strict and bounded; malformed records fall back to ordinary diagnostics.
- Cache files remain project-local, symlink-rejected, size-bounded, atomically written, and exact-schema validated.
- Project database schema remains version 1; no migration is required.
- Protocol ABI remains 1; the app and Coordinator must still match exact product version/channel/capability handshake before project actions are enabled.
