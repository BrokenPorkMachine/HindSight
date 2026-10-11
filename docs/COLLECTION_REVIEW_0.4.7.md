# HindSight 0.4.7 — Collection Review & Evidence Drift

HindSight 0.4.7 extends the project-local investigation layer without changing the analysis worker protocol, report schema, project database schema, or analysis-engine identity.

## Review checkpoints

Any investigation collection can be explicitly marked reviewed. The checkpoint stores only bounded continuity metadata:

- the collection ID and artifact record ID;
- the captured source SHA-256 when available;
- the collection's exact preserved pin identities at review time;
- when analysis is loaded, the subset of those identities present in the bounded validated analysis corpus;
- the review timestamp.

Review checkpoints do not duplicate analyst notes or binary content. They remain inside `.vectorglass/analysis-continuity-v1.json`, which retains the 1 MiB file budget, regular-file/symlink admission checks, and atomic publication behavior.

## Evidence drift

HindSight compares the current collection state to its last explicit review checkpoint and reports:

- findings added to collection membership;
- findings removed from collection membership;
- reviewed findings that now appear in the bounded current analysis corpus;
- reviewed findings that were current at review time but are now preserved-only.

These are exact-identity/provenance transitions. A preserved or changed finding is not automatically invalid, fixed, vulnerable, important, reachable, or semantically obsolete.

## Pairwise collection comparison

The Analysis Triage **Review** mode compares two collections by preserved pin identity and exposes three bounded navigable sets:

1. evidence shared by both collections;
2. evidence only in collection A;
3. evidence only in collection B.

The review UI uses the same responsive flow/card primitives as the rest of the 0.4.x workbench and caps each rendered comparison section while retaining the exact aggregate counts.

## Compatibility

The continuity document advances to schema 3 and continues to read schema 1 and schema 2 documents. The storage filename remains unchanged, so existing project continuity state upgrades naturally on the next write.

0.4.7 retains:

- protocol ABI 1;
- the exact 66-field `AnalysisReport`;
- analysis engine `arm64-analysis-v11-schema66`;
- cache envelope schema 2;
- project SQLite schema v1.

Compatible 0.4.2–0.4.6 analysis caches therefore remain eligible for reuse when source and engine identity checks succeed.
