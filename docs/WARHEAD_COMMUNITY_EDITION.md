# Warhead Community — staged HindSight edition (NOT RELEASED)

## Repository ownership

- `BrokenPorkMachine/warhead` is the **private source-of-truth** for the owner's full Warhead suite. This branch does not change it.
- `BrokenPorkMachine/HindSight` hosts the **public-facing, separately maintained Warhead Community edition** as `BundledPlugins/Warhead.vgplugin`.
- Existing HindSight already contained a Warhead runtime copy. This increment hardens **that copy**; it does **not** mirror the private repository or publish private Core, Team, Ready Room, credentials, or research configurations.
- The bundled runtime's current `LICENSE` is proprietary. Code visibility in a public Git repository is **not** an open-source grant. Select a public-edition license only after copyright/dependency review and approval.
- A private Warhead `bundle:hindsight` command currently stages general `dist` and private `LICENSE` into HindSight. **Do not run that command to publish Community**. Future updates must use an independently reviewed, Community-only release pipeline.

## Initial enforced runtime policy (preview)

The Community tool boundary is in
`BundledPlugins/Warhead.vgplugin/runtime/community/guardrails.js`.
The bundled `tools/registry.js` applies it **before** caches, configuration or tool execution. The registry does not advertise excluded tools.

| Capability | Community preview |
| --- | --- |
| Bounded in-memory Studio snapshot (`hindsight_context`) | Allowed only with explicit `communitySnapshot` invocation context; direct filesystem export reader denied |
| Session-local todo list (`todo`) | Allowed |
| `read_file` | Only when the host provides an explicit authorized workspace root; canonical path checked; sensitive path names denied |
| Shell/commands, writes/edits, REA, generic MCP, arbitrary dynamic tools, web search/fetch | Denied, regardless of model request or relaxed configuration |
| Interactive broad Warhead CLI | Disabled (unqualified execution/extension pathways) |
| Studio extension entrypoint | Existing bounded `runtime/cli/extension.js`, no shell or writes, 20-second agent deadline |

The private Warhead repository is **unmodified**, so none of these restrictions are imposed on the owner's independent private build.

## Threat model and limitations

This initial allowlist is defense-in-depth, **not a complete sandbox**.

1. The agent input, imported binaries, model output, and MCP/tool results are untrusted. Prompts must never be interpreted as authorizations.
2. Canonical path checks can still have filesystem race conditions. They do not stop every form of symlink swap; require OS isolation for more powerful file operations.
3. A local process with the user's own OS permissions can bypass a JavaScript policy by modifying its code or invoking other executables. OS-enforced sandboxing and a locked-down launcher are release prerequisites for autonomous operation.
4. The Studio extension's selected analysis may contain private data; the configured model endpoint receives that bounded data. Preserve informed consent, endpoint identification and privacy warnings.
5. Secrets must not be committed to the public repo. `node_modules`, binary blobs, old history, Git objects, environment examples, logs, and distribution bundles require scanning before release.
6. No new tool or CLI entrypoint may be made public simply because it passes the permission gate. Every execution path needs its own threat review and tests.
7. Never trust the model's claims of successful verification. Cite immutable evidence IDs and run independent checks.
8. Public branch publication must **not** automatically sync private commits; keep an explicit, audited source import/change-review process.

## Gates before opening the Community edition to users

- [ ] Audit every existing file and Git history in the proposed distribution; scan for credentials, private data, problematic third-party licenses, and unreviewed source.
- [ ] Establish the exact license for Community and compatibility with bundled dependencies; remove or replace the private proprietary notice only with the owner's approval.
- [ ] Build dedicated Community artifacts using an allowlisted manifest of files and modules, not a raw copy of private `dist`.
- [ ] Remove unsupported private CLI, REA, remote/server and broad tool entrypoints from the distribution (not just from tool discovery).
- [ ] Implement OS-level confinement on supported macOS/Linux targets for any agent-driven process launch or mutable filesystem access; verify it on the actual host.
- [ ] Add explicit per-project capability grants and policy-revocation tests; no privileged default mode.
- [ ] Preserve the local HindSight HTTPS gateway authentication, project grants, TLS pinning, short-lived token handling, and explicit remote controls.
- [ ] Red-team prompt-injection, symlink swaps, subcommand indirection, credentials, unauthorized network access, cancellation, budget exhaustion, and restart/recovery.
- [ ] Exercise actual HindSight GUI → Studio extension → model → evidence output, including failure/restart paths.
- [ ] Independently validate source archives, clean installs, packaging, and repeatable security/test reports.
- [ ] Document the edition's exact capability matrix and non-goals; no “secure” claim solely because a test is green.

## Development and guard tests

Run from HindSight root (Node 22+ and the bundled dependencies required):

```sh
node --test scripts/test-warhead-community.mjs
python3 scripts/test-warhead-bundle.py
```

The bundle static contract invokes the Community regression tests so a later accidental overwrite with the private runtime **fails closed**. This branch has not passed the full macOS application or release matrix yet.

## Release workflow

1. Update and test Warhead in private as before.
2. Select reviewed public-source commits or components, preserving provenance and licensing.
3. Stage the components in a scratch release worktree; do not copy the private checkout or private release ZIP.
4. Apply the Community runtime/packaging policy and run independent audits and all tests.
5. Open a HindSight PR with the source manifest, diff, attestation, test results and known limitations.
6. Merge/release only after explicit maintainer review. Private branch remains private.

The intended result is a useful, evidence-grounded, local-first research collaborator in HindSight — not a generic unrestricted coding agent distributed under a different name.
