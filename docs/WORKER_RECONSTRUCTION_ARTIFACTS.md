# Worker reconstruction artifacts: first transport slice

The local worker command below emits a real per-function artifact conforming to `schemas/reconstruction.schema.json`. It is a review-only machine-state representation, not recompilable recovered C or proof of equivalence.

```text
reworker --reconstruct ABSOLUTE_PATH SHA256 SIZE 0xSTART 0xEND REQUEST_UUID
```

Use a normalized function range returned by ordinary analysis. The worker verifies the captured regular file without following a final symlink, checks its SHA-256 and size, admits thin unencrypted little-endian AArch64 Mach-O, and requires the supplied range to match the shared function-boundary admission path. Its executable scan is limited to 4 MiB. The returned artifact uses the worker's existing four-byte big-endian length framing; errors use the existing error envelope. This command adds no listener, authentication or remote API.

Records link captured instruction bytes, decoded instructions, shared typed IR, a conservative instruction-granularity CFG, structured register SSA definitions/uses/phis, and machine-state AST nodes. The AST is built from shared IR, not parsed from pseudocode text. Rendered text is a structural review view with unresolved declarations and source-result ABI. CFG partitioning and reachability policy are explicit and need not match the presentation report's basic blocks or pruned edges.

Content identities include source digest, function, pass version and analysis-settings digest using the same canonical encoding as the offline Python validator. Request UUIDs do not alter content identities. Analysis engine identity remains `arm64-analysis-v89-schema122`; this additive command has its own `arm64-reconstruction-1` pass identity and does not extend the ordinary 72-field report.

The initial instruction budget is 128 words. Larger functions retain their full admitted bounds but publish only the bounded prefix and explicit truncation/exhaustion blockers. Unsupported instructions/effects, unresolved call contracts, unqualified runtime markers and unresolved flag writes produce blockers, even when conservatively captured outside known-edge reachability. Guest memory accesses carry an unqualified fault-model limitation. No artifact claims successful compilation, export eligibility or equivalence; review-only is not a semantic-completeness certificate.

Qualification: `python3 scripts/test-worker-reconstruction.py PATH_TO_REWORKER` compiles six CC0 AArch64 Mach-O fixtures without executing the imported targets. It checks real worker output with the independent Python validator, cross-language content identities, graph-to-byte queries, deterministic requests, SSA phi presence, unsupported/flag blockers, truncation and adversarial source/range/content/eligibility mutations. This test is included in macOS CI and the macOS qualification script.

Remaining: recovered variables and source-level types, complete effects and memory SSA, native catalog joins, semantically validated source AST, strict C17/Objective-C emission, recompile/behavior gates and coordinator/client integration. Those remain open in C00–C11 of the master roadmap.
