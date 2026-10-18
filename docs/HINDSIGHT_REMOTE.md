# HindSight Remote: app-hosted server contract

User decisions recorded 2026-10-18. This is the integration and acceptance contract, not a claim that the remote server or client is implemented or deployed.

The HindSight macOS application hosts the server. Its coordinator, analysis workers, debugger and VM services perform the heavy work on the user's machine; authenticated clients send requests and receive results and events. Reuse existing project, target, session and evidence identities rather than implement a second analysis engine. A separately installed server is not required for this initial architecture.

ZenSoftware's public portal supplies the demonstration, downloads and documentation. The user reports a web client is being implemented for `zensoftware/HindSight/webclient`; its deployment and interoperability have not been verified here. Real binaries, projects, credentials and execution streams belong on the user's selected HindSight host, not the public demo. The demo uses synthetic/offline data.

Whether the client lives in this repository, another branch, or a separate repository is deliberately deferred until the server is complete. Server contracts must not depend on that choice. AArch64 pseudocode and evidence-backed source reconstruction remain the analysis-engine priority.

## Client communication readiness gate

Current checkpoint: the app has an opt-in, loopback-only TLS listener with a
per-start bearer token. Protocol v1 exposes `GET /v1/capabilities`,
`GET /v1/project`, and an asynchronous captured-source analysis job:
`POST /v1/jobs/analysis`, `GET /v1/jobs/{request_id}`,
`GET /v1/jobs/{request_id}/result`, `DELETE /v1/jobs/{request_id}`.
The POST body requires a client-generated UUID `request_id`, exact
`project_path`, `record_id`, and `source_sha256`; retries with the same ID and
source return the existing job for up to ten minutes. Results are returned
only when the project and source still match. No browser cross-origin access,
LAN exposure, uploads, event streaming, debugger or VM control is enabled.
The gateway itself passed a loopback TLS client smoke test with certificate
verification, bearer authentication (200), unauthenticated rejection (401),
host rejection (400), and a bounded JSON POST (202). Run the full automated
transactional gate with `bash scripts/test-server-transactions.sh` on macOS;
`--quick` substitutes arm64/x86_64 app-target type-checking for the full build
when another workstream is editing the native worker.
It exercises the production job state machine against a controlled coordinator
(admission, retry idempotency, stale-source/result rejection, polling, limits,
cancellation and stop cleanup), the real TLS gateway, project-store security
contracts, and the full app/coordinator/worker build. The full app → coordinator → worker analysis
job path has not yet passed a real client interoperability test, and the
server is not ready for release. The dependency-ordered next actions are in the
[Chimera current queue](CHIMERA_COMPLETION_ROADMAP.md#current-actionable-queue-2026-10-18).

All items below are open until demonstrated against the app-hosted implementation:

- [ ] Explicit server enable/disable and remote-access configuration in the app; encrypted connections, authenticated sessions, revocation and project/target-scoped authorization.
- [ ] Versioned API and event contracts, compatibility negotiation and truthful capability discovery. Preserve 64-bit addresses as strings in browser-facing JSON.
- [ ] Integrity-checked, bounded binary uploads/imports and server-backed projects; importing never launches a binary. Enforce size, storage and analysis-resource limits.
- [ ] A real client can submit an analysis request and receive progress and source-bound results, including disassembly, pseudocode, symbols and evidence identities. Reject stale project/analysis revisions.
- [ ] Bounded event streaming with sequencing, backpressure, disconnect/reconnect behavior and cancellation; connection loss cannot silently repeat an execution request.
- [ ] Debug operations require explicit authorization and execution consent. Enforce controller ownership versus observers, maintain an audit trail, and keep debugger transports private behind the server gateway.
- [ ] VM operations require explicit execution consent, resource limits and isolated defaults with networking disabled initially. Advertise only qualified host/backend capabilities; expose console, lifecycle and debugging features only where actually supported.
- [ ] Verify app restart, service failure, malformed requests, unauthorized access, resource exhaustion and session cleanup, alongside a documented client interoperability test.

Native debugger and VM facilities are integration points, not evidence that these remote gates already pass. A worker's local framed output is likewise not an authenticated network server.

## Linux processing backend

[Docker worker support](LINUX_DOCKER.md) packages the portable Rust/C analysis
engine for Linux amd64/arm64 with a bounded, non-root, network-disabled host-side
launcher. This is one-shot static analysis/paging/reconstruction, not native
target execution, Linux attach, a network service, or automatic coordinator
scheduling. The app-hosted server must eventually discover and schedule this
backend through a trusted adapter, preserve source/job identities, stream its
framed results, and enforce authorization/cancellation before advertising remote
Linux processing. Apple runtime/SDK evidence discovery remains a macOS capability;
shared static Mach-O/dyld parsing does not make Linux a Darwin runtime oracle.
All client-readiness gates above remain open.

## GitHub handoff

The user has authorized a GitHub push once the server is complete and ready for client communication. Before pushing, audit correctness, completeness and security, record findings and resolve release-blocking issues, then demonstrate a real client connection against the app-hosted server. Verify the gate above, review the exact changes and destination, exclude credentials, captured binaries and unrelated work, then commit and push the tested server scope. The configured destination at the time of this decision is `BrokenPorkMachine/HindSight`, branch `main`; recheck before publishing. Do not hold server completion on the deferred client-repository decision, and do not treat analysis-engine-only progress as server readiness.
