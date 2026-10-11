# HindSight 0.4.8 — Review Journal & Handoff

HindSight 0.4.8 extends the project-local investigation continuity layer without changing the Rust analysis worker protocol, the exact 66-field `AnalysisReport`, project SQLite schema, cache envelope schema, or `arm64-analysis-v11-schema66` engine identity.

## Review journal

Each investigation collection can now carry a bounded analyst-authored journal. Journal entries record:

- one explicit workflow state: **Open**, **In Review**, **Follow Up**, or **Reviewed**;
- an optional note capped at 2 KiB of UTF-8 text;
- the exact preserved pin identities that belonged to the collection when the entry was recorded; and
- an ISO-8601 timestamp.

The journal is operational analyst context. A workflow state is not a claim that the evidence is correct, vulnerable, exploitable, reachable, fixed, important, or semantically understood.

Continuity schema 4 accepts schemas 1–3 and migrates them in place on the next write. The same 1 MiB project-local continuity ceiling remains in force. Review journal storage is additionally bounded to 256 entries globally and 64 entries per collection.

## Handoff summary

The Review surface can generate a Markdown handoff for one collection and copy it to the clipboard. The handoff is bounded and contains:

- collection and artifact identity;
- captured source SHA-256 when available;
- current workflow state;
- collection health against the bounded currently loaded report corpus;
- exact-identity review drift, when a baseline exists;
- preserved collection evidence; and
- the newest 16 review journal entries.

The handoff keeps the same interpretability boundary as the rest of Analysis Triage: health, review drift, workflow state, and collection membership are provenance/continuity signals, not risk or semantic scoring.

## Final decisions and conflict-safe handoff

The Review workflow supports explicit **Approved** and **Rejected** decisions. Both require a bounded rationale note. A handoff is ready only when the decision has an exact-identity baseline for the current artifact record and source SHA-256, the collection has no membership/presence drift, and no follow-ups remain open. Otherwise the Review surface and handoff report the blocking condition and the available remediation, such as refreshing the baseline or resolving the follow-up.

## Responsive review workflow

Review Journal & Handoff works with a single collection. Pairwise collection comparison remains available when two or more collections exist. Controls use `VGFlowLayout`, and the visible journal is bounded to 12 recent entries so the Review surface does not reintroduce fixed-width or unbounded-density regressions.

## Compatibility

0.4.8 retains:

- protocol ABI 1;
- the exact 66-field `AnalysisReport`;
- `arm64-analysis-v11-schema66`;
- cache envelope schema 2; and
- project SQLite schema 1.

Compatible 0.4.2–0.4.7 schema-2 analysis caches remain reusable when existing source/engine identity checks succeed.
