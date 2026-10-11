# HindSight 0.4.6 — Investigation Workflow & Collection Health

HindSight 0.4.6 extends the project-local investigation layer without changing the analysis worker protocol, report schema, project SQLite schema, or analysis-engine identity.

## Collection search

The Collections view can filter by collection name and by the preserved evidence text, source label, or explicit symbol identity represented by its pins. Filtering is presentation-only and does not mutate project evidence.

## Collection rename

Collections can be renamed in place. Names are bounded to 96 characters, remain unique per artifact ignoring case, and are organizational labels only. Pin IDs, timestamps, captured evidence text, and provenance are not rewritten when a collection is renamed.

## Bounded collection health

When a validated `AnalysisReport` is loaded, HindSight builds a bounded corpus of up to 25,000 structured evidence rows and derives their stable project pin identities. A collection reports how many preserved pins are present in that corpus. Pins absent from the bounded corpus are labelled preserved. This signal is intentionally conservative: preserved evidence is **not** declared false, stale, unimportant, unreachable, or fixed. The report may be bounded, and evidence can legitimately change across analyses.

When no report is loaded, health is reported as `analysis not loaded` rather than guessed.

## Evidence history traversal

Recent Source, Code, CFG, and Symbol pivots remain bounded project-local history. 0.4.6 adds Previous and Next traversal over that history. Traversal replays an existing visit without appending another visit, while a newly opened evidence pivot resets the transient traversal cursor to the newest position.

## Compatibility

0.4.6 retains protocol ABI 1, the exact 66-field `AnalysisReport`, cache envelope schema 2, project database schema v1, and analysis engine identity `arm64-analysis-v11-schema66`. Compatible 0.4.2–0.4.5 cached analyses therefore remain eligible for reuse when their captured-source and engine identities validate.
