# Analysis reliability & observability — 0.4.0-beta.6

HindSight analysis is a bounded asynchronous worker lifecycle. beta.6 exposes that lifecycle without turning diagnostics into an unbounded log stream.

## Phases

`admitted → launching → running → receiving → validating → publishing → completed`

Cancellation transitions through `cancelling → cancelled`. Any transport, worker, schema, identity, or publication failure terminates as `failed`.

## Bounds

- capture/page worker deadline: 120 seconds
- analysis worker deadline: 240 seconds
- analysis client envelope: 260 seconds
- retained recent request statuses: 32
- worker diagnostic capture: 32 KiB
- control/page worker frame: 256 KiB
- analysis worker frame: 8 MiB
- analysis XPC reply: 12 MiB

Status is diagnostic and ephemeral. It is not inserted into the project database and does not replace Investigation Ledger evidence. A status-poll timeout is intentionally non-fatal: it fails only that poll and cannot invalidate the XPC generation carrying the primary analysis request.

Cancellation revokes the Rust publication lease before requesting process termination. A cancelled or revoked job cannot publish a late analysis result. Terminal worker diagnostics are retained in the recent status record and are also attached to worker-result failures when available.
