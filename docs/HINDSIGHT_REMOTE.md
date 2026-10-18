# HindSight Remote: app-hosted server contract

User decisions recorded 2026-10-18. This is the integration and acceptance contract, not a claim that the remote server or client is implemented or deployed.

The HindSight macOS application hosts the server. Its coordinator, analysis workers, debugger and VM services perform the heavy work on the user's machine; authenticated clients send requests and receive results and events. Reuse existing project, target, session and evidence identities rather than implement a second analysis engine. A separately installed server is not required for this initial architecture.

ZenSoftware's public portal supplies the demonstration, downloads and documentation. The user reports a web client is being implemented for `zensoftware/HindSight/webclient`; its deployment and interoperability have not been verified here. Real binaries, projects, credentials and execution streams belong on the user's selected HindSight host, not the public demo. The demo uses synthetic/offline data.

Whether the client lives in this repository, another branch, or a separate repository is deliberately deferred until the server is complete. Server contracts must not depend on that choice. AArch64 pseudocode and evidence-backed source reconstruction remain the analysis-engine priority.

## Client communication readiness gate

Current checkpoint: the app has an opt-in TLS listener with a per-start bearer
token. It defaults to loopback; an explicit private IPv4 bind address and one
exact HTTPS browser origin can be configured. Wildcard/public binds and other
browser origins are rejected. Protocol v1 exposes a machine-readable OpenAPI
contract at `GET /v1/openapi.json`, truthful capability discovery at
`GET /v1/capabilities`, `GET /v1/project`, and an asynchronous captured-source analysis job:
`POST /v1/jobs/analysis`, `GET /v1/jobs/{request_id}`,
`GET /v1/jobs/{request_id}/result`, `DELETE /v1/jobs/{request_id}`.
`GET /v1/jobs/{request_id}` returns `{job_id, state}` and, while active,
includes `operation`, `detail`, `progress_percent`, `progress_stage`,
`elapsed_ms`, and `updated_at_ms`;
`state` is the coordinator phase (`queued`, `admitted`, `checkingCache`,
`launching`, `running`, `receiving`, `validating`, `publishing`,
`cancelling`, `completed`, `failed`, or `cancelled`). Millisecond counters
are decimal strings so browser clients do not lose integer precision.
The POST body requires a client-generated UUID `request_id`, exact
`project_path`, `record_id`, and `source_sha256`; retries with the same ID and
source return the existing job for up to ten minutes. Results are returned
only when the project and source still match. The OpenAPI document defines
request/response schemas, authentication, operation IDs, limits and errors for
every implemented route; capabilities report execution as disabled. An exact-origin CORS preflight
and response path is implemented. The v1 API is ready for a client to model
and integrate against; cross-device/browser interoperability still needs a
real browser on a second device. Uploads, event streaming, debugger and VM
control are disabled and are not advertised as supported capabilities.
The gateway itself passed a loopback TLS client smoke test with certificate
verification, bearer authentication (200), unauthenticated rejection (401),
host/origin rejection (400), exact-origin preflight (204), and a bounded JSON
POST (202). Run the full automated
transactional gate with `bash scripts/test-server-transactions.sh` on macOS;
`--quick` substitutes arm64/x86_64 app-target type-checking for the full build
when another workstream is editing the native worker.
It exercises the production job state machine against a controlled coordinator
(admission, retry idempotency, stale-source/result rejection, polling, limits,
cancellation and stop cleanup), the real TLS gateway, project-store security
contracts, the ordinary app/coordinator/worker build, and a test-only app
launch that imports a captured Mach-O via XPC and returns a source-bound
analysis result to a certificate-verifying HTTPS client. That local end-to-end
transaction passed on the current branch. The full gate passed the ordinary
universal app/coordinator/worker build and a real app → XPC → worker → HTTPS
client transaction. The gate also validates the served OpenAPI and capabilities
responses before submitting the analysis job. Remaining product work is actual
LAN/browser interoperability, uploads, event streaming, multi-project
authorization, and remote execution security. The dependency-ordered next
actions are in the
[Chimera current queue](CHIMERA_COMPLETION_ROADMAP.md#current-actionable-queue-2026-10-18).

## Security audit and supported-scope limits

The publication audit passed for the v1 analysis API. HTTPS uses a Keychain
identity and TLS 1.2 or newer. Clients must trust that certificate, and its
subject alternative name must match the server address. The listener binds to
loopback by default; remote binding accepts only RFC1918 IPv4 addresses and
requires one exact HTTPS browser origin. Bearer tokens are generated with
32 random bytes per start, compared without content-dependent byte exits,
kept in memory, and discarded on stop. The HTTP parser rejects duplicate
authorization/host/origin/length headers, transfer encoding, oversized
headers/bodies, nonmatching hosts/origins, and pipelined data. It limits
connections and request lifetime. Analysis is restricted to the current ready
project and captured source digest; request IDs are idempotent for ten minutes,
at most two jobs run concurrently, 256 jobs are retained, and only four results
are kept. Importing a binary does not execute it.

This is a single-user host-wide bearer-token model: every holder of the current
token can read the open project and submit/cancel its analysis jobs. It does
not provide individual accounts, multiple project grants, or audit logging.
Stop/start rotates the token. Do not expose the app through a public address or
port-forward it as a multi-user service.

Remaining remote-product scope:

- [x] Explicit server start/stop, loopback default, private IPv4 configuration, TLS, per-start bearer-token rotation, exact-origin CORS, versioned API, OpenAPI endpoint and truthful capability discovery.
- [ ] Individual users, multiple project grants, durable audit log and finer-grained project/target authorization.
- [ ] Integrity-checked, bounded binary uploads/imports and server-backed projects; importing never launches a binary. Enforce size, storage and analysis-resource limits.
- [x] Source-bound analysis submission, polling, cancellation and result retrieval, with retry idempotency and stale-source rejection. The result carries the existing disassembly, pseudocode, symbols and evidence artifacts.
- [ ] Real browser interoperability from a second device; bounded event streaming with sequencing, backpressure, disconnect/reconnect behavior and cancellation.
- [ ] Debug operations require explicit authorization and execution consent. Enforce controller ownership versus observers, maintain an audit trail, and keep debugger transports private behind the server gateway.
- [ ] VM operations require explicit execution consent, resource limits and isolated defaults with networking disabled initially. Advertise only qualified host/backend capabilities; expose console, lifecycle and debugging features only where actually supported.
- [~] Verify app restart and service failure recovery, alongside the passing malformed-request, unauthorized-access, resource-limit and session-cleanup tests.

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
The remaining remote-product gates above remain open.

## GitHub handoff

The user has authorized a GitHub push once the server is complete and ready for client communication. Before pushing, audit correctness, completeness and security, record findings and resolve release-blocking issues, then demonstrate a real client connection against the app-hosted server. Verify the gate above, review the exact changes and destination, exclude credentials, captured binaries and unrelated work, then commit and push the tested server scope. The configured destination at the time of this decision is `BrokenPorkMachine/HindSight`, branch `main`; recheck before publishing. Do not hold server completion on the deferred client-repository decision, and do not treat analysis-engine-only progress as server readiness.
