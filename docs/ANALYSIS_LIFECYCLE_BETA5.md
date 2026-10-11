# Analysis lifecycle — 0.4.0-beta.5

Beta testing exposed two independent failures in the static-analysis path.

1. `reworker` emitted the current 60-field `analysis_result`, while CoordinatorService still required the early 13-field report. Valid results were rejected after the worker completed.
2. The worker transport still applied the 256 KiB control-message limit to analysis. Current bounded reports can legitimately exceed that size. The app-side XPC client independently rejected replies over 2 MiB.

0.4.0-beta.5 makes analysis a distinct bounded bulk response class:

| Boundary | Limit |
| --- | ---: |
| capture/page/control worker frame | 256 KiB |
| analysis worker frame | 8 MiB |
| ordinary XPC reply | 2 MiB |
| analysis XPC reply | 12 MiB |

The decoded `AnalysisReport` continues to bound each textual evidence section individually. The larger frame limit is therefore a transport envelope, not permission for unbounded analysis output.

## Worker supervision

Worker stdout, stderr, and process exit are observed concurrently. `WorkerSupervisor` no longer calls `Process.waitUntilExit()` or `DispatchGroup.wait()` from a user-initiated dispatch block. Completion is published only after an asynchronous dispatch-group notification confirms that both pipes reached EOF and the process terminated.

Cancellation revokes the governor lease, sends terminate, escalates to `SIGKILL` after 500 ms if required, and suppresses late publication.

## Regression gates

`Tests/analysis_protocol_contract.swift` constructs and decodes a valid modern analysis report larger than 256 KiB, verifies the exact 60-field schema, verifies unexpected fields are rejected by the field contract, and confirms the encoded XPC reply remains inside its bounded envelope.

`validate-source.py` also compares the Rust worker's emitted analysis keys to Swift's `AnalysisReport.jsonFields` and rejects synchronous worker waits in `WorkerSupervisor`.
